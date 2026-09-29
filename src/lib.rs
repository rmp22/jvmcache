pub mod arg_classifier;
pub mod argfile;
pub mod args;
pub mod async_queue;
pub mod bytecode_parser;
pub mod cas;
pub mod cds;
pub mod compiler;
pub mod compiler_locator;
pub mod config;
pub mod constants;
pub mod daemon;
pub mod delta_compiler;
pub mod delta_detector;
pub mod delta_pipeline;
pub mod dependency_graph;
pub mod domain;
pub mod fingerprint;
pub mod flags;
pub mod hasher;
pub mod kotlin_build_file;
pub mod manifest_store;
pub mod member_traversal;
pub mod object_inspector;
pub mod pipeline;
pub mod storage;
pub mod target_resolver;
pub mod telemetry;
pub mod time_utils;

use crate::args::parse_compiler_args;
use crate::config::JvmCacheConfig;
use crate::domain::JvmCacheError;
use crate::hasher::CacheHasher;
use crate::pipeline::CachePipeline;
use crate::storage::CacheStorage;
use crate::telemetry::TelemetryLogger;
use std::time::Instant;

pub fn run_compiler_cache(raw_argv: &[String]) -> Result<i32, JvmCacheError> {
    let t_start = Instant::now();
    let parsed = match parse_compiler_args(raw_argv) {
        Ok(p) => p,
        Err(e) => return Err(e),
    };

    let config = JvmCacheConfig::load();
    let target_label = TelemetryLogger::extract_target_label(&parsed);
    let storage = CacheStorage::with_config(config.clone()).ok();

    if !parsed.is_compilation || storage.is_none() {
        return CachePipeline::handle_passthrough(
            &parsed,
            &config,
            storage.as_ref(),
            &target_label,
            t_start,
        );
    }

    let storage = storage.unwrap();

    let (key, compiler_version, source_hashes, classpath_hash) =
        match CacheHasher::compute_key_components(&parsed) {
            Ok(components) => components,
            Err(e) => {
                return CachePipeline::handle_hash_fallback(
                    &parsed,
                    &config,
                    &target_label,
                    &e.to_string(),
                    t_start,
                );
            }
        };

    let hash_time = t_start.elapsed();
    let module_key = CacheHasher::compute_module_key(&parsed);
    let primary_classes_dir = CachePipeline::resolve_primary_classes_dir(&parsed);

    if let Some(exit_code) = CachePipeline::try_exact_cache_hit(
        &storage,
        &key,
        &parsed,
        &config,
        &target_label,
        &module_key,
        &primary_classes_dir,
        &source_hashes,
        &classpath_hash,
        t_start,
        hash_time,
    )? {
        return Ok(exit_code);
    }

    if let Some(exit_code) = CachePipeline::try_delta_compilation(
        &storage,
        &parsed,
        &config,
        &key,
        &module_key,
        &target_label,
        &compiler_version,
        &source_hashes,
        &classpath_hash,
        &primary_classes_dir,
        t_start,
    )? {
        return Ok(exit_code);
    }

    CachePipeline::execute_full_compilation(
        &storage,
        &parsed,
        &config,
        &key,
        &module_key,
        &target_label,
        compiler_version,
        source_hashes,
        classpath_hash,
        primary_classes_dir,
        t_start,
        hash_time,
    )
}
