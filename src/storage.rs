use crate::async_queue::{AsyncRestorationQueue, RestorationTask};
use crate::cas::CasStorage;
use crate::config::JvmCacheConfig;
use crate::constants::{
    CACHE_CLEANUP_WATERMARK_RATIO, DIR_BASELINES, DIR_CAS, DIR_OBJECTS, DIR_TMP, EXT_JAR,
    FILE_MANIFEST, FILE_STATS,
};
use crate::domain::{CacheStats, JvmCacheError, Manifest, ModuleBaseline, OutputDirTarget};
use crate::manifest_store::ManifestStore;
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub use crate::cas::{copy_or_link, unique_temp_id};

pub struct CacheStorage {
    root_dir: PathBuf,
    config: JvmCacheConfig,
}

impl CacheStorage {
    pub fn with_config(config: JvmCacheConfig) -> Result<Self, JvmCacheError> {
        let root = config.cache_dir.clone();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join(DIR_OBJECTS))?;
        fs::create_dir_all(root.join(DIR_CAS))?;
        fs::create_dir_all(root.join(DIR_TMP))?;
        fs::create_dir_all(root.join(DIR_BASELINES))?;
        Ok(Self { root_dir: root, config })
    }

    pub fn new() -> Result<Self, JvmCacheError> {
        Self::with_config(JvmCacheConfig::load())
    }

    pub fn config(&self) -> &JvmCacheConfig {
        &self.config
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn get_manifest(&self, key: &str) -> Option<Manifest> {
        ManifestStore::get_manifest(&self.root_dir, key)
    }

    pub fn build_restoration_tasks(
        &self,
        manifest: &Manifest,
        output_dirs: &[OutputDirTarget],
        excluded_prefixes: &[String],
    ) -> Result<Vec<RestorationTask>, JvmCacheError> {
        let key = &manifest.cache_key;
        let artifacts_base = ManifestStore::object_dir(&self.root_dir, key).join("artifacts");

        let mut tasks = Vec::with_capacity(manifest.artifacts.len());
        for art in &manifest.artifacts {
            if let Some(name) = art.rel_path.file_name().and_then(|n| n.to_str()) {
                if excluded_prefixes
                    .iter()
                    .any(|p| crate::delta_detector::DeltaDetector::matches_source_stem(name, p))
                {
                    continue;
                }
            }

            let blob_path = CasStorage::blob_path(&self.root_dir, &art.sha256);
            let src = if blob_path.is_file() {
                blob_path
            } else {
                let legacy = artifacts_base.join(&art.target_tag).join(&art.rel_path);
                if legacy.is_file() {
                    legacy
                } else {
                    return Err(JvmCacheError::Execution(
                        "One or more cached CAS artifacts are missing or corrupted".to_string(),
                    ));
                }
            };

            let target_dest = match output_dirs.iter().find(|d| d.tag == art.target_tag) {
                Some(d) => d,
                None => continue,
            };
            let target_root = &target_dest.path;
            let dst = if target_root.extension().and_then(|e| e.to_str()) == Some(EXT_JAR) {
                target_root.clone()
            } else {
                target_root.join(&art.rel_path)
            };

            tasks.push(RestorationTask { src, dst });
        }

        let mut created_parents: HashSet<&Path> = HashSet::new();
        for task in &tasks {
            if let Some(p) = task.dst.parent() {
                if created_parents.insert(p) {
                    let _ = fs::create_dir_all(p);
                }
            }
        }

        Ok(tasks)
    }

    pub fn restore_artifacts(
        &self,
        manifest: &Manifest,
        output_dirs: &[OutputDirTarget],
    ) -> Result<(), JvmCacheError> {
        let tasks = self.build_restoration_tasks(manifest, output_dirs, &[])?;
        AsyncRestorationQueue::spawn_restoration(tasks).join()
    }

    pub fn store(
        &self,
        manifest: &Manifest,
        output_dirs: &[OutputDirTarget],
    ) -> Result<(), JvmCacheError> {
        ManifestStore::store(&self.root_dir, manifest, output_dirs)?;
        let bytes_stored: u64 = manifest.artifacts.iter().map(|a| a.size_bytes).sum();
        self.record_miss(bytes_stored);
        Ok(())
    }

    pub fn record_hit(&self) {
        let mut stats = self.load_stats();
        stats.hits += 1;
        self.save_stats(&stats);
    }

    pub fn record_miss(&self, bytes: u64) {
        let mut stats = self.load_stats();
        stats.misses += 1;
        stats.bytes_cached += bytes;
        self.save_stats(&stats);
        let _ = self.enforce_max_size();
    }

    pub fn record_passthrough(&self) {
        let mut stats = self.load_stats();
        stats.direct_passthrough += 1;
        self.save_stats(&stats);
    }

    pub fn load_stats(&self) -> CacheStats {
        let path = self.root_dir.join(FILE_STATS);
        if let Ok(file) = File::open(path) {
            serde_json::from_reader(file).unwrap_or_default()
        } else {
            CacheStats::default()
        }
    }

    fn save_stats(&self, stats: &CacheStats) {
        let path = self.root_dir.join(FILE_STATS);
        let tmp_path = self.root_dir.join(unique_temp_id("stats"));
        if let Ok(file) = File::create(&tmp_path) {
            if serde_json::to_writer_pretty(file, stats).is_ok() {
                let _ = fs::rename(tmp_path, path);
            }
        }
    }

    pub fn enforce_max_size(&self) -> Result<(), JvmCacheError> {
        let max_bytes = self.config.max_cache_size_mb * 1024 * 1024;
        let mut stats = self.load_stats();
        if stats.bytes_cached <= max_bytes {
            return Ok(());
        }

        let target_bytes = (max_bytes as f64 * CACHE_CLEANUP_WATERMARK_RATIO) as u64;
        let objects_dir = self.root_dir.join(DIR_OBJECTS);
        if !objects_dir.exists() {
            return Ok(());
        }

        let mut manifests = Vec::new();
        for entry in walkdir::WalkDir::new(&objects_dir)
            .min_depth(2)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            if entry.file_type().is_file() && entry.file_name() == FILE_MANIFEST {
                if let Ok(content) = fs::read_to_string(entry.path())
                    && let Ok(manifest) = serde_json::from_str::<Manifest>(&content)
                {
                    let total_bytes: u64 = manifest.artifacts.iter().map(|a| a.size_bytes).sum();
                    manifests.push((entry.path().to_path_buf(), manifest.created_at_epoch_secs, total_bytes));
                }
            }
        }

        manifests.sort_by_key(|m| m.1);

        for (m_path, _created, bytes) in manifests {
            if stats.bytes_cached <= target_bytes {
                break;
            }
            if let Some(parent) = m_path.parent() {
                let _ = fs::remove_dir_all(parent);
            }
            stats.bytes_cached = stats.bytes_cached.saturating_sub(bytes);
        }

        self.save_stats(&stats);
        Ok(())
    }

    pub fn clear(&self) -> Result<(), JvmCacheError> {
        for dir in [DIR_OBJECTS, DIR_CAS, DIR_TMP] {
            let p = self.root_dir.join(dir);
            if p.exists() {
                fs::remove_dir_all(&p)?;
                fs::create_dir_all(&p)?;
            }
        }
        let _ = fs::remove_file(self.root_dir.join(FILE_STATS));
        Ok(())
    }

    pub fn baselines_dir(&self) -> PathBuf {
        ManifestStore::baselines_dir(&self.root_dir)
    }

    pub fn get_baseline(&self, module_key: &str) -> Option<ModuleBaseline> {
        ManifestStore::get_baseline(&self.root_dir, module_key)
    }

    pub fn save_baseline(&self, baseline: &ModuleBaseline) -> Result<(), JvmCacheError> {
        ManifestStore::save_baseline(&self.root_dir, baseline)
    }

    pub fn remove_baseline(&self, module_key: &str) {
        ManifestStore::remove_baseline(&self.root_dir, module_key)
    }
}
