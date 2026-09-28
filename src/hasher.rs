use crate::constants::HASH_BUFFER_SIZE;
use crate::domain::{CompilerTraits, JvmCacheError, ParsedArgs};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct CacheHasher;

impl CacheHasher {
    pub fn hash_bytes(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    pub fn compute_key_components(
        args: &ParsedArgs,
    ) -> Result<(String, String, HashMap<PathBuf, String>, String), JvmCacheError> {
        let compiler_ver = Self::get_compiler_version(args)?;
        let mut hasher = Sha256::new();

        hasher.update(b"JVMCACHE_PROTOCOL:v3\n");
        hasher.update(b"COMPILER:");
        hasher.update(compiler_ver.as_bytes());
        hasher.update(b"\n");

        let mut sorted_flags = args.semantic_flags.clone();
        sorted_flags.sort();
        hasher.update(b"FLAGS:");
        for flag in &sorted_flags {
            hasher.update(flag.as_bytes());
            hasher.update(b";");
        }
        hasher.update(b"\n");

        let mut sorted_sources = args.source_files.clone();
        sorted_sources.sort();
        let mut source_hashes = HashMap::with_capacity(sorted_sources.len());
        hasher.update(b"SOURCES:");
        let hashed_sources = Self::hash_files_parallel(&sorted_sources)?;
        for (src_path, file_hash) in hashed_sources {
            let file_name = src_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            hasher.update(file_name.as_bytes());
            hasher.update(b":");
            hasher.update(file_hash.as_bytes());
            hasher.update(b"\n");
            source_hashes.insert(src_path, file_hash);
        }

        let mut cp_hasher = Sha256::new();
        for cp_entry in &args.classpath {
            if cp_entry.is_file() {
                if let Ok(meta) = fs::metadata(cp_entry) {
                    let path_bytes = cp_entry.to_string_lossy();
                    let len = meta.len();
                    cp_hasher.update(b"JAR:");
                    cp_hasher.update(path_bytes.as_bytes());
                    cp_hasher.update(b":");
                    cp_hasher.update(len.to_le_bytes());
                    if let Ok(digest) = Self::hash_jar_content(cp_entry, len) {
                        cp_hasher.update(b":");
                        cp_hasher.update(digest);
                    }
                    cp_hasher.update(b"\n");
                }
            } else if cp_entry.is_dir() {
                let dir_hash = Self::hash_directory(cp_entry)?;
                cp_hasher.update(b"DIR:");
                cp_hasher.update(dir_hash.as_bytes());
                cp_hasher.update(b"\n");
            }
        }
        let cp_bytes = cp_hasher.finalize();
        hasher.update(b"CLASSPATH:");
        hasher.update(cp_bytes);
        hasher.update(b"\n");
        let classpath_hash = hex::encode(cp_bytes);

        let result_bytes = hasher.finalize();
        let key_hex = hex::encode(result_bytes);
        Ok((key_hex, compiler_ver, source_hashes, classpath_hash))
    }

    pub fn compute_module_key(args: &ParsedArgs) -> String {
        let compiler_tag = if args.compiler.traits().contains(CompilerTraits::KOTLIN) {
            "Kotlin"
        } else {
            args.compiler.default_command()
        };
        let mut s = format!("v3:{}:", compiler_tag);
        for out in &args.output_dirs {
            s.push_str(&out.tag);
            s.push(':');
            s.push_str(&out.path.to_string_lossy());
            s.push(';');
        }
        if let Some(ref bf) = args.build_file_path {
            s.push_str(&bf.to_string_lossy());
        }
        let mut hasher = Sha256::new();
        hasher.update(s.as_bytes());
        hex::encode(&hasher.finalize()[..16])
    }

    pub fn compute_key(args: &ParsedArgs) -> Result<(String, String), JvmCacheError> {
        Self::compute_key_components(args).map(|(k, v, _, _)| (k, v))
    }

    pub fn get_compiler_version(args: &ParsedArgs) -> Result<String, JvmCacheError> {
        crate::fingerprint::FingerprintManager::get_compiler_version(args)
    }

    pub fn hash_file(path: &Path) -> Result<String, JvmCacheError> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; HASH_BUFFER_SIZE];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 { break; }
            hasher.update(&buffer[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    fn hash_jar_content(path: &Path, len: u64) -> Result<[u8; 32], JvmCacheError> {
        let mut file = File::open(path)?;
        let mut h = Sha256::new();
        if len <= 2 * 1024 * 1024 {
            let mut buf = Vec::with_capacity(len as usize);
            file.read_to_end(&mut buf)?;
            h.update(&buf);
        } else {
            let mut head = [0u8; 4096];
            let n = file.read(&mut head)?;
            h.update(&head[..n]);
            let tail_len = 65536u64.min(len);
            file.seek(std::io::SeekFrom::End(-(tail_len as i64)))?;
            let mut tail = Vec::with_capacity(tail_len as usize);
            file.read_to_end(&mut tail)?;
            h.update(&tail);
        }
        Ok(h.finalize().into())
    }

    pub fn hash_files_parallel(
        paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, String)>, JvmCacheError> {
        if paths.len() <= 4 {
            let mut results = Vec::with_capacity(paths.len());
            for p in paths {
                match Self::hash_file(p) {
                    Ok(hash) => results.push((p.clone(), hash)),
                    Err(JvmCacheError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                }
            }
            return Ok(results);
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(paths.len())
            .min(16);

        let chunk_size = paths.len().div_ceil(num_threads);

        std::thread::scope(|s| {
            let mut handles = Vec::new();
            for chunk in paths.chunks(chunk_size) {
                handles.push(s.spawn(move || {
                    let mut chunk_res = Vec::with_capacity(chunk.len());
                    for p in chunk {
                        match Self::hash_file(p) {
                            Ok(hash) => chunk_res.push((p.clone(), hash)),
                            Err(JvmCacheError::Io(ref e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
                            Err(e) => return Err(e),
                        }
                    }
                    Ok::<_, JvmCacheError>(chunk_res)
                }));
            }

            let mut all_results = Vec::with_capacity(paths.len());
            for handle in handles {
                let chunk_res = handle.join().map_err(|_| {
                    JvmCacheError::Execution("Worker thread panicked during parallel file hashing".to_string())
                })??;
                all_results.extend(chunk_res);
            }
            Ok(all_results)
        })
    }

    pub fn hash_directory(path: &Path) -> Result<String, JvmCacheError> {
        let mut entries: Vec<(PathBuf, String)> = Vec::new();
        for entry_res in WalkDir::new(path).sort_by_file_name() {
            let entry = match entry_res {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.file_type().is_file()
                && let Ok(rel) = entry.path().strip_prefix(path)
                    && let Ok(h) = Self::hash_file(entry.path()) {
                        entries.push((rel.to_path_buf(), h));
                    }
        }

        let mut hasher = Sha256::new();
        for (rel, file_hash) in entries {
            hasher.update(rel.to_string_lossy().as_bytes());
            hasher.update(b"=");
            hasher.update(file_hash.as_bytes());
            hasher.update(b"\n");
        }
        Ok(hex::encode(hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_files_parallel_matches_sequential() {
        let tmp = std::env::temp_dir().join(format!("hasher_par_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&tmp);

        let mut paths = Vec::new();
        let mut expected = Vec::new();

        for i in 0..12 {
            let p = tmp.join(format!("file_{:02}.txt", i));
            fs::write(&p, format!("content payload number {}", i)).unwrap();
            expected.push((p.clone(), CacheHasher::hash_file(&p).unwrap()));
            paths.push(p);
        }

        assert_eq!(CacheHasher::hash_files_parallel(&paths).unwrap(), expected);
        let _ = fs::remove_dir_all(&tmp);
    }
}
