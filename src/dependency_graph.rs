use crate::compiler::{CompilerRunner, ExecutionResult};
use crate::config::JvmCacheConfig;
use crate::domain::{JvmCacheError, Manifest, ParsedArgs};
use crate::member_traversal::MemberTraversal;
use crate::storage::CacheStorage;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct DependencyGraph;

impl DependencyGraph {
    pub fn resolve_targeted_expansion(
        parsed: &ParsedArgs,
        storage: &CacheStorage,
        baseline_manifest: &Manifest,
        config: &JvmCacheConfig,
        primary_classes_dir: &Path,
        delta_sources: &[PathBuf],
        initial_result: ExecutionResult,
    ) -> Result<Option<ExecutionResult>, JvmCacheError> {
        if delta_sources.len() >= parsed.source_files.len() {
            return Ok(Some(initial_result));
        }

        let mutated_symbols = MemberTraversal::collect_mutated_symbols(
            storage.root_dir(),
            baseline_manifest,
            &parsed.output_dirs,
            &initial_result.artifacts,
        );

        if mutated_symbols.is_empty() {
            return Ok(Some(initial_result));
        }

        if config.strict_abi {
            return Ok(None);
        }

        let callers = match Self::find_affected_callers(
            &parsed.source_files,
            delta_sources,
            &mutated_symbols,
            config.max_targeted_callers,
        ) {
            Some(c) => c,
            None => return Ok(None),
        };

        if callers.is_empty() {
            return Ok(Some(initial_result));
        }

        let mut expanded = delta_sources.to_vec();
        expanded.extend(callers);

        match CompilerRunner::execute_delta(parsed, &expanded, primary_classes_dir, config) {
            Ok(res) if res.exit_code == 0 => Ok(Some(res)),
            _ => Ok(None),
        }
    }
    pub fn find_affected_callers(
        source_files: &[PathBuf],
        already_modified: &[PathBuf],
        mutated_symbols: &HashSet<String>,
        max_callers: usize,
    ) -> Option<HashSet<PathBuf>> {
        if mutated_symbols.is_empty() || source_files.is_empty() {
            return Some(HashSet::new());
        }

        let modified_set: HashSet<&Path> = already_modified.iter().map(|p| p.as_path()).collect();
        let candidate_files: Vec<&PathBuf> = source_files
            .iter()
            .filter(|p| !modified_set.contains(p.as_path()) && p.is_file())
            .collect();

        if candidate_files.is_empty() {
            return Some(HashSet::new());
        }

        let symbol_bytes: Vec<Vec<u8>> = mutated_symbols
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();

        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(candidate_files.len())
            .min(16);

        let chunk_size = candidate_files.len().div_ceil(num_threads);

        let callers_per_thread = std::thread::scope(|s| {
            let mut handles = Vec::new();
            for chunk in candidate_files.chunks(chunk_size) {
                let symbols = &symbol_bytes;
                handles.push(s.spawn(move || {
                    let mut found = Vec::new();
                    for file_path in chunk {
                        if let Ok(content) = fs::read(file_path) {
                            for sym in symbols {
                                if Self::contains_isolated_identifier(&content, sym) {
                                    found.push((*file_path).clone());
                                    break;
                                }
                            }
                        }
                    }
                    found
                }));
            }

            let mut all_callers = HashSet::new();
            for handle in handles {
                if let Ok(list) = handle.join() {
                    for path in list {
                        all_callers.insert(path);
                        if all_callers.len() > max_callers {
                            return None;
                        }
                    }
                }
            }
            Some(all_callers)
        });

        callers_per_thread
    }

    pub fn contains_isolated_identifier(haystack: &[u8], needle: &[u8]) -> bool {
        if needle.is_empty() || haystack.len() < needle.len() {
            return false;
        }

        let mut offset = 0;
        while let Some(pos) = Self::find_subslice(&haystack[offset..], needle) {
            let abs_pos = offset + pos;
            let left_ok = abs_pos == 0 || !Self::is_identifier_char(haystack[abs_pos - 1]);
            let right_pos = abs_pos + needle.len();
            let right_ok = right_pos >= haystack.len() || !Self::is_identifier_char(haystack[right_pos]);

            if left_ok && right_ok {
                return true;
            }
            offset = abs_pos + 1;
            if offset + needle.len() > haystack.len() {
                break;
            }
        }
        false
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    fn is_identifier_char(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_isolated_identifier_exact_match() {
        let code = b"import com.example.FooModel\nclass Bar(val m: FooModel)";
        assert!(DependencyGraph::contains_isolated_identifier(code, b"FooModel"));
    }

    #[test]
    fn test_contains_isolated_identifier_rejects_prefixes_and_suffixes() {
        let code = b"class MyFooModel : FooModelHelper";
        assert!(!DependencyGraph::contains_isolated_identifier(code, b"FooModel"));
    }

    #[test]
    fn test_find_affected_callers_finds_real_callers() {
        let tmp = std::env::temp_dir().join(format!("dep_graph_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);

        let f_model = tmp.join("FooModel.kt");
        let f_caller1 = tmp.join("BarRepo.kt");
        let f_caller2 = tmp.join("BarViewModel.kt");
        let f_unrelated = tmp.join("BazView.kt");

        fs::write(&f_model, b"data class FooModel(val id: Int)").unwrap();
        fs::write(&f_caller1, b"import FooModel\nval x = FooModel(1)").unwrap();
        fs::write(&f_caller2, b"val y: FooModel? = null").unwrap();
        fs::write(&f_unrelated, b"class BazView").unwrap();

        let sources = vec![f_model.clone(), f_caller1.clone(), f_caller2.clone(), f_unrelated];
        let mut symbols = HashSet::new();
        symbols.insert("FooModel".to_string());

        let callers = DependencyGraph::find_affected_callers(&sources, &[f_model], &symbols, 50).unwrap();
        assert_eq!(callers.len(), 2);
        assert!(callers.contains(&f_caller1));
        assert!(callers.contains(&f_caller2));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_affected_callers_exceeding_ceiling_returns_none() {
        let tmp = std::env::temp_dir().join(format!("dep_graph_ceiling_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);

        let mut sources = Vec::new();
        for i in 0..10 {
            let p = tmp.join(format!("Caller_{i}.kt"));
            fs::write(&p, b"val m: CoreSymbol = TODO()").unwrap();
            sources.push(p);
        }

        let mut symbols = HashSet::new();
        symbols.insert("CoreSymbol".to_string());

        let res = DependencyGraph::find_affected_callers(&sources, &[], &symbols, 5);
        assert!(res.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
