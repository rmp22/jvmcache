use crate::compiler::CompilerRunner;
use crate::config::JvmCacheConfig;
use crate::constants::{
    COMPILER_ERROR_MARKER, EXT_JAVA, EXT_KT, EXT_KOTLIN_MODULE, KAPT_ERROR_NON_EXISTENT_CLASS,
    KAPT_ERROR_OBJECT_ANNOTATION, KAPT_ERROR_UNRESOLVED_MARKER, KAPT_PACKAGE_JVM_FUNCTIONS,
    MAX_DELTA_DELETED_FILES, MAX_DELTA_MODIFIED_FILES, TAG_ABI_HEADERS, TAG_KAPT_STUBS,
};
use crate::domain::{CompilerKind, CompilerTraits, JvmCacheError, Manifest, ModuleBaseline, ParsedArgs};
use crate::storage::CacheStorage;
use crate::telemetry::TelemetryLogger;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct DeltaPipeline;

impl DeltaPipeline {
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
        let baseline = match storage.get_baseline(module_key) {
            Some(b) => b,
            None => return Ok(Self::skip(config, parsed, target_label, "no baseline", module_key)),
        };

        let compiler_matches = baseline.compiler_kind == parsed.compiler
            || (baseline.compiler_kind.traits() & parsed.compiler.traits())
                .contains(CompilerTraits::KOTLIN);

        if !compiler_matches {
            return Ok(Self::skip(config, parsed, target_label, "compiler mismatch", &format!("{:?} vs {:?}", baseline.compiler_kind, parsed.compiler)));
        } else if config.strict_abi && baseline.classpath_hash != *classpath_hash {
            return Ok(Self::skip(config, parsed, target_label, "classpath mismatch", &format!("{} vs {}", &baseline.classpath_hash[..8.min(baseline.classpath_hash.len())], &classpath_hash[..8.min(classpath_hash.len())])));
        } else if baseline.semantic_flags != parsed.semantic_flags {
            return Ok(Self::skip(config, parsed, target_label, "flags mismatch", &format!("{}/{} flags", baseline.semantic_flags.len(), parsed.semantic_flags.len())));
        }

        let baseline_manifest = match storage.get_manifest(&baseline.last_cache_key) {
            Some(m) => m,
            None => return Ok(Self::skip(config, parsed, target_label, "manifest missing", &baseline.last_cache_key)),
        };

        let modified_sources: Vec<_> = source_hashes
            .iter()
            .filter(|(p, h)| baseline.source_hashes.get(*p) != Some(*h))
            .map(|(p, _)| (*p).clone())
            .collect();
        let deleted_sources: Vec<_> = baseline
            .source_hashes
            .keys()
            .filter(|p| !source_hashes.contains_key(*p))
            .cloned()
            .collect();

        let is_small_delta = deleted_sources.len() <= MAX_DELTA_DELETED_FILES
            && modified_sources.len() <= MAX_DELTA_MODIFIED_FILES
            && (parsed.source_files.len() <= 10
                || (modified_sources.len() + deleted_sources.len()) <= parsed.source_files.len() / 2);

        if !is_small_delta {
            return Ok(Self::skip(config, parsed, target_label, "delta too large", &format!("mod={}, del={}", modified_sources.len(), deleted_sources.len())));
        }

        let deleted_prefixes = Self::extract_stems(&deleted_sources);
        let mut excluded_prefixes = deleted_prefixes.clone();
        excluded_prefixes.extend(Self::extract_stems(&modified_sources));

        let tasks = match storage.build_restoration_tasks(&baseline_manifest, &parsed.output_dirs, &excluded_prefixes) {
            Ok(t) => t,
            Err(_) => return Ok(None),
        };

        crate::delta_detector::DeltaDetector::prune_deleted_artifacts(
            &parsed.output_dirs,
            &deleted_prefixes,
        );

        let t_restore_start = Instant::now();
        let bg_restore_handle =
            crate::async_queue::AsyncRestorationQueue::spawn_restoration(tasks);

        let mut composite_map =
            crate::delta_detector::DeltaDetector::filter_composite_baseline_artifacts(
                &baseline_manifest.artifacts,
                &deleted_prefixes,
            );

