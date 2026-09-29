use crate::constants::{
    DEFAULT_MAX_CACHE_SIZE_MB, DIR_OBJECTS, FILE_FINGERPRINTS, FILE_STATS, MAX_TARGETED_CALLERS,
};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFileModel {
    pub cache_dir: Option<String>,
    pub max_cache_size_mb: Option<u64>,
    pub javac_path: Option<String>,
    pub kotlinc_path: Option<String>,
    pub kapt_path: Option<String>,
    pub d8_path: Option<String>,
    pub r8_path: Option<String>,
    pub hardlink_enabled: Option<bool>,
    pub daemon_enabled: Option<bool>,
    pub auto_spawn_daemon: Option<bool>,
    pub cds_enabled: Option<bool>,
    pub flags_enabled: Option<bool>,
    pub kotlinc_threads: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct JvmCacheConfig {
    pub cache_dir: PathBuf,
    pub max_cache_size_mb: u64,
    pub javac_path: Option<PathBuf>,
    pub kotlinc_path: Option<PathBuf>,
    pub kapt_path: Option<PathBuf>,
    pub d8_path: Option<PathBuf>,
    pub r8_path: Option<PathBuf>,
    pub hardlink_enabled: bool,
    pub daemon_enabled: bool,
    pub auto_spawn_daemon: bool,
    pub cds_enabled: bool,
    pub flags_enabled: bool,
    pub kotlinc_threads: usize,
    pub strict_abi: bool,
    pub max_targeted_callers: usize,
    pub config_file_path: Option<PathBuf>,
}

impl Default for JvmCacheConfig {
    fn default() -> Self {
        Self::load()
    }
}

impl JvmCacheConfig {
    pub fn load() -> Self {
        let (file_path, file_model) = Self::discover_and_read_config();

        let cache_dir = env::var("JVMCACHE_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.cache_dir.map(PathBuf::from))
            .unwrap_or_else(Self::default_cache_dir);

        let max_cache_size_mb = env::var("JVMCACHE_MAXSIZE")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .or(file_model.max_cache_size_mb)
            .unwrap_or(DEFAULT_MAX_CACHE_SIZE_MB);

        let javac_path = env::var("JVMCACHE_JAVAC")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.javac_path.map(PathBuf::from));

        let kotlinc_path = env::var("JVMCACHE_KOTLINC")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.kotlinc_path.map(PathBuf::from));

        let kapt_path = env::var("JVMCACHE_KAPT")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.kapt_path.map(PathBuf::from));

        let d8_path = env::var("JVMCACHE_D8")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.d8_path.map(PathBuf::from));

        let r8_path = env::var("JVMCACHE_R8")
            .ok()
            .map(PathBuf::from)
            .or_else(|| file_model.r8_path.map(PathBuf::from));

        let hardlink_enabled = Self::env_bool("JVMCACHE_HARDLINK", file_model.hardlink_enabled, true);
        let daemon_enabled = Self::env_bool("JVMCACHE_DAEMON", file_model.daemon_enabled, true);
        let auto_spawn_daemon = Self::env_bool("JVMCACHE_AUTO_SPAWN", file_model.auto_spawn_daemon, true);
        let cds_enabled = Self::env_bool("JVMCACHE_CDS", file_model.cds_enabled, true);
        let flags_enabled = Self::env_bool("JVMCACHE_AUTO_FLAGS", file_model.flags_enabled, true);
        let strict_abi = Self::env_bool("JVMCACHE_STRICT_ABI", None, false);
        let max_targeted_callers = env::var("JVMCACHE_MAX_TARGETED_CALLERS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(MAX_TARGETED_CALLERS);

        let kotlinc_threads = env::var("JVMCACHE_KOTLINC_THREADS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .or(file_model.kotlinc_threads)
            .unwrap_or(0);

        Self {
            cache_dir,
            max_cache_size_mb,
            javac_path,
            kotlinc_path,
            kapt_path,
            d8_path,
            r8_path,
            hardlink_enabled,
            daemon_enabled,
            auto_spawn_daemon,
            cds_enabled,
            flags_enabled,
            kotlinc_threads,
            strict_abi,
            max_targeted_callers,
            config_file_path: file_path,
        }
    }

    fn env_bool(var: &str, fallback: Option<bool>, default: bool) -> bool {
        env::var(var).ok().map(|s| s == "1" || s.eq_ignore_ascii_case("true")).or(fallback).unwrap_or(default)
    }

    fn default_cache_dir() -> PathBuf {
        if let Ok(cwd) = env::current_dir() {
            let mut curr: Option<&Path> = Some(cwd.as_path());
            while let Some(dir) = curr {
                let candidate = dir.join(".jvmcache");
                if candidate.is_dir() {
                    return candidate;
                }
                curr = dir.parent();
            }
        }
        if let Ok(dir) = env::var("XDG_CACHE_HOME") {
            return PathBuf::from(dir).join("jvmcache");
        }
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(".cache").join("jvmcache");
        }
        env::temp_dir().join("jvmcache")
    }

    fn discover_and_read_config() -> (Option<PathBuf>, ConfigFileModel) {
        if let Ok(custom) = env::var("JVMCACHE_CONFIG") {
            if let Some(res) = try_load_config_file(PathBuf::from(custom)) {
                return (Some(res.0), res.1);
            }
        }

        if let Ok(cwd) = env::current_dir() {
            let mut curr: Option<&Path> = Some(cwd.as_path());
            while let Some(dir) = curr {
                if let Some(res) = try_load_config_file(dir.join(".jvmcache.json")) {
                    return (Some(res.0), res.1);
                }
                if let Some(res) = try_load_config_file(dir.join(".jvmcache").join("config.json")) {
                    return (Some(res.0), res.1);
                }
                curr = dir.parent();
            }
        }

        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            if let Some(res) = try_load_config_file(PathBuf::from(config_home).join("jvmcache/config.json")) {
                return (Some(res.0), res.1);
            }
        } else if let Ok(home) = env::var("HOME") {
            if let Some(res) = try_load_config_file(PathBuf::from(home).join(".config/jvmcache/config.json")) {
                return (Some(res.0), res.1);
            }
        }

        (None, ConfigFileModel::default())
    }

    pub fn print_summary(&self) {
        println!("jvmcache configuration:");
        println!(
            "  Config file:           {}",
            self.config_file_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "<none> (using defaults + env vars)".to_string())
        );
        println!("  Cache directory:       {}", self.cache_dir.display());
        println!(
            "  Max cache size:        {} MB ({:.2} GB)",
            self.max_cache_size_mb,
            self.max_cache_size_mb as f64 / 1024.0
        );
        println!("  Hardlink enabled:      {}", self.hardlink_enabled);
        println!("  Resident daemon:       {} (auto-spawn: {})", self.daemon_enabled, self.auto_spawn_daemon);
        println!("  AppCDS JVM tuning:     {}", self.cds_enabled);
        println!("  Auto compiler flags:   {} (kotlinc threads: {})", self.flags_enabled, self.kotlinc_threads);
        println!(
            "  Custom javac binary:   {}",
            self.javac_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "<none> (resolves from PATH)".to_string())
        );
        println!(
            "  Custom kotlinc binary: {}",
            self.kotlinc_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| "<none> (resolves from PATH)".to_string())
        );
        println!("  Objects storage:       {}/{}", self.cache_dir.display(), DIR_OBJECTS);
        println!("  Statistics file:       {}/{}", self.cache_dir.display(), FILE_STATS);
        println!(
            "  Fingerprints file:     {}/{}",
            self.cache_dir.display(),
            FILE_FINGERPRINTS
        );
    }
}

fn try_load_config_file(p: PathBuf) -> Option<(PathBuf, ConfigFileModel)> {
    if p.is_file() && let Ok(file) = File::open(&p) {
        let m: ConfigFileModel = serde_json::from_reader(file).unwrap_or_default();
        return Some((p, m));
    }
    None
}
