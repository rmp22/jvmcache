use crate::constants::{DIR_CAS, DIR_TMP};
use crate::domain::JvmCacheError;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn unique_temp_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}_{}", prefix, std::process::id(), nanos, count)
}

pub struct CasStorage;

impl CasStorage {
    pub fn blob_path(root_dir: &Path, sha256: &str) -> PathBuf {
        let prefix = if sha256.len() >= 2 { &sha256[..2] } else { "00" };
        root_dir.join(DIR_CAS).join(prefix).join(sha256)
    }

    pub fn store_blob(root_dir: &Path, src: &Path, sha256: &str) -> Result<(), JvmCacheError> {
        let blob_dst = Self::blob_path(root_dir, sha256);
        if !blob_dst.is_file() {
            if let Some(bp) = blob_dst.parent() {
                fs::create_dir_all(bp)?;
            }
            let blob_tmp = root_dir
                .join(DIR_TMP)
                .join(unique_temp_id(&format!("blob_{}", sha256)));
            if copy_or_link(src, &blob_tmp).is_ok() {
                let _ = fs::rename(&blob_tmp, &blob_dst);
            }
        }
        Ok(())
    }
}

pub fn copy_or_link(src: &Path, dst: &Path) -> Result<(), JvmCacheError> {
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }

    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp_dst = dst.with_extension(format!("tmp_{}", unique_temp_id("cplink")));
    fs::copy(src, &tmp_dst)?;
    let _ = fs::rename(&tmp_dst, dst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_or_link_creates_valid_file() {
        let tmp = std::env::temp_dir().join(format!("jvmcache_cas_test_{}", unique_temp_id("t")));
        let _ = fs::create_dir_all(&tmp);
        let src = tmp.join("src.txt");
        let dst = tmp.join("dst.txt");
        fs::write(&src, b"hello cas").unwrap();
        copy_or_link(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"hello cas");
        let _ = fs::remove_dir_all(&tmp);
    }
}
