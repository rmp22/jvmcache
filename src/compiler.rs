use crate::cds::{CdsConfig, CdsManager};
use crate::config::JvmCacheConfig;
use crate::constants::{COMPILER_CMD_KAPT, TAG_ANNO};
use crate::daemon::{DaemonConfig, DaemonLauncher, DaemonRequest};
use crate::delta_compiler::DeltaCompiler;
use crate::delta_detector::DeltaDetector;
use crate::domain::{
    Artifact, CompilerKind, CompilerTraits, JvmCacheError, OutputDirTarget, ParsedArgs,
};
use crate::flags::{FlagConfig, FlagOptimizer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub artifacts: Vec<Artifact>,
    pub output_dirs: Vec<OutputDirTarget>,
}

pub struct CompilerRunner;

impl CompilerRunner {
    pub fn execute(
        args: &ParsedArgs,
        config: &JvmCacheConfig,
    ) -> Result<ExecutionResult, JvmCacheError> {
        let mut pre_snapshots: Vec<(&OutputDirTarget, HashMap<PathBuf, u64>)> = Vec::new();
        for target in &args.output_dirs {
            pre_snapshots.push((target, DeltaDetector::snapshot_directory(&target.path)));
        }

        let (exit_code, stdout, stderr) = run_compiler_dispatch(args, &args.raw_args, config)?;

        let mut artifacts = Vec::new();
        if exit_code == 0 {
            for (target, pre) in &pre_snapshots {
                artifacts.extend(DeltaDetector::detect_delta(&target.path, pre, &target.tag)?);
            }
        }

        Ok(ExecutionResult {
            exit_code,
            stdout,
            stderr,
            artifacts,
            output_dirs: args.output_dirs.clone(),
        })
    }

    pub fn execute_delta(
        args: &ParsedArgs,
        modified_sources: &[PathBuf],
        classes_dir: &Path,
        config: &JvmCacheConfig,
    ) -> Result<ExecutionResult, JvmCacheError> {
        DeltaCompiler::execute_delta(args, modified_sources, classes_dir, config)
    }
}

pub fn run_compiler_dispatch(
    args: &ParsedArgs,
    raw_args: &[String],
    config: &JvmCacheConfig,
) -> Result<(i32, String, String), JvmCacheError> {
    let flag_config = FlagConfig {
        enabled: config.flags_enabled,
        kotlinc_backend_threads: if config.kotlinc_threads > 0 {
            Some(config.kotlinc_threads)
        } else {
            None
        },
        kotlinc_fast_jar_fs: false,
        javac_compile_policy_simple: config.flags_enabled,
        javac_proc_none: config.flags_enabled,
    };
    let optimized_args = FlagOptimizer::optimize_args(args.compiler, raw_args, &flag_config);

    let is_kapt = args.compiler.traits().contains(CompilerTraits::KAPT)
        || args
            .output_dirs
            .iter()
            .any(|d| d.tag.contains(COMPILER_CMD_KAPT) || d.tag.contains(TAG_ANNO))
        || raw_args.iter().any(|a| a.contains(COMPILER_CMD_KAPT));

    let is_dex = args.compiler.traits().contains(CompilerTraits::DEX);

    if config.daemon_enabled && !is_kapt && !is_dex {
        let daemon_cfg = DaemonConfig {
            enabled: true,
            auto_spawn: config.auto_spawn_daemon,
            socket_path: DaemonConfig::new(&config.cache_dir).socket_path,
            daemon_jar_path: DaemonConfig::new(&config.cache_dir).daemon_jar_path,
        };
        if let Ok(client) = DaemonLauncher::ensure_daemon_running(&daemon_cfg) {
            let daemon_args: Vec<String> = optimized_args
                .iter()
                .filter(|a| !a.starts_with("-J"))
                .cloned()
                .collect();

            let req = DaemonRequest {
                compiler: match args.compiler {
                    CompilerKind::Kotlinc | CompilerKind::Kapt => "kotlinc".to_string(),
                    CompilerKind::Javac => "javac".to_string(),
                    CompilerKind::D8 => "d8".to_string(),
                    CompilerKind::R8 => "r8".to_string(),
                },
                working_dir: std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                args: daemon_args,
            };
            if let Ok(resp) = client.send_request(&req) {
                return Ok((resp.exit_code, resp.stdout, resp.stderr));
            }
        }
    }

    let mut cmd = Command::new(&args.real_compiler_path);
    if config.cds_enabled {
        let cds_cfg = CdsConfig::new(&config.cache_dir);
        let cds_args = CdsManager::get_jvm_tuning_args(args.compiler, &cds_cfg);
        cmd.args(&cds_args);
    }
    cmd.args(&optimized_args);

    let output = cmd.output()?;
    let exit_code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((exit_code, stdout, stderr))
}
