use jvmcache::bytecode_parser::{BytecodeParser, ClassMember, ParsedClass};
use jvmcache::compiler::ExecutionResult;
use jvmcache::config::JvmCacheConfig;
use jvmcache::constants::TAG_ABI_HEADERS;
use jvmcache::dependency_graph::DependencyGraph;
use jvmcache::domain::{Artifact, CompilerKind, Manifest, OutputDirTarget, ParsedArgs};
use jvmcache::member_traversal::{ClassMutation, MemberTraversal};
use jvmcache::storage::CacheStorage;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn make_temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jvmcache_abi_test_{}_{}", prefix, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn synthesize_valid_class_bytes(class_name: &str, method_name: &str, descriptor: &str) -> Vec<u8> {
    let mut cp = Vec::new();
    cp.push(vec![]); // 1-indexed

    let utf8_class = cp.len();
    let mut b1 = vec![1, 0, class_name.len() as u8];
    b1.extend_from_slice(class_name.as_bytes());
    cp.push(b1);

    let utf8_super = cp.len();
    let super_name = "java/lang/Object";
    let mut b2 = vec![1, 0, super_name.len() as u8];
    b2.extend_from_slice(super_name.as_bytes());
    cp.push(b2);

    let utf8_method = cp.len();
    let mut b3 = vec![1, 0, method_name.len() as u8];
    b3.extend_from_slice(method_name.as_bytes());
    cp.push(b3);

    let utf8_desc = cp.len();
    let mut b4 = vec![1, 0, descriptor.len() as u8];
    b4.extend_from_slice(descriptor.as_bytes());
    cp.push(b4);

    let class_idx = cp.len();
    let mut b5 = vec![7];
    b5.extend_from_slice(&(utf8_class as u16).to_be_bytes());
    cp.push(b5);

    let super_idx = cp.len();
    let mut b6 = vec![7];
    b6.extend_from_slice(&(utf8_super as u16).to_be_bytes());
    cp.push(b6);

    let mut result = Vec::new();
    result.extend_from_slice(&0xCAFEBABEu32.to_be_bytes());
    result.extend_from_slice(&0u16.to_be_bytes()); // minor
    result.extend_from_slice(&61u16.to_be_bytes()); // major (Java 17)
    result.extend_from_slice(&(cp.len() as u16).to_be_bytes());
    for item in &cp[1..] {
        result.extend_from_slice(item);
    }
    result.extend_from_slice(&0x0001u16.to_be_bytes()); // ACC_PUBLIC
    result.extend_from_slice(&(class_idx as u16).to_be_bytes());
    result.extend_from_slice(&(super_idx as u16).to_be_bytes());
    result.extend_from_slice(&0u16.to_be_bytes()); // interfaces_count
    result.extend_from_slice(&0u16.to_be_bytes()); // fields_count

    result.extend_from_slice(&1u16.to_be_bytes()); // methods_count
    result.extend_from_slice(&0x0001u16.to_be_bytes()); // ACC_PUBLIC
    result.extend_from_slice(&(utf8_method as u16).to_be_bytes());
    result.extend_from_slice(&(utf8_desc as u16).to_be_bytes());
    result.extend_from_slice(&0u16.to_be_bytes()); // attributes_count

    result.extend_from_slice(&0u16.to_be_bytes()); // class attributes_count
    result
}

#[test]
fn test_bytecode_parser_and_member_mutation_flow() {
    let bytes_v1 = synthesize_valid_class_bytes("com/example/FooService", "executeAction", "()V");
    let bytes_v2_identical = synthesize_valid_class_bytes("com/example/FooService", "executeAction", "()V");
    let bytes_v3_mutated = synthesize_valid_class_bytes("com/example/FooService", "executeAction", "(I)V");

    let parsed_v1 = BytecodeParser::parse(&bytes_v1).expect("v1 parse failed");
    let parsed_v2 = BytecodeParser::parse(&bytes_v2_identical).expect("v2 parse failed");
    let parsed_v3 = BytecodeParser::parse(&bytes_v3_mutated).expect("v3 parse failed");

    assert_eq!(parsed_v1.this_class, "com/example/FooService");
    assert_eq!(parsed_v1.non_private_methods.len(), 1);
    assert_eq!(parsed_v1.non_private_methods[0].name, "executeAction");
    assert_eq!(parsed_v1.non_private_methods[0].descriptor, "()V");

    let mutation_identical = MemberTraversal::compare_classes(&parsed_v1, &parsed_v2);
    assert_eq!(mutation_identical, ClassMutation::Identical);

    let mutation_changed = MemberTraversal::compare_classes(&parsed_v1, &parsed_v3);
    match mutation_changed {
        ClassMutation::SignatureMutated { simple_name, mutated_members, .. } => {
            assert_eq!(simple_name, "FooService");
            assert!(mutated_members.iter().any(|m| m.contains("executeAction")));
        }
        _ => panic!("Expected SignatureMutated"),
    }
}

