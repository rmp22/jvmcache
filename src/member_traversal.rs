use crate::bytecode_parser::{BytecodeParser, ParsedClass};
use crate::cas::CasStorage;
use crate::constants::TAG_ABI_HEADERS;
use crate::domain::{Artifact, JvmCacheError, Manifest, OutputDirTarget};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassMutation {
    Identical,
    NonBreakingAdditive,
    SignatureMutated {
        class_name: String,
        simple_name: String,
        mutated_members: Vec<String>,
    },
}

pub struct MemberTraversal;

impl MemberTraversal {
    pub fn compare_bytecode(
        baseline_bytes: &[u8],
        new_bytes: &[u8],
    ) -> Result<ClassMutation, JvmCacheError> {
        let baseline_class = BytecodeParser::parse(baseline_bytes)?;
        let new_class = BytecodeParser::parse(new_bytes)?;
        Ok(Self::compare_classes(&baseline_class, &new_class))
    }

    pub fn compare_classes(baseline: &ParsedClass, new_class: &ParsedClass) -> ClassMutation {
        let simple_name = Self::extract_simple_name(&new_class.this_class);

        if baseline.this_class != new_class.this_class
            || baseline.super_class != new_class.super_class
        {
            return ClassMutation::SignatureMutated {
                class_name: new_class.this_class.clone(),
                simple_name,
                mutated_members: vec!["hierarchy_changed".to_string()],
            };
        }

        for iface in &baseline.interfaces {
            if !new_class.interfaces.contains(iface) {
                return ClassMutation::SignatureMutated {
                    class_name: new_class.this_class.clone(),
                    simple_name,
                    mutated_members: vec![format!("interface_removed:{iface}")],
                };
            }
        }

        let mut mutated_members = Vec::new();

        let new_method_map: HashMap<(&str, &str), bool> = new_class
            .non_private_methods
            .iter()
            .map(|m| ((m.name.as_str(), m.descriptor.as_str()), m.is_static))
            .collect();

        for base_m in &baseline.non_private_methods {
            match new_method_map.get(&(base_m.name.as_str(), base_m.descriptor.as_str())) {
                Some(is_static) if *is_static == base_m.is_static => {}
                _ => {
                    mutated_members.push(format!("{}:{}", base_m.name, base_m.descriptor));
                }
            }
        }

        let new_field_map: HashMap<(&str, &str), bool> = new_class
            .non_private_fields
            .iter()
            .map(|f| ((f.name.as_str(), f.descriptor.as_str()), f.is_static))
            .collect();

        for base_f in &baseline.non_private_fields {
            match new_field_map.get(&(base_f.name.as_str(), base_f.descriptor.as_str())) {
                Some(is_static) if *is_static == base_f.is_static => {}
                _ => {
                    mutated_members.push(format!("field {}:{}", base_f.name, base_f.descriptor));
                }
            }
        }

        if !mutated_members.is_empty() {
            ClassMutation::SignatureMutated {
                class_name: new_class.this_class.clone(),
                simple_name,
                mutated_members,
            }
        } else if new_class.non_private_methods.len() > baseline.non_private_methods.len()
            || new_class.non_private_fields.len() > baseline.non_private_fields.len()
            || new_class.interfaces.len() > baseline.interfaces.len()
        {
            ClassMutation::NonBreakingAdditive
        } else {
            ClassMutation::Identical
        }
    }

    pub fn extract_simple_name(class_name: &str) -> String {
        let base = class_name.rsplit('/').next().unwrap_or(class_name);
        let base = base.strip_suffix(".class").unwrap_or(base);
        base.strip_suffix("Kt").unwrap_or(base).to_string()
    }

