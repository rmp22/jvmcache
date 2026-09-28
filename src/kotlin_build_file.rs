use crate::cas::unique_temp_id;
use crate::constants::{
    XML_ATTR_OUTPUT_DIR, XML_ATTR_PATH, XML_TAG_CLASSPATH_OPEN, XML_TAG_JAVA_ROOTS_OPEN,
    XML_TAG_MODULE_OPEN, XML_TAG_SOURCES_OPEN,
};
use crate::domain::JvmCacheError;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct KotlinBuildFileInfo {
    pub output_dir: Option<PathBuf>,
    pub sources: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
}

pub fn parse_kotlin_build_file(path: &Path) -> Option<KotlinBuildFileInfo> {
    let content = fs::read_to_string(path).ok()?;
    let mut output_dir = None;
    let mut sources = Vec::with_capacity(256);
    let mut classpath = Vec::with_capacity(256);

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(XML_TAG_MODULE_OPEN)
            && let Some(out) = extract_attribute(trimmed, XML_ATTR_OUTPUT_DIR)
        {
            output_dir = Some(PathBuf::from(out));
        } else if (trimmed.starts_with(XML_TAG_SOURCES_OPEN)
            || trimmed.starts_with(XML_TAG_JAVA_ROOTS_OPEN))
            && let Some(p) = extract_attribute(trimmed, XML_ATTR_PATH)
        {
            sources.push(PathBuf::from(p));
        } else if trimmed.starts_with(XML_TAG_CLASSPATH_OPEN)
            && let Some(p) = extract_attribute(trimmed, XML_ATTR_PATH)
        {
            classpath.push(PathBuf::from(p));
        }
    }

    Some(KotlinBuildFileInfo {
        output_dir,
        sources,
        classpath,
    })
}

pub fn extract_attribute<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let mut search_idx = 0;
    while let Some(pos) = line[search_idx..].find(attr) {
        let abs_pos = search_idx + pos;
        let after_attr = &line[abs_pos + attr.len()..];
        if after_attr.starts_with("=\"") {
            let start = abs_pos + attr.len() + 2;
            let end = line[start..].find('"')? + start;
            return Some(&line[start..end]);
        }
        search_idx = abs_pos + attr.len();
    }
    None
}

pub fn create_delta_kotlinc_build_file(
    orig_path: &Path,
    modified_sources: &[PathBuf],
    classes_dir: &Path,
    baseline_jar: Option<&Path>,
) -> Result<PathBuf, JvmCacheError> {
    let content = fs::read_to_string(orig_path)?;
    let mut modified_set = HashSet::new();
    for p in modified_sources {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            std::env::current_dir().unwrap_or_default().join(p)
        };
        let abs_str = abs.to_string_lossy().to_string();
        let canon_str = fs::canonicalize(&abs)
            .map(|c| c.to_string_lossy().to_string())
            .unwrap_or_else(|_| abs_str.clone());
        modified_set.insert(abs_str);
        modified_set.insert(canon_str);
        modified_set.insert(p.to_string_lossy().to_string());
    }

    let classes_dir_abs = if classes_dir.is_absolute() {
        classes_dir.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(classes_dir)
    };
    let classes_str = classes_dir_abs.to_string_lossy();

    let mut new_lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(XML_TAG_MODULE_OPEN) {
            let replaced_line = if let Some(old_out) = extract_attribute(trimmed, XML_ATTR_OUTPUT_DIR) {
                line.replace(
                    &format!("{}=\"{}\"", XML_ATTR_OUTPUT_DIR, old_out),
                    &format!("{}=\"{}\"", XML_ATTR_OUTPUT_DIR, classes_str),
                )
            } else {
                line.to_string()
            };
            new_lines.push(replaced_line);
            if !classes_str.is_empty() {
                if let Some(jar) = baseline_jar {
                    let jar_abs = if jar.is_absolute() {
                        jar.to_path_buf()
                    } else {
                        std::env::current_dir().unwrap_or_default().join(jar)
                    };
                    if jar_abs.is_file() {
                        let jar_str = jar_abs.to_string_lossy();
                        new_lines.push(format!("    <classpath path=\"{}\"/>", jar_str));
                        new_lines.push(format!("    <friendDir path=\"{}\"/>", jar_str));
                    }
                }
                new_lines.push(format!("    <classpath path=\"{}\"/>", classes_str));
                new_lines.push(format!("    <friendDir path=\"{}\"/>", classes_str));
            }
        } else if trimmed.starts_with(XML_TAG_SOURCES_OPEN) {
            if let Some(src_path) = extract_attribute(trimmed, XML_ATTR_PATH) {
                let candidate = PathBuf::from(src_path);
                let cand_abs = if candidate.is_absolute() {
                    candidate.to_string_lossy().to_string()
                } else {
                    std::env::current_dir().unwrap_or_default().join(&candidate).to_string_lossy().to_string()
                };
                let cand_canon = fs::canonicalize(&candidate)
                    .map(|c| c.to_string_lossy().to_string())
                    .unwrap_or_else(|_| cand_abs.clone());

                if modified_set.contains(src_path)
                    || modified_set.contains(&cand_abs)
                    || modified_set.contains(&cand_canon)
                {
                    new_lines.push(line.to_string());
                }
            }
        } else {
            new_lines.push(line.to_string());
        }
    }

    let temp_id = unique_temp_id("kotlinc");
    let tmp_path = std::env::temp_dir().join(format!("jvmcache_delta_kotlinc_{}.xml", temp_id));
    fs::write(&tmp_path, new_lines.join("\n"))?;
    Ok(tmp_path)
}
