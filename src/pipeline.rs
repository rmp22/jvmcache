use crate::compiler::CompilerRunner;
use crate::config::JvmCacheConfig;
use crate::constants::{TAG_CLASSES, TAG_KAPT_STUBS};
use crate::delta_pipeline::DeltaPipeline;
use crate::domain::{CompilerKind, JvmCacheError, Manifest, ModuleBaseline, ParsedArgs};
use crate::storage::CacheStorage;
use crate::telemetry::TelemetryLogger;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct CachePipeline;

impl CachePipeline {
    pub fn resolve_primary_classes_dir(parsed: &ParsedArgs) -> PathBuf {
        parsed
            .output_dirs
            .iter()
            .find(|d| d.tag == TAG_CLASSES || d.tag == TAG_KAPT_STUBS)
            .map(|d| d.path.clone())
            .or_else(|| parsed.output_dirs.first().map(|d| d.path.clone()))
            .unwrap_or_default()
    }

    pub fn handle_passthrough(
        parsed: &ParsedArgs,
        config: &JvmCacheConfig,
        storage: Option<&CacheStorage>,
        target_label: &str,
        t_start: Instant,
    ) -> Result<i32, JvmCacheError> {
        if let Some(s) = storage {
            s.record_passthrough();
        }
        TelemetryLogger::log_passthrough(
            &config.cache_dir,
            parsed.compiler,
            target_label,
            "non-compilation or storage disabled",
            t_start.elapsed(),
        );
        let output = Command::new(&parsed.real_compiler_path)
            .args(&parsed.raw_args)
            .output()?;
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stderr().write_all(&output.stderr);
        Ok(output.status.code().unwrap_or(0))
    }

    pub fn handle_hash_fallback(
        parsed: &ParsedArgs,
        config: &JvmCacheConfig,
        target_label: &str,
        reason: &str,
        t_start: Instant,
    ) -> Result<i32, JvmCacheError> {
        TelemetryLogger::log_passthrough(
            &config.cache_dir,
            parsed.compiler,
            target_label,
            &format!("key computation fallback: {}", reason),
            t_start.elapsed(),
        );
        let output = Command::new(&parsed.real_compiler_path)
            .args(&parsed.raw_args)
            .output()?;
        let _ = io::stdout().write_all(&output.stdout);
        let _ = io::stderr().write_all(&output.stderr);
        Ok(output.status.code().unwrap_or(0))
    }

    pub fn try_exact_cache_hit(
        storage: &CacheStorage,
        key: &str,
        parsed: &ParsedArgs,
        config: &JvmCacheConfig,
        target_label: &str,
        module_key: &str,
        primary_classes_dir: &Path,
        source_hashes: &HashMap<PathBuf, String>,
        classpath_hash: &str,
        t_start: Instant,
        hash_time: Duration,
    ) -> Result<Option<i32>, JvmCacheError> {
        let manifest = match storage.get_manifest(key) {
            Some(m) => m,
            None => return Ok(None),
        };

        let is_incomplete = (parsed.source_files.len() > 10
            && manifest.artifacts.len() < parsed.source_files.len() / 2)
            || (parsed.compiler != CompilerKind::Kapt
                && parsed.output_dirs.iter().any(|d| d.tag == TAG_CLASSES)
                && !parsed.source_files.is_empty()
                && !manifest.artifacts.iter().any(|a| a.target_tag == TAG_CLASSES))
            || (parsed.compiler == CompilerKind::Kapt
                && manifest.artifacts.iter().any(|a| {
                    a.target_tag == TAG_KAPT_STUBS
                        && a.rel_path.extension().and_then(|e| e.to_str()) == Some("java")
                        && DeltaPipeline::is_invalid_kapt_stub(&crate::cas::CasStorage::blob_path(storage.root_dir(), &a.sha256))
                }));
        if is_incomplete {
            return Ok(None);
        }

        let t_restore_start = Instant::now();
        if storage.restore_artifacts(&manifest, &parsed.output_dirs).is_ok() {
            let restore_time = t_restore_start.elapsed();
            let total_time = t_start.elapsed();
            let _ = io::stdout().write_all(manifest.stdout.as_bytes());
            let _ = io::stderr().write_all(manifest.stderr.as_bytes());
            storage.record_hit();

            let baseline = ModuleBaseline {
                module_key: module_key.to_string(),
                compiler_kind: parsed.compiler,
                last_cache_key: key.to_string(),
                classes_dir: primary_classes_dir.to_path_buf(),
                classpath_hash: classpath_hash.to_string(),
                semantic_flags: parsed.semantic_flags.clone(),
                source_hashes: source_hashes.clone(),
            };
            let _ = storage.save_baseline(&baseline);

            TelemetryLogger::log_hit(
                &config.cache_dir,
                parsed.compiler,
                target_label,
                key,
                parsed.source_files.len(),
                &manifest.artifacts,
                total_time,
                hash_time,
                restore_time,
            );
            return Ok(Some(manifest.exit_code));
        }

        Ok(None)
    }

