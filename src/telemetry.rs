use crate::domain::{Artifact, CompilerKind, ParsedArgs};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use crate::object_inspector::{ManifestSummary, ObjectInspector};
pub use crate::target_resolver::extract_target_label;
pub use crate::time_utils::{format_duration, format_epoch_secs};

pub struct TelemetryLogger;

impl TelemetryLogger {
    pub fn log_hit(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        key: &str,
        sources_count: usize,
        artifacts: &[Artifact],
        total_time: Duration,
        hash_time: Duration,
        restore_time: Duration,
    ) {
        let short_key = if key.len() >= 16 { &key[..16] } else { key };
        let timestamp = Self::current_timestamp();
        let total_str = Self::format_duration(total_time);
        let hash_str = Self::format_duration(hash_time);
        let restore_str = Self::format_duration(restore_time);
        let sample = Self::format_sample_artifacts(artifacts);

        let entry = format!(
            "[{timestamp}] [HIT ] [{compiler:<7?}] target=\"{target}\" sources={sources_count} artifacts={} duration={total_str} (hash={hash_str}, restore={restore_str}) key={short_key}\n      <- restored {} classes/objects{sample}\n",
            artifacts.len(),
            artifacts.len(),
        );

        Self::append_log(cache_dir, &entry);
    }

    pub fn log_delta_hit(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        key: &str,
        modified_count: usize,
        total_sources: usize,
        artifacts_count: usize,
        total_time: Duration,
        restore_time: Duration,
        exec_time: Duration,
        store_time: Duration,
    ) {
        let short_key = if key.len() >= 16 { &key[..16] } else { key };
        let timestamp = Self::current_timestamp();
        let total_str = Self::format_duration(total_time);
        let restore_str = Self::format_duration(restore_time);
        let exec_str = Self::format_duration(exec_time);
        let store_str = Self::format_duration(store_time);

        let entry = format!(
            "[{timestamp}] [DELTA] [{compiler:<7?}] target=\"{target}\" modified={modified_count}/{total_sources} artifacts={artifacts_count} duration={total_str} (restore={restore_str}, exec={exec_str}, store={store_str}) key={short_key}\n      <- delta recompiled {modified_count} sources, preserved {} baseline classes/objects\n",
            artifacts_count.saturating_sub(modified_count)
        );

        Self::append_log(cache_dir, &entry);
    }

    pub fn log_miss(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        key: &str,
        sources_count: usize,
        artifacts: &[Artifact],
        total_time: Duration,
        hash_time: Duration,
        exec_time: Duration,
        store_time: Duration,
    ) {
        let short_key = if key.len() >= 16 { &key[..16] } else { key };
        let timestamp = Self::current_timestamp();
        let total_str = Self::format_duration(total_time);
        let hash_str = Self::format_duration(hash_time);
        let exec_str = Self::format_duration(exec_time);
        let store_str = Self::format_duration(store_time);
        let sample = Self::format_sample_artifacts(artifacts);

        let entry = format!(
            "[{timestamp}] [MISS] [{compiler:<7?}] target=\"{target}\" sources={sources_count} artifacts={} duration={total_str} (hash={hash_str}, exec={exec_str}, store={store_str}) key={short_key}\n      -> stored {} classes/objects{sample}\n",
            artifacts.len(),
            artifacts.len(),
        );

        Self::append_log(cache_dir, &entry);
    }

    pub fn log_passthrough(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        reason: &str,
        total_time: Duration,
    ) {
        let timestamp = Self::current_timestamp();
        let total_str = Self::format_duration(total_time);

        let entry = format!(
            "[{timestamp}] [PASS] [{compiler:<7?}] target=\"{target}\" reason=\"{reason}\" duration={total_str}\n",
        );

        Self::append_log(cache_dir, &entry);
    }

    pub fn log_delta_skip(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        reason: &str,
        detail: &str,
    ) {
        let timestamp = Self::current_timestamp();
        let entry = if detail.is_empty() {
            format!("[{timestamp}] [DSKIP] [{compiler:<7?}] target=\"{target}\" reason=\"{reason}\"\n")
        } else {
            format!("[{timestamp}] [DSKIP] [{compiler:<7?}] target=\"{target}\" reason=\"{reason}\"\n      detail: {detail}\n")
        };
        Self::append_log(cache_dir, &entry);
    }

    pub fn log_failure(
        cache_dir: &Path,
        compiler: CompilerKind,
        target: &str,
        key: &str,
        exit_code: i32,
        total_time: Duration,
    ) {
        let short_key = if key.len() >= 16 { &key[..16] } else { key };
        let timestamp = Self::current_timestamp();
        let total_str = Self::format_duration(total_time);

        let entry = format!(
            "[{timestamp}] [FAIL] [{compiler:<7?}] target=\"{target}\" exit_code={exit_code} duration={total_str} key={short_key}\n",
        );

        Self::append_log(cache_dir, &entry);
    }

    pub fn read_recent_entries(cache_dir: &Path, limit: usize) -> Vec<String> {
        let log_path = cache_dir.join("activity.log");
        let file = match File::open(&log_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        if lines.len() <= limit {
            lines
        } else {
            lines[lines.len() - limit..].to_vec()
        }
    }

    pub fn clear_log(cache_dir: &Path) -> io::Result<()> {
        let log_path = cache_dir.join("activity.log");
        if log_path.exists() {
            fs::remove_file(&log_path)?;
        }
        Ok(())
    }

    pub fn list_objects(
        cache_dir: &Path,
        filter: Option<&str>,
        limit: usize,
    ) -> Vec<ManifestSummary> {
        crate::object_inspector::ObjectInspector::list_objects(cache_dir, filter, limit)
    }

    pub fn extract_target_label(parsed: &ParsedArgs) -> String {
        crate::target_resolver::extract_target_label(parsed)
    }

    fn append_log(cache_dir: &Path, content: &str) {
        if std::env::var("JVMCACHE_VERBOSE").map(|v| v != "0").unwrap_or(false) {
            eprint!("[jvmcache] {}", content);
        }
        let log_path = cache_dir.join("activity.log");
        let _ = fs::create_dir_all(cache_dir);

        if let Ok(meta) = fs::metadata(&log_path) {
            if meta.len() > 20 * 1024 * 1024 {
                let old_log = cache_dir.join("activity.log.old");
                let _ = fs::rename(&log_path, &old_log);
            }
        }

        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = file.write_all(content.as_bytes());
        }
    }

    fn current_timestamp() -> String {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self::format_epoch_secs(secs)
    }

    pub fn format_epoch_secs(secs: u64) -> String {
        crate::time_utils::format_epoch_secs(secs)
    }

    fn format_duration(d: Duration) -> String {
        crate::time_utils::format_duration(d)
    }

    fn format_sample_artifacts(artifacts: &[Artifact]) -> String {
        if artifacts.is_empty() {
            return String::new();
        }
        let mut sample_items = Vec::new();
        for a in artifacts.iter().take(3) {
            sample_items.push(a.rel_path.to_string_lossy().into_owned());
        }
        let remaining = artifacts.len().saturating_sub(3);
        if remaining > 0 {
            format!(" (e.g. {}, +{} more)", sample_items.join(", "), remaining)
        } else {
            format!(" ({})", sample_items.join(", "))
        }
    }
}
