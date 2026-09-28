use crate::constants::{
    COMPILER_CMD_D8, COMPILER_CMD_JAVAC, COMPILER_CMD_KAPT, COMPILER_CMD_KOTLINC,
    COMPILER_CMD_KOTLINC_JVM, COMPILER_CMD_R8, EXT_APK, EXT_CLASS, EXT_DEX, EXT_JAR, EXT_JAVA,
    EXT_KT, EXT_KTS, EXT_ZIP, FLAG_VERSION, FLAG_VERSION_GNU, TAG_CLASSES, TAG_DEX,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerTraits(pub u8);

impl CompilerTraits {
    pub const JAVAC: Self = Self(1 << 0);
    pub const KOTLIN: Self = Self(1 << 1);
    pub const KAPT: Self = Self(1 << 2);
    pub const DEX: Self = Self(1 << 3);
    pub const DAEMON: Self = Self(1 << 4);

    #[inline(always)]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline(always)]
    pub const fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

impl std::ops::BitOr for CompilerTraits {
    type Output = Self;
    #[inline(always)]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for CompilerTraits {
    type Output = Self;
    #[inline(always)]
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompilerKind {
    Javac,
    Kotlinc,
    Kapt,
    D8,
    R8,
}

impl CompilerKind {
    pub const fn traits(self) -> CompilerTraits {
        match self {
            CompilerKind::Javac => {
                CompilerTraits(CompilerTraits::JAVAC.0 | CompilerTraits::DAEMON.0)
            }
            CompilerKind::Kotlinc => {
                CompilerTraits(CompilerTraits::KOTLIN.0 | CompilerTraits::DAEMON.0)
            }
            CompilerKind::Kapt => {
                CompilerTraits(CompilerTraits::KOTLIN.0 | CompilerTraits::KAPT.0)
            }
            CompilerKind::D8 | CompilerKind::R8 => CompilerTraits::DEX,
        }
    }

    pub fn default_command(self) -> &'static str {
        match self {
            CompilerKind::Javac => COMPILER_CMD_JAVAC,
            CompilerKind::Kotlinc => COMPILER_CMD_KOTLINC,
            CompilerKind::Kapt => COMPILER_CMD_KAPT,
            CompilerKind::D8 => COMPILER_CMD_D8,
            CompilerKind::R8 => COMPILER_CMD_R8,
        }
    }

    pub fn underlying_binary_name(self) -> &'static str {
        if self.traits().contains(CompilerTraits::KAPT) {
            COMPILER_CMD_KOTLINC
        } else {
            self.default_command()
        }
    }

    pub fn version_flag(self) -> &'static str {
        if self.traits().contains(CompilerTraits::DEX) {
            FLAG_VERSION_GNU
        } else {
            FLAG_VERSION
        }
    }

    pub fn default_output_tag(self) -> &'static str {
        if self.traits().contains(CompilerTraits::DEX) {
            TAG_DEX
        } else {
            TAG_CLASSES
        }
    }

    pub fn daemon_compiler_name(self) -> &'static str {
        if self.traits().contains(CompilerTraits::KOTLIN) {
            COMPILER_CMD_KOTLINC
        } else {
            self.default_command()
        }
    }

    pub fn matches_binary_name(self, name: &str) -> bool {
        let clean = name.trim().to_lowercase();
        let cmd = self.default_command();
        if clean == cmd || clean.ends_with(&format!("/{}", cmd)) {
            return true;
        }
        self == CompilerKind::Kotlinc && clean == COMPILER_CMD_KOTLINC_JVM
    }

    pub fn from_binary_name(name: &str) -> Option<Self> {
        [
            CompilerKind::Javac,
            CompilerKind::Kotlinc,
            CompilerKind::Kapt,
            CompilerKind::D8,
            CompilerKind::R8,
        ]
        .into_iter()
        .find(|k| k.matches_binary_name(name))
    }

    pub fn is_valid_source_extension(self, ext: &str) -> bool {
        let lower = ext.to_lowercase();
        if self.traits().contains(CompilerTraits::DEX) {
            matches!(
                lower.as_str(),
                EXT_CLASS | EXT_JAR | EXT_ZIP | EXT_DEX | EXT_APK
            )
        } else {
            matches!(lower.as_str(), EXT_JAVA | EXT_KT | EXT_KTS)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDirTarget {
    pub tag: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ParsedArgs {
    pub compiler: CompilerKind,
    pub real_compiler_path: PathBuf,
    pub output_dirs: Vec<OutputDirTarget>,
    pub classpath: Vec<PathBuf>,
    pub source_files: Vec<PathBuf>,
    pub semantic_flags: Vec<String>,
    pub non_semantic_flags: Vec<String>,
    pub is_compilation: bool,
    pub raw_args: Vec<String>,
    pub build_file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleBaseline {
    pub module_key: String,
    pub compiler_kind: CompilerKind,
    pub last_cache_key: String,
    pub classes_dir: PathBuf,
    pub classpath_hash: String,
    pub semantic_flags: Vec<String>,
    pub source_hashes: HashMap<PathBuf, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub target_tag: String,
    pub rel_path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub cache_key: String,
    pub compiler_kind: CompilerKind,
    pub compiler_version: String,
    pub created_at_epoch_secs: u64,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub direct_passthrough: u64,
    pub bytes_cached: u64,
}

#[derive(Debug)]
pub enum JvmCacheError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Execution(String),
    InvalidInvocation(String),
}

impl std::fmt::Display for JvmCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::Json(e) => write!(f, "JSON serialization error: {}", e),
            Self::Execution(msg) => write!(f, "Compiler execution error: {}", msg),
            Self::InvalidInvocation(msg) => write!(f, "Invalid compiler invocation: {}", msg),
        }
    }
}

impl std::error::Error for JvmCacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::Execution(_) | Self::InvalidInvocation(_) => None,
        }
    }
}

impl From<std::io::Error> for JvmCacheError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for JvmCacheError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
