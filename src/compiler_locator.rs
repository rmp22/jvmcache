use crate::config::JvmCacheConfig;
use crate::constants::{
    AOSP_HOST_D8, AOSP_HOST_R8, AOSP_JAVAC_JDK21, AOSP_JAVAC_JDK25, AOSP_KOTLINC_BIN,
    AOSP_MARKER_JDK, AOSP_MARKER_SOONG, AOSP_PREBUILT_D8, AOSP_PREBUILT_R8, AOSP_R8_D8,
    EXCLUDE_JVMCACHE, EXCLUDE_PATH_INTERPOSER,
};
use crate::domain::{CompilerKind, JvmCacheError};
use std::env;
use std::path::{Path, PathBuf};

pub struct CompilerLocator;

impl CompilerLocator {
    pub fn find_real_compiler(compiler: CompilerKind) -> Result<PathBuf, JvmCacheError> {
        let config = JvmCacheConfig::load();
        let configured = match compiler {
            CompilerKind::Javac => config.javac_path,
            CompilerKind::Kotlinc => config.kotlinc_path,
            CompilerKind::Kapt => config.kapt_path.or(config.kotlinc_path),
            CompilerKind::D8 => config.d8_path,
            CompilerKind::R8 => config.r8_path,
        };
        if let Some(p) = configured && p.exists() {
            return Ok(p);
        }

        if let Some(aosp_top) = Self::find_aosp_root() {
            let p = match compiler {
                CompilerKind::Javac => {
                    let j21 = aosp_top.join(AOSP_JAVAC_JDK21);
                    if j21.is_file() {
                        j21
                    } else {
                        aosp_top.join(AOSP_JAVAC_JDK25)
                    }
                }
                CompilerKind::Kotlinc | CompilerKind::Kapt => aosp_top.join(AOSP_KOTLINC_BIN),
                CompilerKind::D8 => {
                    let host_d8 = aosp_top.join(AOSP_HOST_D8);
                    if host_d8.is_file() {
                        host_d8
                    } else {
                        let prebuilt_d8 = aosp_top.join(AOSP_PREBUILT_D8);
                        if prebuilt_d8.is_file() {
                            prebuilt_d8
                        } else {
                            aosp_top.join(AOSP_R8_D8)
                        }
                    }
                }
                CompilerKind::R8 => {
                    let host_r8 = aosp_top.join(AOSP_HOST_R8);
                    if host_r8.is_file() {
                        host_r8
                    } else {
                        aosp_top.join(AOSP_PREBUILT_R8)
                    }
                }
            };
            if p.is_file() {
                return Ok(p);
            }
        }

        let default_cmd = match compiler {
            CompilerKind::Kapt => "kotlinc",
            _ => compiler.default_command(),
        };
        let current_exe = env::current_exe().ok();

        if let Ok(paths) = env::var("PATH") {
            for dir in env::split_paths(&paths) {
                let candidate = dir.join(default_cmd);
                if candidate.is_file() && let Ok(canonical) = candidate.canonicalize() {
                    let canon_str = canonical.to_string_lossy();
                    if canon_str.contains(EXCLUDE_PATH_INTERPOSER) {
                        continue;
                    }
                    if canon_str.ends_with(&format!("/{}", EXCLUDE_JVMCACHE))
                        || canonical.file_name().and_then(|n| n.to_str()) == Some(EXCLUDE_JVMCACHE)
                    {
                        continue;
                    }
                    if let Some(ref self_exe) = current_exe
                        && let Ok(self_canonical) = self_exe.canonicalize()
                        && canonical == self_canonical
                    {
                        continue;
                    }
                    return Ok(canonical);
                }
            }
        }

        Err(JvmCacheError::InvalidInvocation(format!(
            "Underlying compiler binary '{}' not found in PATH",
            default_cmd
        )))
    }

    pub fn find_aosp_root() -> Option<PathBuf> {
        if let Ok(top) = env::var("ANDROID_BUILD_TOP") {
            let p = PathBuf::from(top);
            if p.join(AOSP_MARKER_SOONG).is_dir() {
                return Some(p);
            }
        }
        if let Ok(top) = env::var("TOP") {
            let p = PathBuf::from(top);
            if p.join(AOSP_MARKER_SOONG).is_dir() {
                return Some(p);
            }
        }
        if let Ok(exe) = env::current_exe() {
            let mut p = exe;
            while let Some(parent) = p.parent() {
                if parent.join(AOSP_MARKER_SOONG).is_dir()
                    && parent.join(AOSP_MARKER_JDK).is_dir()
                {
                    return Some(parent.to_path_buf());
                }
                p = parent.to_path_buf();
            }
        }
        if let Ok(cwd) = env::current_dir() {
            let mut curr: Option<&Path> = Some(&cwd);
            while let Some(dir) = curr {
                if dir.join(AOSP_MARKER_SOONG).is_dir()
                    && dir.join(AOSP_MARKER_JDK).is_dir()
                {
                    return Some(dir.to_path_buf());
                }
                curr = dir.parent();
            }
        }
        None
    }
}

pub fn find_aosp_root() -> Option<PathBuf> {
    CompilerLocator::find_aosp_root()
}
