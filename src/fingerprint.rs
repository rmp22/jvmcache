use crate::config::JvmCacheConfig;
use crate::constants::FILE_FINGERPRINTS;
use crate::domain::{JvmCacheError, ParsedArgs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintEntry {
    pub size_bytes: u64,
    pub mtime_secs: u64,
    pub version_string: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FingerprintStore {
    pub entries: HashMap<String, FingerprintEntry>,
}

pub struct FingerprintManager;

impl FingerprintManager {
    pub fn get_compiler_version(args: &ParsedArgs) -> Result<String, JvmCacheError> {
        let path_str = args.real_compiler_path.to_string_lossy().to_string();
        let meta = fs::metadata(&args.real_compiler_path).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let cache_file = Self::resolve_fingerprint_file();
        let mut store = Self::load_fingerprints(&cache_file);

        if let Some(entry) = store.entries.get(&path_str)
            && entry.size_bytes == size
            && entry.mtime_secs == mtime
            && !entry.version_string.is_empty()
        {
            return Ok(entry.version_string.clone());
        }

        let output = Command::new(&args.real_compiler_path)
            .arg(args.compiler.version_flag())
            .output()?;

        let mut combined = String::new();
        combined.push_str(&String::from_utf8_lossy(&output.stdout));
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
        let trimmed = combined.trim().to_string();

        let version_str = if trimmed.is_empty() {
            args.compiler.default_command().to_string()
        } else {
            trimmed
        };

        store.entries.insert(
            path_str,
            FingerprintEntry {
                size_bytes: size,
                mtime_secs: mtime,
                version_string: version_str.clone(),
            },
        );
        Self::save_fingerprints(&cache_file, &store);

        Ok(version_str)
    }

    fn resolve_fingerprint_file() -> PathBuf {
        let config = JvmCacheConfig::load();
        config.cache_dir.join(FILE_FINGERPRINTS)
    }

    fn load_fingerprints(path: &Path) -> FingerprintStore {
        if let Ok(file) = File::open(path) {
            serde_json::from_reader(file).unwrap_or_default()
        } else {
            FingerprintStore::default()
        }
    }

    fn save_fingerprints(path: &Path, store: &FingerprintStore) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(file) = File::create(path) {
            let _ = serde_json::to_writer(file, store);
        }
    }
}
