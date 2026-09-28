use crate::async_queue::{AsyncRestorationQueue, RestorationTask};
use crate::cas::{unique_temp_id, CasStorage};
use crate::constants::{DIR_BASELINES, DIR_OBJECTS, DIR_TMP, EXT_JAR, FILE_MANIFEST};
use crate::domain::{JvmCacheError, Manifest, ModuleBaseline, OutputDirTarget};
use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub struct ManifestStore;

impl ManifestStore {
    pub fn object_dir(root_dir: &Path, key: &str) -> PathBuf {
        let prefix = if key.len() >= 2 { &key[..2] } else { "00" };
        root_dir.join(DIR_OBJECTS).join(prefix).join(key)
    }

    pub fn manifest_path(root_dir: &Path, key: &str) -> PathBuf {
        Self::object_dir(root_dir, key).join(FILE_MANIFEST)
    }

    pub fn get_manifest(root_dir: &Path, key: &str) -> Option<Manifest> {
        let path = Self::manifest_path(root_dir, key);
        if !path.is_file() {
            return None;
        }
        let file = File::open(&path).ok()?;
        serde_json::from_reader(file).ok()
    }

    pub fn store(
        root_dir: &Path,
        manifest: &Manifest,
        output_dirs: &[OutputDirTarget],
    ) -> Result<(), JvmCacheError> {
        let key = &manifest.cache_key;
        let final_dir = Self::object_dir(root_dir, key);
        if final_dir.exists() {
            return Ok(());
        }

        let tmp_dir = root_dir.join(DIR_TMP).join(unique_temp_id(key));
        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
        fs::create_dir_all(&tmp_dir)?;

        let tasks: Vec<RestorationTask> = manifest
            .artifacts
            .iter()
            .filter_map(|art| {
                let target_dest = output_dirs.iter().find(|d| d.tag == art.target_tag)?;
                let target_root = &target_dest.path;
                let is_jar_dest = target_root.extension().and_then(|e| e.to_str()) == Some(EXT_JAR);
                let src = if is_jar_dest {
                    target_root.clone()
                } else {
                    target_root.join(&art.rel_path)
                };
                let blob_dst = CasStorage::blob_path(root_dir, &art.sha256);
                if blob_dst.is_file() {
                    None
                } else {
                    Some(RestorationTask { src, dst: blob_dst })
                }
            })
            .collect();

        let mut created_cas_parents: HashSet<&Path> = HashSet::new();
        for task in &tasks {
            if let Some(bp) = task.dst.parent() {
                if created_cas_parents.insert(bp) {
                    let _ = fs::create_dir_all(bp);
                }
            }
        }

        let bg_handle = AsyncRestorationQueue::spawn_restoration(tasks);
        bg_handle.join()?;

        let manifest_file = File::create(tmp_dir.join(FILE_MANIFEST))?;
        serde_json::to_writer_pretty(manifest_file, manifest)?;

        if let Some(parent) = final_dir.parent() {
            fs::create_dir_all(parent)?;
        }

        let _ = fs::rename(&tmp_dir, &final_dir);
        Ok(())
    }

    pub fn baselines_dir(root_dir: &Path) -> PathBuf {
        root_dir.join(DIR_BASELINES)
    }

    pub fn get_baseline(root_dir: &Path, module_key: &str) -> Option<ModuleBaseline> {
        let path = Self::baselines_dir(root_dir).join(format!("{}.json", module_key));
        if !path.is_file() {
            return None;
        }
        let file = File::open(&path).ok()?;
        serde_json::from_reader(file).ok()
    }

    pub fn save_baseline(
        root_dir: &Path,
        baseline: &ModuleBaseline,
    ) -> Result<(), JvmCacheError> {
        let dir = Self::baselines_dir(root_dir);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", &baseline.module_key));
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, baseline)?;
        Ok(())
    }

    pub fn remove_baseline(root_dir: &Path, module_key: &str) {
        let path = Self::baselines_dir(root_dir).join(format!("{}.json", module_key));
        let _ = fs::remove_file(path);
    }
}
