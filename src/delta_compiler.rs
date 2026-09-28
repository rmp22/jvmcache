use crate::cas::unique_temp_id;
use crate::compiler::{run_compiler_dispatch, ExecutionResult};
use crate::config::JvmCacheConfig;
use crate::constants::{
    DIR_KOTLIN, DIR_LIBS, DIR_META_INF, DIR_OUT, DOT_EXT_JAVA, DOT_EXT_KT, EXT_JAR,
    EXT_KOTLIN_MODULE, FLAG_BUILD_FILE, FLAG_BUILD_FILE_PREFIX, FLAG_CLASSPATH, FLAG_CLASS_PATH,
    FLAG_CP, FLAG_FRIEND_PATHS_PREFIX, FLAG_P, FLAG_XBUILD_FILE, FLAG_XBUILD_FILE_PREFIX,
    PLUGIN_PREFIX_COMPILED_SOURCES, SUBDIR_STUBS,
};
use crate::delta_detector::DeltaDetector;
use crate::domain::{CompilerKind, CompilerTraits, JvmCacheError, OutputDirTarget, ParsedArgs};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct DeltaCompiler;

impl DeltaCompiler {
    pub fn execute_delta(
        args: &ParsedArgs,
        modified_sources: &[PathBuf],
        classes_dir: &Path,
        config: &JvmCacheConfig,
    ) -> Result<ExecutionResult, JvmCacheError> {
        let mut delta_args = Vec::new();
        let mut cleanup_file: Option<PathBuf> = None;

        let mut prev_module_bytes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        for target in &args.output_dirs {
            if let Some((p, bytes)) = Self::find_and_read_kotlin_module(&target.path) {
                prev_module_bytes.push((p, bytes));
            }
        }

        if let Some(build_file) = &args.build_file_path {
            let baseline_jar = Self::find_or_create_baseline_jar(classes_dir);
            let tmp_xml = crate::kotlin_build_file::create_delta_kotlinc_build_file(
                build_file,
                modified_sources,
                classes_dir,
                baseline_jar.as_deref(),
            )?;
            cleanup_file = Some(tmp_xml.clone());
            let tmp_xml_str = tmp_xml.to_string_lossy().to_string();

            let mut i = 0;
            while i < args.raw_args.len() {
                let arg = &args.raw_args[i];
                if arg.starts_with(FLAG_XBUILD_FILE_PREFIX) {
                    delta_args.push(format!("{}{}", FLAG_XBUILD_FILE_PREFIX, tmp_xml_str));
                    i += 1;
                } else if (arg == FLAG_XBUILD_FILE || arg == FLAG_BUILD_FILE || arg == "--build-file")
                    && i + 1 < args.raw_args.len()
                {
                    delta_args.push(arg.clone());
                    delta_args.push(tmp_xml_str.clone());
                    i += 2;
                } else if arg.starts_with(FLAG_BUILD_FILE_PREFIX) || arg.starts_with("--build-file=") {
                    delta_args.push(format!("{}{}", FLAG_BUILD_FILE_PREFIX, tmp_xml_str));
                    i += 1;
                } else {
                    delta_args.push(arg.clone());
                    i += 1;
                }
            }

            if args.compiler.traits().contains(CompilerTraits::KAPT)
                && let Some(ref jar) = baseline_jar
            {
                let jar_abs = if jar.is_absolute() { jar.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(jar) };
                if jar_abs.is_file() {
                    delta_args.push(FLAG_P.to_string());
                    delta_args.push(format!("{}{}", PLUGIN_PREFIX_COMPILED_SOURCES, jar_abs.to_string_lossy()));
                }
            }
        } else if args.compiler.traits().contains(CompilerTraits::KOTLIN) {
            let classes_abs = if classes_dir.is_absolute() {
                classes_dir.to_path_buf()
            } else {
                std::env::current_dir().unwrap_or_default().join(classes_dir)
            };
            let classes_str = classes_abs.to_string_lossy();
            let is_stubs = classes_str.contains(SUBDIR_STUBS);
            if !is_stubs && !classes_str.is_empty() {
                delta_args.push(format!("{}{}", FLAG_FRIEND_PATHS_PREFIX, classes_str));
            }

            let mut i = 0;
            let mut cp_inserted = false;
            let modified_set: HashSet<PathBuf> = modified_sources.iter().cloned().collect();

            while i < args.raw_args.len() {
                let arg = &args.raw_args[i];
                if (arg == FLAG_CP || arg == FLAG_CLASSPATH) && i + 1 < args.raw_args.len() {
                    delta_args.push(arg.clone());
                    if !is_stubs && !classes_str.is_empty() {
                        delta_args.push(format!("{}:{}", classes_str, &args.raw_args[i + 1]));
                    } else {
                        delta_args.push(args.raw_args[i + 1].clone());
                    }
                    cp_inserted = true;
                    i += 2;
                } else if arg.ends_with(DOT_EXT_KT) || arg.ends_with(DOT_EXT_JAVA) {
                    let p = PathBuf::from(arg);
                    if modified_set.contains(&p) {
                        delta_args.push(arg.clone());
                    }
                    i += 1;
                } else {
                    delta_args.push(arg.clone());
                    i += 1;
                }
            }

            if !cp_inserted && !is_stubs && !classes_str.is_empty() {
                delta_args.push(FLAG_CP.to_string());
                delta_args.push(classes_str.to_string());
            }
        } else if args.compiler == CompilerKind::Javac {
            let classes_str = classes_dir.to_string_lossy();
            let has_argfile = args.raw_args.iter().any(|a| a.starts_with('@'));
            let rsp_arg = if has_argfile {
                let temp_id = unique_temp_id("javac");
                let rsp_path = std::env::temp_dir().join(format!("jvmcache_delta_javac_{}.rsp", temp_id));
                let mut rsp_content = String::new();
                for src in modified_sources {
                    rsp_content.push_str(&src.to_string_lossy());
                    rsp_content.push('\n');
                }
                fs::write(&rsp_path, rsp_content)?;
                cleanup_file = Some(rsp_path.clone());
                Some(format!("@{}", rsp_path.to_string_lossy()))
            } else {
                None
            };

            let modified_set: HashSet<PathBuf> = modified_sources.iter().cloned().collect();
            let mut i = 0;
            let mut cp_inserted = false;

            while i < args.raw_args.len() {
                let arg = &args.raw_args[i];
                if let Some(ref rsp) = rsp_arg
                    && arg.starts_with('@')
                {
                    delta_args.push(rsp.clone());
                    i += 1;
                } else if (arg == FLAG_CP || arg == FLAG_CLASSPATH || arg == FLAG_CLASS_PATH)
                    && i + 1 < args.raw_args.len()
                {
                    delta_args.push(arg.clone());
                    delta_args.push(format!("{}:{}", classes_str, &args.raw_args[i + 1]));
                    cp_inserted = true;
                    i += 2;
                } else if arg.ends_with(DOT_EXT_JAVA) {
                    let p = PathBuf::from(arg);
                    if modified_set.contains(&p) {
                        delta_args.push(arg.clone());
                    }
                    i += 1;
                } else {
                    delta_args.push(arg.clone());
                    i += 1;
                }
            }

            if !cp_inserted && !classes_str.is_empty() {
                delta_args.push(FLAG_CP.to_string());
                delta_args.push(classes_str.to_string());
            }
        } else {
            delta_args = args.raw_args.clone();
        }

        let mut pre_snapshots: Vec<(&OutputDirTarget, HashMap<PathBuf, u64>)> = Vec::new();
        for target in &args.output_dirs {
            pre_snapshots.push((target, DeltaDetector::snapshot_directory(&target.path)));
        }

        let (exit_code, stdout, stderr) = run_compiler_dispatch(args, &delta_args, config)?;

        if let Some(clean) = cleanup_file {
            let _ = fs::remove_file(clean);
        }

        for (mod_path, orig_bytes) in prev_module_bytes {
            if let Ok(meta) = fs::metadata(&mod_path)
                && meta.len() < orig_bytes.len() as u64
                && orig_bytes.len() > 32
            {
                let _ = fs::write(&mod_path, orig_bytes);
            }
        }

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

    pub fn find_or_create_baseline_jar(classes_dir: &Path) -> Option<PathBuf> {
        let mut curr = classes_dir.parent();
        while let Some(dir) = curr {
            for sub in [DIR_KOTLIN, DIR_LIBS] {
                let candidate = dir.join(sub);
                if candidate.is_dir() && let Ok(entries) = fs::read_dir(&candidate) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().and_then(|e| e.to_str()) == Some(EXT_JAR) {
                            return Some(p);
                        }
                    }
                }
            }
            if dir.ends_with(DIR_OUT) || dir.parent().is_none() {
                break;
            }
            curr = dir.parent();
        }
        None
    }

    fn find_and_read_kotlin_module(classes_dir: &Path) -> Option<(PathBuf, Vec<u8>)> {
        let meta_inf = classes_dir.join(DIR_META_INF);
        if !meta_inf.is_dir() {
            return None;
        }
        for entry in fs::read_dir(meta_inf).ok()? {
            let entry = entry.ok()?;
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|e| e.to_str()) == Some(EXT_KOTLIN_MODULE) {
                let bytes = fs::read(&p).ok()?;
                return Some((p, bytes));
            }
        }
        None
    }
}