    pub fn collect_mutated_symbols(
        storage_root: &Path,
        baseline_manifest: &Manifest,
        output_dirs: &[OutputDirTarget],
        new_artifacts: &[Artifact],
    ) -> HashSet<String> {
        let mut mutated = HashSet::new();
        let abi_dir = output_dirs
            .iter()
            .find(|d| d.tag == TAG_ABI_HEADERS)
            .map(|d| &d.path);

        for art in new_artifacts {
            if art.target_tag != TAG_ABI_HEADERS || art.rel_path.to_string_lossy().contains('$') {
                continue;
            }

            let baseline_art = baseline_manifest.artifacts.iter().find(|ba| {
                ba.target_tag == art.target_tag && ba.rel_path == art.rel_path
            });

            let ba = match baseline_art {
                Some(ba) => ba,
                None => {
                    mutated.insert(Self::extract_simple_name(&art.rel_path.to_string_lossy()));
                    continue;
                }
            };

            if ba.sha256 == art.sha256 {
                continue;
            }

            let base_blob = CasStorage::blob_path(storage_root, &ba.sha256);
            let base_bytes = fs::read(&base_blob).ok();
            let new_bytes = abi_dir.and_then(|d| fs::read(d.join(&art.rel_path)).ok());

            match (base_bytes, new_bytes) {
                (Some(b), Some(n)) => {
                    if let Ok(ClassMutation::SignatureMutated { simple_name, .. }) =
                        Self::compare_bytecode(&b, &n)
                    {
                        mutated.insert(simple_name);
                    }
                }
                _ => {
                    mutated.insert(Self::extract_simple_name(&art.rel_path.to_string_lossy()));
                }
            }
        }

        mutated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode_parser::ClassMember;

    fn make_class(name: &str, methods: Vec<(&str, &str)>, fields: Vec<(&str, &str)>) -> ParsedClass {
        ParsedClass {
            this_class: name.to_string(),
            super_class: Some("java/lang/Object".to_string()),
            interfaces: Vec::new(),
            non_private_methods: methods
                .into_iter()
                .map(|(n, d)| ClassMember {
                    name: n.to_string(),
                    descriptor: d.to_string(),
                    is_static: false,
                })
                .collect(),
            non_private_fields: fields
                .into_iter()
                .map(|(n, d)| ClassMember {
                    name: n.to_string(),
                    descriptor: d.to_string(),
                    is_static: false,
                })
                .collect(),
        }
    }

    #[test]
    fn test_identical_classes_returns_identical() {
        let c1 = make_class("com/example/Foo", vec![("test", "()V")], vec![]);
        let c2 = make_class("com/example/Foo", vec![("test", "()V")], vec![]);
        assert_eq!(MemberTraversal::compare_classes(&c1, &c2), ClassMutation::Identical);
    }

    #[test]
    fn test_adding_new_method_is_non_breaking_additive() {
        let c1 = make_class("com/example/Foo", vec![("test", "()V")], vec![]);
        let c2 = make_class("com/example/Foo", vec![("test", "()V"), ("newMethod", "()I")], vec![]);
        assert_eq!(
            MemberTraversal::compare_classes(&c1, &c2),
            ClassMutation::NonBreakingAdditive
        );
    }

    #[test]
    fn test_mutating_method_descriptor_is_signature_mutated() {
        let c1 = make_class("com/example/FooModel", vec![("copy$default", "(LFoo;I)V")], vec![]);
        let c2 = make_class("com/example/FooModel", vec![("copy$default", "(LFoo;II)V")], vec![]);
        match MemberTraversal::compare_classes(&c1, &c2) {
            ClassMutation::SignatureMutated { simple_name, .. } => {
                assert_eq!(simple_name, "FooModel");
            }
            _ => panic!("Expected SignatureMutated"),
        }
    }

    #[test]
    fn test_extract_simple_name_handles_kt_facade() {
        assert_eq!(MemberTraversal::extract_simple_name("com/example/UtilsKt"), "Utils");
        assert_eq!(MemberTraversal::extract_simple_name("com/example/FooModel"), "FooModel");
    }
}
