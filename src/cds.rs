use crate::constants::{
    CDS_ARCHIVE_D8, CDS_ARCHIVE_JAVAC, CDS_ARCHIVE_KOTLINC, CDS_ARCHIVE_R8, DIR_CDS, FLAG_CP,
    FLAG_VERSION, MAIN_CLASS_KOTLINC,
};
use crate::domain::{CompilerKind, JvmCacheError};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdsConfig {
    pub enabled: bool,
    pub tier1_c1_jit: bool,
    pub parallel_gc: bool,
    pub archive_dir: PathBuf,
}

impl CdsConfig {
    pub fn new(cache_root: &Path) -> Self {
        Self {
            enabled: true,
            tier1_c1_jit: false,
            parallel_gc: false,
            archive_dir: cache_root.join(DIR_CDS),
        }
    }
}

pub struct CdsManager;

impl CdsManager {
    pub fn archive_path(archive_dir: &Path, kind: CompilerKind) -> PathBuf {
        let filename = match kind {
            CompilerKind::Kotlinc | CompilerKind::Kapt => CDS_ARCHIVE_KOTLINC,
            CompilerKind::Javac => CDS_ARCHIVE_JAVAC,
            CompilerKind::D8 => CDS_ARCHIVE_D8,
            CompilerKind::R8 => CDS_ARCHIVE_R8,
        };
        archive_dir.join(filename)
    }

    pub fn get_jvm_tuning_args(
        kind: CompilerKind,
        config: &CdsConfig,
    ) -> Vec<String> {
        if !config.enabled {
            return Vec::new();
        }

        let mut args = Vec::new();

        if config.tier1_c1_jit {
            args.push("-XX:TieredStopAtLevel=1".to_string());
            args.push("-XX:ReservedCodeCacheSize=512m".to_string());
        }

        if config.parallel_gc {
            args.push("-XX:+UseParallelGC".to_string());
        }

        let archive = Self::archive_path(&config.archive_dir, kind);
        if archive.is_file() {
            args.push(format!("-XX:SharedArchiveFile={}", archive.display()));
        }

        args
    }

    pub fn try_create_kotlinc_archive(
        java_cmd: &Path,
        compiler_jar: &Path,
        stdlib_jar: &Path,
        out_archive: &Path,
    ) -> Result<(), JvmCacheError> {
        if let Some(parent) = out_archive.parent() {
            fs::create_dir_all(parent)?;
        }

        let cp = format!("{}:{}", compiler_jar.display(), stdlib_jar.display());
        let status = Command::new(java_cmd)
            .arg(format!("-XX:ArchiveClassesAtExit={}", out_archive.display()))
            .arg(FLAG_CP)
            .arg(&cp)
            .arg(MAIN_CLASS_KOTLINC)
            .arg(FLAG_VERSION)
            .status()?;

        if status.success() && out_archive.is_file() {
            Ok(())
        } else {
            Err(JvmCacheError::Execution(
                "Failed to generate AppCDS archive for kotlinc".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cds_manager_flags_generation() {
        let tmp = std::env::temp_dir().join(format!("jvmcache_cds_test_{}", std::process::id()));
        let mut config = CdsConfig::new(&tmp);
        config.tier1_c1_jit = true;
        config.parallel_gc = true;

        let args = CdsManager::get_jvm_tuning_args(CompilerKind::Kotlinc, &config);
        assert!(args.contains(&"-XX:TieredStopAtLevel=1".to_string()));
        assert!(args.contains(&"-XX:ReservedCodeCacheSize=512m".to_string()));
        assert!(args.contains(&"-XX:+UseParallelGC".to_string()));

        let archive = CdsManager::archive_path(&config.archive_dir, CompilerKind::Kotlinc);
        assert!(!args.iter().any(|a| a.starts_with("-XX:SharedArchiveFile=")));

        let _ = fs::create_dir_all(&config.archive_dir);
        let _ = fs::write(&archive, b"dummy-archive");

        let args_with_archive = CdsManager::get_jvm_tuning_args(CompilerKind::Kotlinc, &config);
        assert!(args_with_archive.iter().any(|a| a.starts_with("-XX:SharedArchiveFile=")));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_cds_disabled() {
        let tmp = std::env::temp_dir().join(format!("jvmcache_cds_dis_{}", std::process::id()));
        let mut config = CdsConfig::new(&tmp);
        config.enabled = false;

        let args = CdsManager::get_jvm_tuning_args(CompilerKind::Kotlinc, &config);
        assert!(args.is_empty());
    }
}
