use crate::constants::{
    AOSP_ANDROID_COMMON_MARKER, AOSP_INTERMEDIATES_MARKER, FALLBACK_GENERIC_TARGET,
};
use crate::domain::ParsedArgs;

pub fn extract_target_label(parsed: &ParsedArgs) -> String {
    for target in &parsed.output_dirs {
        let path_str = target.path.to_string_lossy();
        if let Some(pos) = path_str.find(AOSP_INTERMEDIATES_MARKER) {
            let sub = &path_str[pos + AOSP_INTERMEDIATES_MARKER.len()..];
            if let Some(end) = sub.find(AOSP_ANDROID_COMMON_MARKER) {
                return sub[..end].to_string();
            }
            return sub.to_string();
        }
        if !path_str.is_empty() {
            return path_str.to_string();
        }
    }

    if let Some(ref bf) = parsed.build_file_path {
        let bf_str = bf.to_string_lossy();
        if let Some(pos) = bf_str.find(AOSP_INTERMEDIATES_MARKER) {
            let sub = &bf_str[pos + AOSP_INTERMEDIATES_MARKER.len()..];
            if let Some(end) = sub.find(AOSP_ANDROID_COMMON_MARKER) {
                return sub[..end].to_string();
            }
            return sub.to_string();
        }
        return bf_str.to_string();
    }

    if let Some(first_src) = parsed.source_files.first() {
        if let Some(parent) = first_src.parent() {
            let s = parent.to_string_lossy();
            if !s.is_empty() {
                return s.into_owned();
            }
        }
    }

    FALLBACK_GENERIC_TARGET.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::TAG_CLASSES;
    use crate::domain::{CompilerKind, OutputDirTarget, ParsedArgs};
    use std::path::PathBuf;

    #[test]
    fn test_extract_target_label() {
        let parsed = ParsedArgs {
            compiler: CompilerKind::Kotlinc,
            real_compiler_path: PathBuf::from("kotlinc"),
            output_dirs: vec![OutputDirTarget {
                tag: TAG_CLASSES.to_string(),
                path: PathBuf::from(
                    "out/soong/.intermediates/packages/apps/FooBarApp/FooBarApp-core/android_common/javac/classes",
                ),
            }],
            classpath: Vec::new(),
            source_files: Vec::new(),
            semantic_flags: Vec::new(),
            non_semantic_flags: Vec::new(),
            is_compilation: true,
            raw_args: Vec::new(),
            build_file_path: None,
        };

        let label = extract_target_label(&parsed);
        assert_eq!(label, "packages/apps/FooBarApp/FooBarApp-core");
    }
}
