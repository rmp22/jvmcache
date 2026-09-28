use crate::arg_classifier::ArgClassifier;
use crate::argfile::expand_argfiles;
use crate::compiler_locator::CompilerLocator;
use crate::constants::{
    FLAG_DEPS_FILE, FLAG_DIR, FLAG_GLOBALS_OUTPUT, FLAG_HEADER_DIR, FLAG_INJARS,
    FLAG_NO_DEX_INPUT_JAR, FLAG_OUTPUT_LONG, FLAG_OUTPUT_SHORT, FLAG_PACKAGE_OUTPUT,
    FLAG_PRINT_CONFIGURATION, FLAG_PRINT_MAPPING, FLAG_PRINT_USAGE, FLAG_SRC_DIR, TAG_ANNO,
    TAG_CLASSES, TAG_DEX, TAG_DEX_GLOBALS, TAG_DEX_PACKAGES, TAG_HEADERS, TAG_R8_CONFIG,
    TAG_R8_DEPS, TAG_R8_DICT, TAG_R8_USAGE,
};
use crate::domain::{CompilerKind, JvmCacheError, OutputDirTarget, ParsedArgs};
use crate::kotlin_build_file::parse_kotlin_build_file;
use std::path::{Path, PathBuf};

pub use crate::compiler_locator::find_aosp_root;

pub fn parse_compiler_args(argv: &[String]) -> Result<ParsedArgs, JvmCacheError> {
    let (compiler, compiler_args) = detect_compiler(argv)?;
    let expanded_args = expand_argfiles(compiler_args, 0)?;
    let real_path = CompilerLocator::find_real_compiler(compiler)?;

    let mut output_dirs: Vec<OutputDirTarget> = Vec::new();
    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut source_files: Vec<PathBuf> = Vec::new();
    let mut semantic_flags: Vec<String> = Vec::new();
    let mut non_semantic_flags: Vec<String> = Vec::new();
    let mut is_compilation = true;
    let mut build_file_path: Option<PathBuf> = None;

    let mut i = 0;
    while i < expanded_args.len() {
        let arg = &expanded_args[i];

        if ArgClassifier::is_info_or_help_flag(arg) {
            is_compilation = false;
            semantic_flags.push(arg.clone());
            i += 1;
            continue;
        }

        if let Some(bf) = ArgClassifier::extract_build_file_arg(arg, &expanded_args, &mut i) {
            let path = PathBuf::from(&bf);
            build_file_path = Some(path.clone());
            if let Some(info) = parse_kotlin_build_file(&path) {
                if let Some(out) = info.output_dir {
                    ArgClassifier::add_output_dir(&mut output_dirs, TAG_CLASSES, out);
                }
                source_files.extend(info.sources);
                classpath.extend(info.classpath);
            }
            continue;
        }

        if (arg == FLAG_DIR || arg == FLAG_SRC_DIR || arg == FLAG_HEADER_DIR)
            && i + 1 < expanded_args.len()
        {
            let path = PathBuf::from(&expanded_args[i + 1]);
            let tag = match arg.as_str() {
                FLAG_DIR => TAG_CLASSES,
                FLAG_SRC_DIR => TAG_ANNO,
                FLAG_HEADER_DIR => TAG_HEADERS,
                _ => TAG_CLASSES,
            };
            ArgClassifier::add_output_dir(&mut output_dirs, tag, path);
            i += 2;
            continue;
        }

        if (compiler == CompilerKind::D8 || compiler == CompilerKind::R8)
            && i + 1 < expanded_args.len()
        {
            let next = &expanded_args[i + 1];
            if arg == FLAG_OUTPUT_LONG || arg == FLAG_OUTPUT_SHORT {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_DEX, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_PACKAGE_OUTPUT {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_DEX_PACKAGES, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_PRINT_MAPPING {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_R8_DICT, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_PRINT_CONFIGURATION {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_R8_CONFIG, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_PRINT_USAGE {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_R8_USAGE, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_DEPS_FILE {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_R8_DEPS, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_GLOBALS_OUTPUT {
                ArgClassifier::add_output_dir(&mut output_dirs, TAG_DEX_GLOBALS, PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == FLAG_NO_DEX_INPUT_JAR {
                semantic_flags.push(arg.clone());
                i += 1;
                continue;
            }
            if arg == FLAG_INJARS {
                source_files.push(PathBuf::from(next));
                semantic_flags.push(format!("-injars={}", next));
                i += 2;
                continue;
            }
            if arg == "-libraryjars" || arg == "--lib" || arg == "--classpath" {
                classpath.push(PathBuf::from(next));
                i += 2;
                continue;
            }
            if arg == "--pg-conf"
                || arg == "--packages"
                || arg == "--mod-packages"
                || arg == "--main-dex-rules"
                || arg == "--main-dex-list"
                || arg == "--globals"
            {
                let p = PathBuf::from(next);
                if p.is_file() {
                    source_files.push(p);
                }
                semantic_flags.push(format!("{}={}", arg, next));
                i += 2;
                continue;
            }
        }

        if let Some(target) = ArgClassifier::extract_plugin_output_dir(arg) {
            ArgClassifier::add_output_dir(&mut output_dirs, &target.tag, target.path);
        }

        if ArgClassifier::is_classpath_flag(arg) && i + 1 < expanded_args.len() {
            for entry in expanded_args[i + 1].split(':') {
                if !entry.trim().is_empty() {
                    classpath.push(PathBuf::from(entry));
                }
            }
            i += 2;
            continue;
        }

        if ArgClassifier::is_option_with_value(arg) && i + 1 < expanded_args.len() {
            semantic_flags.push(format!("{}={}", arg, expanded_args[i + 1]));
            i += 2;
            continue;
        }

        if ArgClassifier::is_non_semantic_flag(arg) {
            non_semantic_flags.push(arg.clone());
            i += 1;
            continue;
        }

        if ArgClassifier::is_source_file(arg, compiler) {
            source_files.push(PathBuf::from(arg));
        } else if arg.starts_with('-') {
            semantic_flags.push(arg.clone());
        }
        i += 1;
    }

    if source_files.is_empty() {
        is_compilation = false;
    }

    if output_dirs.is_empty() {
        let tag = ArgClassifier::fallback_tag_for_compiler(compiler);
        let p = if let Some(first_src) = source_files.first() {
            first_src
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        } else {
            PathBuf::from(".")
        };
        output_dirs.push(OutputDirTarget {
            tag: tag.into(),
            path: p,
        });
    }

    Ok(ParsedArgs {
        compiler,
        real_compiler_path: real_path,
        output_dirs,
        classpath,
        source_files,
        semantic_flags,
        non_semantic_flags,
        is_compilation,
        raw_args: expanded_args,
        build_file_path,
    })
}

fn detect_compiler(argv: &[String]) -> Result<(CompilerKind, &[String]), JvmCacheError> {
    if argv.is_empty() {
        return Err(JvmCacheError::InvalidInvocation("Empty arguments list".into()));
    }
    let prog = Path::new(&argv[0])
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if let Some(kind) = CompilerKind::from_binary_name(prog) {
        return Ok((kind, &argv[1..]));
    }
    if argv.len() >= 2
        && let Some(kind) = CompilerKind::from_binary_name(&argv[1])
    {
        return Ok((kind, &argv[2..]));
    }
    Err(JvmCacheError::InvalidInvocation(format!(
        "Unable to determine compiler target from command: {}",
        argv.join(" ")
    )))
}