#[test]
fn test_additive_new_method_detection() {
    let base = ParsedClass {
        this_class: "com/example/BarRepo".to_string(),
        super_class: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        non_private_methods: vec![ClassMember {
            name: "findId".to_string(),
            descriptor: "(I)Ljava/lang/String;".to_string(),
            is_static: false,
        }],
        non_private_fields: vec![],
    };

    let mut added = base.clone();
    added.non_private_methods.push(ClassMember {
        name: "findAll".to_string(),
        descriptor: "()Ljava/util/List;".to_string(),
        is_static: false,
    });

    let mutation = MemberTraversal::compare_classes(&base, &added);
    assert_eq!(mutation, ClassMutation::NonBreakingAdditive);
}

#[test]
fn test_dependency_graph_boundary_conditions() {
    let content = b"package com.example;\nimport com.example.FooBar;\nclass MyClass { FooBar field; }";

    assert!(DependencyGraph::contains_isolated_identifier(content, b"FooBar"));
    assert!(!DependencyGraph::contains_isolated_identifier(content, b"Foo"));
    assert!(!DependencyGraph::contains_isolated_identifier(content, b"Bar"));
    assert!(!DependencyGraph::contains_isolated_identifier(content, b"Class"));
    assert!(DependencyGraph::contains_isolated_identifier(content, b"MyClass"));

    let empty = b"";
    assert!(!DependencyGraph::contains_isolated_identifier(empty, b"FooBar"));
    assert!(!DependencyGraph::contains_isolated_identifier(content, b""));
}

#[test]
fn test_targeted_caller_discovery_and_ceiling_defense() {
    let tmp = make_temp_dir("dep_ceiling");
    let model_src = tmp.join("FooModel.kt");
    fs::write(&model_src, b"package com.example.model\ndata class FooModel(val flag: Int)").unwrap();

    let mut sources = vec![model_src.clone()];
    for i in 0..12 {
        let caller = tmp.join(format!("BarConsumer_{}.kt", i));
        fs::write(&caller, format!("import com.example.model.FooModel\nval x_{} : FooModel? = null", i).as_bytes()).unwrap();
        sources.push(caller);
    }

    let mut symbols = HashSet::new();
    symbols.insert("FooModel".to_string());

    let callers_limited = DependencyGraph::find_affected_callers(&sources, &[model_src.clone()], &symbols, 5);
    assert!(callers_limited.is_none());

    let callers_allowed = DependencyGraph::find_affected_callers(&sources, &[model_src], &symbols, 50).unwrap();
    assert_eq!(callers_allowed.len(), 12);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_extract_simple_name_variants() {
    assert_eq!(MemberTraversal::extract_simple_name("com/example/Foo"), "Foo");
    assert_eq!(MemberTraversal::extract_simple_name("com/example/BarKt"), "Bar");
    assert_eq!(MemberTraversal::extract_simple_name("com/example/Baz.class"), "Baz");
    assert_eq!(MemberTraversal::extract_simple_name("com/example/HelperKt.class"), "Helper");
}

#[test]
fn test_strict_abi_flag_enforcement() {
    let tmp = make_temp_dir("strict_abi");
    let mut config = JvmCacheConfig::default();
    config.cache_dir = tmp.join("cache");
    config.strict_abi = true;
    let storage = CacheStorage::with_config(config.clone()).unwrap();

    let parsed = ParsedArgs {
        compiler: CompilerKind::Kotlinc,
        real_compiler_path: PathBuf::from("kotlinc"),
        output_dirs: vec![OutputDirTarget {
            tag: TAG_ABI_HEADERS.into(),
            path: tmp.join("abi"),
        }],
        classpath: vec![],
        source_files: vec![tmp.join("Foo.kt"), tmp.join("Bar.kt")],
        semantic_flags: vec![],
        non_semantic_flags: vec![],
        is_compilation: true,
        raw_args: vec![],
        build_file_path: None,
    };

    let manifest = Manifest {
        cache_key: "base_key".to_string(),
        compiler_kind: CompilerKind::Kotlinc,
        compiler_version: "2.1".to_string(),
        created_at_epoch_secs: 100,
        exit_code: 0,
        stdout: "".to_string(),
        stderr: "".to_string(),
        artifacts: vec![Artifact {
            rel_path: PathBuf::from("com/example/Foo.class"),
            target_tag: TAG_ABI_HEADERS.into(),
            size_bytes: 100,
            sha256: "000011112222".to_string(),
        }],
    };

    let res = ExecutionResult {
        exit_code: 0,
        stdout: "".to_string(),
        stderr: "".to_string(),
        artifacts: vec![Artifact {
            rel_path: PathBuf::from("com/example/Foo.class"),
            target_tag: TAG_ABI_HEADERS.into(),
            size_bytes: 120,
            sha256: "333344445555".to_string(),
        }],
        output_dirs: vec![],
    };

    let expansion = DependencyGraph::resolve_targeted_expansion(
        &parsed,
        &storage,
        &manifest,
        &config,
        &tmp.join("classes"),
        &[tmp.join("Foo.kt")],
        res,
    ).unwrap();

    assert!(expansion.is_none());
    let _ = fs::remove_dir_all(&tmp);
}