    pub fn try_delta_compilation(
        storage: &CacheStorage,
        parsed: &ParsedArgs,
        config: &JvmCacheConfig,
        key: &str,
        module_key: &str,
        target_label: &str,
        compiler_version: &str,
        source_hashes: &HashMap<PathBuf, String>,
        classpath_hash: &str,
        primary_classes_dir: &Path,
        t_start: Instant,
    ) -> Result<Option<i32>, JvmCacheError> {
        DeltaPipeline::try_delta_compilation(
            storage,
            parsed,
            config,
            key,
            module_key,
            target_label,
            compiler_version,
            source_hashes,
            classpath_hash,
            primary_classes_dir,
            t_start,
        )
    }

    pub fn execute_full_compilation(
        storage: &CacheStorage,
        parsed: &ParsedArgs,
        config: &JvmCacheConfig,
        key: &str,
        module_key: &str,
        target_label: &str,
        compiler_version: String,
        source_hashes: HashMap<PathBuf, String>,
        classpath_hash: String,
        primary_classes_dir: PathBuf,
        t_start: Instant,
        hash_time: Duration,
    ) -> Result<i32, JvmCacheError> {
        let t_exec_start = Instant::now();
        let result = CompilerRunner::execute(parsed, config)?;
        let exec_time = t_exec_start.elapsed();
        let _ = io::stdout().write_all(result.stdout.as_bytes());
        let _ = io::stderr().write_all(result.stderr.as_bytes());

        if result.exit_code == 0 {
            let requires_classes = parsed.compiler != CompilerKind::Kapt
                && parsed.output_dirs.iter().any(|d| d.tag == TAG_CLASSES);
            let has_classes = result.artifacts.iter().any(|a| a.target_tag == TAG_CLASSES);
            let has_invalid_kapt = parsed.compiler == CompilerKind::Kapt
                && result.artifacts.iter().any(|a| a.target_tag == TAG_KAPT_STUBS && a.rel_path.extension().and_then(|e| e.to_str()) == Some("java") && result.output_dirs.iter().find(|d| d.tag == TAG_KAPT_STUBS).is_some_and(|d| DeltaPipeline::is_invalid_kapt_stub(&d.path.join(&a.rel_path))));
            if (requires_classes && !parsed.source_files.is_empty() && !has_classes) || has_invalid_kapt {
                return Ok(result.exit_code);
            }

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let manifest = Manifest {
                cache_key: key.to_string(),
                compiler_kind: parsed.compiler,
                compiler_version,
                created_at_epoch_secs: now,
                exit_code: result.exit_code,
                stdout: result.stdout,
                stderr: result.stderr,
                artifacts: result.artifacts,
            };

            let t_store_start = Instant::now();
            let _ = storage.store(&manifest, &result.output_dirs);
            let store_time = t_store_start.elapsed();
            let total_time = t_start.elapsed();

            let baseline = ModuleBaseline {
                module_key: module_key.to_string(),
                compiler_kind: parsed.compiler,
                last_cache_key: key.to_string(),
                classes_dir: primary_classes_dir,
                classpath_hash,
                semantic_flags: parsed.semantic_flags.clone(),
                source_hashes,
            };
            let _ = storage.save_baseline(&baseline);

            TelemetryLogger::log_miss(
                &config.cache_dir, parsed.compiler, target_label, key,
                parsed.source_files.len(), &manifest.artifacts, total_time, hash_time, exec_time, store_time,
            );
        } else {
            TelemetryLogger::log_failure(
                &config.cache_dir, parsed.compiler, target_label, key,
                result.exit_code, t_start.elapsed(),
            );
        }

        Ok(result.exit_code)
    }
}