        let modified_kt_sources: Vec<_> = modified_sources
            .iter().filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXT_KT)).cloned().collect();
        let modified_java_sources: Vec<_> = modified_sources
            .iter().filter(|p| p.extension().and_then(|e| e.to_str()) == Some(EXT_JAVA)).cloned().collect();

        let traits = parsed.compiler.traits();
        let compiler_sources_modified = if traits.contains(CompilerTraits::KOTLIN) {
            !modified_kt_sources.is_empty()
        } else if traits.contains(CompilerTraits::JAVAC) {
            !modified_java_sources.is_empty()
        } else {
            !modified_sources.is_empty()
        };

        if !compiler_sources_modified {
            let _ = bg_restore_handle.join();
            let _ = io::stdout().write_all(baseline_manifest.stdout.as_bytes());
            let _ = io::stderr().write_all(baseline_manifest.stderr.as_bytes());
            storage.record_hit();
            Self::save_delta_baseline(storage, parsed, module_key, key, primary_classes_dir, classpath_hash, source_hashes);
            TelemetryLogger::log_delta_hit(
                &config.cache_dir, parsed.compiler, target_label, key, 0,
                parsed.source_files.len(), composite_map.len(),
                t_start.elapsed(), t_restore_start.elapsed(), Duration::ZERO, Duration::ZERO,
            );
            return Ok(Some(0));
        }

        let delta_sources_to_compile = if traits.contains(CompilerTraits::KOTLIN) {
            &modified_kt_sources
        } else if traits.contains(CompilerTraits::JAVAC) {
            &modified_java_sources
        } else {
            &modified_sources
        };

        if let Err(e) = bg_restore_handle.join() {
            return Err(e);
        }
        let restore_time = t_restore_start.elapsed();

        let t_exec_start = Instant::now();
        let delta_attempt = CompilerRunner::execute_delta(
            parsed,
            delta_sources_to_compile,
            primary_classes_dir,
            config,
        );

        if let Ok(delta_result) = delta_attempt {
            let _ = io::stdout().write_all(delta_result.stdout.as_bytes());
            let _ = io::stderr().write_all(delta_result.stderr.as_bytes());

            if delta_result.exit_code == 0 {
                let abi_changed = delta_result.artifacts.iter().any(|a| {
                    if a.target_tag != TAG_ABI_HEADERS || a.rel_path.to_string_lossy().contains('$') {
                        return false;
                    }
                    match baseline_manifest.artifacts.iter().find(|ba| {
                        ba.target_tag == a.target_tag && ba.rel_path == a.rel_path
                    }) {
                        Some(ba) => ba.sha256 != a.sha256,
                        None => false,
                    }
                });

                if abi_changed && delta_sources_to_compile.len() < parsed.source_files.len() {
                    return Ok(Self::skip(config, parsed, target_label, "abi changed", "fallback to full"));
                }

                if parsed.compiler == CompilerKind::Kapt
                    && delta_result.artifacts.iter().any(|a| a.target_tag == TAG_KAPT_STUBS && parsed.output_dirs.iter().find(|d| d.tag == TAG_KAPT_STUBS).is_some_and(|d| Self::is_invalid_kapt_stub(&d.path.join(&a.rel_path))))
                {
                    return Ok(Self::skip(config, parsed, target_label, "invalid kapt stub", "unresolved symbols; fallback to full"));
                }

                let exec_time = t_exec_start.elapsed();
                let is_partial = delta_sources_to_compile.len() < parsed.source_files.len();

                for art in delta_result.artifacts {
                    let is_mod = art.rel_path.extension().and_then(|e| e.to_str()) == Some(EXT_KOTLIN_MODULE);
                    let k = format!("{}:{}", art.target_tag, art.rel_path.to_string_lossy());
                    if is_mod && is_partial && composite_map.get(&k).is_some_and(|ex| ex.size_bytes > art.size_bytes) {
                        continue;
                    }
                    composite_map.insert(k, art);
                }

                    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let manifest = Manifest {
                        cache_key: key.to_string(),
                        compiler_kind: parsed.compiler,
                        compiler_version: compiler_version.to_string(),
                        created_at_epoch_secs: now,
                        exit_code: 0,
                        stdout: delta_result.stdout,
                        stderr: delta_result.stderr,
                        artifacts: composite_map.into_values().collect(),
                    };

                    let t_store_start = Instant::now();
                    let _ = storage.store(&manifest, &parsed.output_dirs);
                    Self::save_delta_baseline(storage, parsed, module_key, key, primary_classes_dir, classpath_hash, source_hashes);

                    TelemetryLogger::log_delta_hit(
                        &config.cache_dir, parsed.compiler, target_label, key,
                        delta_sources_to_compile.len(), parsed.source_files.len(), manifest.artifacts.len(),
                        t_start.elapsed(), restore_time, exec_time, t_store_start.elapsed(),
                    );

                    return Ok(Some(0));
            } else if delta_result.stdout.contains(COMPILER_ERROR_MARKER) || delta_result.stderr.contains(COMPILER_ERROR_MARKER) {
                TelemetryLogger::log_failure(&config.cache_dir, parsed.compiler, target_label, key, delta_result.exit_code, t_start.elapsed());
                return Ok(Some(delta_result.exit_code));
            }
        }

        Ok(None)
    }

    fn save_delta_baseline(storage: &CacheStorage, parsed: &ParsedArgs, module_key: &str, key: &str, dir: &Path, cp_hash: &str, srcs: &HashMap<PathBuf, String>) {
        let _ = storage.save_baseline(&ModuleBaseline {
            module_key: module_key.to_string(), compiler_kind: parsed.compiler,
            last_cache_key: key.to_string(), classes_dir: dir.to_path_buf(),
            classpath_hash: cp_hash.to_string(), semantic_flags: parsed.semantic_flags.clone(),
            source_hashes: srcs.clone(),
        });
    }

    pub fn is_invalid_kapt_stub(path: &Path) -> bool {
        if path.file_name().is_some_and(|n| n == "NonExistentClass.java") { return false; }
        let Ok(c) = std::fs::read_to_string(path) else { return false; };
        Self::contains_invalid_stub_markers(&c)
    }

    fn contains_invalid_stub_markers(c: &str) -> bool {
        c.contains(KAPT_ERROR_OBJECT_ANNOTATION)
            || c.contains(KAPT_ERROR_UNRESOLVED_MARKER)
            || c.contains(KAPT_ERROR_NON_EXISTENT_CLASS)
            || (c.contains("Function") && !c.contains(KAPT_PACKAGE_JVM_FUNCTIONS) && (0..=22).any(|n| c.contains(&format!("Function{n}<"))))
    }

    fn skip(config: &JvmCacheConfig, parsed: &ParsedArgs, target: &str, reason: &str, detail: &str) -> Option<i32> {
        TelemetryLogger::log_delta_skip(&config.cache_dir, parsed.compiler, target, reason, detail);
        None
    }

    fn extract_stems(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().filter_map(|d| d.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())).collect()
    }
}
