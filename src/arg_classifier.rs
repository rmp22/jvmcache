use crate::constants::{
    FLAG_BOOTCLASSPATH, FLAG_CLASSPATH, FLAG_CLASS_PATH, FLAG_CP, FLAG_BUILD_FILE,
    FLAG_BUILD_FILE_PREFIX, FLAG_XBUILD_FILE, FLAG_XBUILD_FILE_PREFIX, PLUGIN_ATTR_OUTPUT_DIR,
    PLUGIN_MARKER_JVM_ABI, PLUGIN_PREFIX_KAPT_CLASSES, PLUGIN_PREFIX_KAPT_SOURCES,
    PLUGIN_PREFIX_KAPT_STUBS, TAG_ABI_HEADERS, TAG_KAPT_CLASSES, TAG_KAPT_SOURCES, TAG_KAPT_STUBS,
};
use crate::domain::{CompilerKind, OutputDirTarget};
use std::path::PathBuf;

pub struct ArgClassifier;

impl ArgClassifier {
    pub fn extract_build_file_arg(
        arg: &str,
        args: &[String],
        i: &mut usize,
    ) -> Option<String> {
        if let Some(bf) = arg
            .strip_prefix(FLAG_XBUILD_FILE_PREFIX)
            .or_else(|| arg.strip_prefix(FLAG_BUILD_FILE_PREFIX))
        {
            *i += 1;
            Some(bf.to_string())
        } else if (arg == FLAG_XBUILD_FILE || arg == FLAG_BUILD_FILE) && *i + 1 < args.len() {
            let val = args[*i + 1].clone();
            *i += 2;
            Some(val)
        } else {
            None
        }
    }

    pub fn extract_plugin_output_dir(arg: &str) -> Option<OutputDirTarget> {
        if arg.contains(PLUGIN_MARKER_JVM_ABI) && let Some(pos) = arg.find(PLUGIN_ATTR_OUTPUT_DIR) {
            return Some(OutputDirTarget {
                tag: TAG_ABI_HEADERS.into(),
                path: PathBuf::from(&arg[pos + PLUGIN_ATTR_OUTPUT_DIR.len()..]),
            });
        }
        for (prefix, tag) in [
            (PLUGIN_PREFIX_KAPT_STUBS, TAG_KAPT_STUBS),
            (PLUGIN_PREFIX_KAPT_SOURCES, TAG_KAPT_SOURCES),
            (PLUGIN_PREFIX_KAPT_CLASSES, TAG_KAPT_CLASSES),
        ] {
            if let Some(pos) = arg.find(prefix) {
                return Some(OutputDirTarget {
                    tag: tag.into(),
                    path: PathBuf::from(&arg[pos + prefix.len()..]),
                });
            }
        }
        None
    }

    pub fn add_output_dir(dirs: &mut Vec<OutputDirTarget>, tag: &str, path: PathBuf) {
        if !dirs.iter().any(|d| d.tag == tag) {
            dirs.push(OutputDirTarget {
                tag: tag.into(),
                path,
            });
        }
    }

    pub fn is_classpath_flag(arg: &str) -> bool {
        matches!(
            arg,
            FLAG_CP | FLAG_CLASSPATH | FLAG_CLASS_PATH | FLAG_BOOTCLASSPATH
        )
    }

    pub fn is_info_or_help_flag(arg: &str) -> bool {
        matches!(
            arg,
            "-version" | "--version" | "-help" | "--help" | "-X" | "-?"
        )
    }

    pub fn is_non_semantic_flag(arg: &str) -> bool {
        arg == "-verbose" || arg.starts_with("-J")
    }

    pub fn is_option_with_value(arg: &str) -> bool {
        matches!(
            arg,
            "-source"
                | "-target"
                | "--release"
                | "-encoding"
                | "-jvm-target"
                | "-opt-in"
                | "-module-name"
                | "-sourcepath"
                | "--source-path"
                | "--min-api"
                | "--pg-conf"
                | "--pg-map"
                | "--main-dex-list"
                | "--desugared-lib"
        )
    }

    pub fn is_source_file(arg: &str, compiler: CompilerKind) -> bool {
        if arg.starts_with('-') || arg.starts_with('@') {
            return false;
        }
        let ext = std::path::Path::new(arg)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        compiler.is_valid_source_extension(ext)
    }

    pub fn fallback_tag_for_compiler(compiler: CompilerKind) -> &'static str {
        compiler.default_output_tag()
    }
}
