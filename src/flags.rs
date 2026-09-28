use crate::constants::{
    COMPILER_CMD_KAPT, FLAG_BACKEND_THREADS_PREFIX, FLAG_COMPILE_POLICY_PREFIX,
    FLAG_COMPILE_POLICY_SIMPLE, FLAG_EXPECT_ACTUAL_CLASSES, FLAG_FAST_JAR_FS, FLAG_MULTI_PLATFORM,
    FLAG_PROCESSOR, FLAG_PROCESSOR_MODULE_PATH, FLAG_PROCESSOR_PATH, FLAG_PROCESSOR_PATH_LONG,
    FLAG_PROC_NONE, FLAG_PROC_PREFIX, PLUGIN_MARKER_ANNO_PROC, PLUGIN_MARKER_JVM_ABI,
    PLUGIN_MARKER_JVM_ABI_GEN,
};
use crate::domain::CompilerKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagConfig {
    pub enabled: bool,
    pub kotlinc_backend_threads: Option<usize>,
    pub kotlinc_fast_jar_fs: bool,
    pub javac_compile_policy_simple: bool,
    pub javac_proc_none: bool,
}

impl Default for FlagConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            kotlinc_backend_threads: None,
            kotlinc_fast_jar_fs: false,
            javac_compile_policy_simple: true,
            javac_proc_none: false,
        }
    }
}

pub struct FlagOptimizer;

impl FlagOptimizer {
    pub fn optimize_args(
        kind: CompilerKind,
        raw_args: &[String],
        config: &FlagConfig,
    ) -> Vec<String> {
        if !config.enabled {
            return raw_args.to_vec();
        }

        match kind {
            CompilerKind::Kotlinc => Self::optimize_kotlinc(raw_args, config),
            CompilerKind::Kapt => raw_args.to_vec(),
            CompilerKind::Javac => Self::optimize_javac(raw_args, config),
            CompilerKind::D8 | CompilerKind::R8 => raw_args.to_vec(),
        }
    }

    fn optimize_kotlinc(raw_args: &[String], config: &FlagConfig) -> Vec<String> {
        let has_backend_threads = raw_args
            .iter()
            .any(|a| a.starts_with(FLAG_BACKEND_THREADS_PREFIX));
        let has_incompatible = raw_args.iter().any(|a| {
            a.contains(PLUGIN_MARKER_JVM_ABI_GEN)
                || a.contains(PLUGIN_MARKER_JVM_ABI)
                || a.contains(COMPILER_CMD_KAPT)
                || a.contains(PLUGIN_MARKER_ANNO_PROC)
                || a == FLAG_MULTI_PLATFORM
                || a == FLAG_EXPECT_ACTUAL_CLASSES
        });
        let needs_backend_threads = !has_backend_threads
            && !has_incompatible
            && config.kotlinc_backend_threads.is_some();

        let has_fast_jar_fs = raw_args.iter().any(|a| a == FLAG_FAST_JAR_FS);
        let needs_fast_jar_fs = !has_fast_jar_fs && config.kotlinc_fast_jar_fs;

        if !needs_backend_threads && !needs_fast_jar_fs {
            return raw_args.to_vec();
        }

        let mut result = Vec::with_capacity(raw_args.len() + 2);
        result.extend_from_slice(raw_args);

        if needs_backend_threads
            && let Some(threads) = config.kotlinc_backend_threads
        {
            result.push(format!("{FLAG_BACKEND_THREADS_PREFIX}{threads}"));
        }

        if needs_fast_jar_fs {
            result.push(FLAG_FAST_JAR_FS.to_string());
        }

        result
    }

    fn optimize_javac(raw_args: &[String], config: &FlagConfig) -> Vec<String> {
        let has_compile_policy = raw_args
            .iter()
            .any(|a| a.starts_with(FLAG_COMPILE_POLICY_PREFIX));
        let needs_policy = !has_compile_policy && config.javac_compile_policy_simple;

        let has_proc = raw_args.iter().any(|a| {
            a.starts_with(FLAG_PROC_PREFIX)
                || a == FLAG_PROCESSOR
                || a == FLAG_PROCESSOR_PATH
                || a == FLAG_PROCESSOR_PATH_LONG
                || a == FLAG_PROCESSOR_MODULE_PATH
        });
        let needs_proc = !has_proc && config.javac_proc_none;

        if !needs_policy && !needs_proc {
            return raw_args.to_vec();
        }

        let mut result = Vec::with_capacity(raw_args.len() + 2);
        result.extend_from_slice(raw_args);

        if needs_policy {
            result.push(FLAG_COMPILE_POLICY_SIMPLE.to_string());
        }
        if needs_proc {
            result.push(FLAG_PROC_NONE.to_string());
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kotlinc_injects_backend_threads() {
        let config = FlagConfig {
            kotlinc_backend_threads: Some(4),
            ..Default::default()
        };
        let raw = vec!["Hello.kt".to_string(), "-d".to_string(), "out".to_string()];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Kotlinc, &raw, &config);
        assert!(opt.contains(&"-Xbackend-threads=4".to_string()));
        assert_eq!(&opt[0..3], &raw[..]);
    }

    #[test]
    fn test_kotlinc_skips_backend_threads_for_incompatible_plugins() {
        let config = FlagConfig {
            kotlinc_backend_threads: Some(4),
            ..Default::default()
        };
        let raw = vec![
            "Hello.kt".to_string(),
            "-Xplugin=external/kotlinc/lib/jvm-abi-gen.jar".to_string(),
        ];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Kotlinc, &raw, &config);
        assert!(!opt.iter().any(|a| a.starts_with("-Xbackend-threads=")));
    }

    #[test]
    fn test_kotlinc_preserves_existing_backend_threads() {
        let config = FlagConfig::default();
        let raw = vec![
            "Hello.kt".to_string(),
            "-Xbackend-threads=8".to_string(),
            "-d".to_string(),
            "out".to_string(),
        ];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Kotlinc, &raw, &config);
        assert_eq!(
            opt.iter()
                .filter(|a| a.starts_with("-Xbackend-threads="))
                .count(),
            1
        );
        assert!(opt.contains(&"-Xbackend-threads=8".to_string()));
    }

    #[test]
    fn test_javac_injects_compile_policy() {
        let config = FlagConfig::default();
        let raw = vec!["Hello.java".to_string(), "-d".to_string(), "out".to_string()];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Javac, &raw, &config);
        assert!(opt.contains(&"-XDcompilePolicy=simple".to_string()));
    }

    #[test]
    fn test_javac_proc_none_injection() {
        let config = FlagConfig {
            javac_proc_none: true,
            ..Default::default()
        };
        let raw = vec!["Hello.java".to_string()];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Javac, &raw, &config);
        assert!(opt.contains(&"-proc:none".to_string()));

        let with_proc = vec!["Hello.java".to_string(), "-processor".to_string(), "MyProc".to_string()];
        let opt_with = FlagOptimizer::optimize_args(CompilerKind::Javac, &with_proc, &config);
        assert!(!opt_with.contains(&"-proc:none".to_string()));
    }

    #[test]
    fn test_optimizer_disabled() {
        let config = FlagConfig {
            enabled: false,
            ..Default::default()
        };
        let raw = vec!["Hello.kt".to_string()];
        let opt = FlagOptimizer::optimize_args(CompilerKind::Kotlinc, &raw, &config);
        assert_eq!(opt, raw);
    }
}
