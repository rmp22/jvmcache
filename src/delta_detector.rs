use crate::domain::{Artifact, JvmCacheError};
use crate::hasher::CacheHasher;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

pub struct DeltaDetector;

impl DeltaDetector {
    pub fn snapshot_directory(root: &Path) -> HashMap<PathBuf, u64> {
        let mut map = HashMap::new();
        if !root.exists() {
            return map;
        }

        if root.is_file() {
            if let Ok(meta) = fs::metadata(root) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                map.insert(
                    root.file_name().unwrap_or_default().into(),
                    meta.len() ^ mtime,
                );
            }
            return map;
        }

        for entry_res in WalkDir::new(root) {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file()
                && let Ok(rel) = entry.path().strip_prefix(root)
                && let Ok(meta) = entry.metadata()
            {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                map.insert(rel.to_path_buf(), meta.len() ^ mtime);
            }
        }
        map
    }

    pub fn detect_delta(
        root: &Path,
        pre_snapshot: &HashMap<PathBuf, u64>,
        tag: &str,
    ) -> Result<Vec<Artifact>, JvmCacheError> {
        let mut artifacts = Vec::new();
        if !root.exists() {
            return Ok(artifacts);
        }

        if root.is_file() {
            let file_name = PathBuf::from(root.file_name().unwrap_or_default());
            let meta = fs::metadata(root)?;
            let sha256 = CacheHasher::hash_file(root)?;
            artifacts.push(Artifact {
                target_tag: tag.to_string(),
                rel_path: file_name,
                size_bytes: meta.len(),
                sha256,
            });
            return Ok(artifacts);
        }

        let mut to_hash_paths = Vec::new();
        let mut to_hash_meta = Vec::new();

        for entry_res in WalkDir::new(root) {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file() {
                let rel = match entry.path().strip_prefix(root) {
                    Ok(r) => r.to_path_buf(),
                    Err(_) => continue,
                };

                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let sig = meta.len() ^ mtime;

                let is_new_or_modified = match pre_snapshot.get(&rel) {
                    Some(old_sig) => *old_sig != sig,
                    None => true,
                };

                if is_new_or_modified {
                    to_hash_paths.push(entry.path().to_path_buf());
                    to_hash_meta.push((rel, meta.len()));
                }
            }
        }

        if to_hash_paths.is_empty() {
            return Ok(artifacts);
        }

        let hashed = CacheHasher::hash_files_parallel(&to_hash_paths)?;
        for ((_p, sha256), (rel, size_bytes)) in hashed.into_iter().zip(to_hash_meta) {
            artifacts.push(Artifact {
                target_tag: tag.to_string(),
                rel_path: rel,
                size_bytes,
                sha256,
            });
        }
        Ok(artifacts)
    }

    pub fn matches_source_stem(class_file_name: &str, stem: &str) -> bool {
        let name_no_ext = class_file_name.strip_suffix(".class").unwrap_or(class_file_name);
        if name_no_ext == stem {
            return true;
        }
        if let Some(rest) = name_no_ext.strip_prefix(stem) {
            return rest.starts_with('$') || rest == "Kt" || rest.starts_with("Kt$");
        }
        false
    }

    pub fn prune_deleted_artifacts(
        output_dirs: &[crate::domain::OutputDirTarget],
        deleted_prefixes: &[String],
    ) {
        if deleted_prefixes.is_empty() {
            return;
        }
        for target in output_dirs {
            for entry in WalkDir::new(&target.path).into_iter().flatten() {
                if entry.file_type().is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        for prefix in deleted_prefixes {
                            if Self::matches_source_stem(name, prefix) {
                                let _ = fs::remove_file(entry.path());
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn filter_composite_baseline_artifacts(
        artifacts: &[Artifact],
        deleted_prefixes: &[String],
    ) -> HashMap<String, Artifact> {
        let mut map = HashMap::new();
        for art in artifacts {
            let rel_str = art.rel_path.to_string_lossy();
            let is_deleted = deleted_prefixes.iter().any(|prefix| {
                if let Some(name) = art.rel_path.file_name().and_then(|n| n.to_str()) {
                    Self::matches_source_stem(name, prefix)
                } else {
                    false
                }
            });
            if !is_deleted {
                let k = format!("{}:{}", art.target_tag, rel_str);
                map.insert(k, art.clone());
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_source_stem_distinguishes_prefixes() {
        let stem = "Foo";
        assert!(DeltaDetector::matches_source_stem("Foo.class", stem));
        assert!(DeltaDetector::matches_source_stem("FooKt.class", stem));
        assert!(DeltaDetector::matches_source_stem("Foo$1.class", stem));
        assert!(DeltaDetector::matches_source_stem("FooKt$sam$1.class", stem));
        assert!(!DeltaDetector::matches_source_stem("FooBar.class", stem));
        assert!(!DeltaDetector::matches_source_stem("FooBar$Factory.class", stem));
        assert!(!DeltaDetector::matches_source_stem("FooAdapter.class", stem));
    }
}
