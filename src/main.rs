use jvmcache::config::JvmCacheConfig;
use jvmcache::domain::JvmCacheError;
use jvmcache::run_compiler_cache;
use jvmcache::storage::CacheStorage;
use jvmcache::telemetry::TelemetryLogger;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 {
        if (args[1] == "-k" || args[1] == "--get-config") && args.len() >= 3 {
            get_config_key(&args[2]);
            return;
        }
        match args[1].as_str() {
            "--show-stats" | "-s" => {
                show_stats();
                return;
            }
            "--show-config" | "-p" => {
                show_config();
                return;
            }
            "--clear" | "-C" => {
                clear_cache();
                return;
            }
            "--clear-log" => {
                clear_log();
                return;
            }
            "--log" | "-l" => {
                show_log(50);
                return;
            }
            "--tail" | "-t" => {
                let limit = args
                    .get(2)
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(25);
                show_log(limit);
                return;
            }
            "--objects" | "-o" => {
                let filter = args.get(2).map(|s| s.as_str());
                show_objects(filter, 20);
                return;
            }
            "--help" | "-h" if args.len() == 2 => {
                print_help();
                return;
            }
            _ => {}
        }
    }

    match run_compiler_cache(&args) {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("jvmcache: error: {}", err);
            match err {
                JvmCacheError::InvalidInvocation(_) => process::exit(2),
                _ => process::exit(1),
            }
        }
    }
}

fn show_stats() {
    match CacheStorage::new() {
        Ok(storage) => {
            let stats = storage.load_stats();
            let total = stats.hits + stats.misses;
            let hit_rate = if total > 0 {
                (stats.hits as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            println!("jvmcache statistics:");
            println!("  Cache hits:             {}", stats.hits);
            println!("  Cache misses:           {}", stats.misses);
            println!("  Cache hit rate:         {:.1}%", hit_rate);
            println!("  Direct passthrough:     {}", stats.direct_passthrough);
            println!(
                "  Stored artifact size:   {:.2} MB",
                stats.bytes_cached as f64 / (1024.0 * 1024.0)
            );
        }
        Err(e) => eprintln!("Failed to load cache: {}", e),
    }
}

fn show_log(limit: usize) {
    let cfg = JvmCacheConfig::load();
    let entries = TelemetryLogger::read_recent_entries(&cfg.cache_dir, limit);
    if entries.is_empty() {
        println!(
            "No activity logged yet in {}/activity.log",
            cfg.cache_dir.display()
        );
        return;
    }
    for line in entries {
        println!("{}", line);
    }
}

fn show_objects(filter: Option<&str>, limit: usize) {
    let cfg = JvmCacheConfig::load();
    let objects = TelemetryLogger::list_objects(&cfg.cache_dir, filter, limit);
    if objects.is_empty() {
        if let Some(f) = filter {
            println!(
                "No cached objects matching filter '{}' found in {}",
                f,
                cfg.cache_dir.display()
            );
        } else {
            println!("No cached objects found in {}", cfg.cache_dir.display());
        }
        return;
    }

    println!(
        "Cached compilation objects in {} (showing top {}):",
        cfg.cache_dir.display(),
        objects.len()
    );
    println!("{:-<100}", "");
    for obj in objects {
        let size_mb = obj.total_bytes as f64 / (1024.0 * 1024.0);
        let key_prefix = if obj.key.len() >= 16 {
            &obj.key[..16]
        } else {
            &obj.key
        };
        println!(
            "[{}] {:<7?} key={} artifacts={} ({:.2} MB)",
            obj.created_at, obj.compiler, key_prefix, obj.artifact_count, size_mb
        );
        for sample in obj.sample_artifacts {
            println!("    - {}", sample);
        }
        let remaining = obj.artifact_count.saturating_sub(4);
        if remaining > 0 {
            println!("    (+{} more classes/objects)", remaining);
        }
        println!();
    }
}

fn clear_log() {
    let cfg = JvmCacheConfig::load();
    match TelemetryLogger::clear_log(&cfg.cache_dir) {
        Ok(_) => println!("jvmcache: activity log cleared successfully."),
        Err(e) => eprintln!("jvmcache: failed to clear activity log: {}", e),
    }
}

fn get_config_key(key: &str) {
    let cfg = jvmcache::config::JvmCacheConfig::load();
    match key {
        "cache_dir" => println!("{}", cfg.cache_dir.display()),
        "max_size" => println!("{}", cfg.max_cache_size_mb),
        "hardlink" => println!("{}", cfg.hardlink_enabled),
        _ => eprintln!("unknown config key: {}", key),
    }
}

fn show_config() {
    jvmcache::config::JvmCacheConfig::load().print_summary();
}

fn clear_cache() {
    match CacheStorage::new() {
        Ok(storage) => match storage.clear() {
            Ok(_) => println!("jvmcache: cache cleared successfully."),
            Err(e) => eprintln!("jvmcache: failed to clear cache: {}", e),
        },
        Err(e) => eprintln!("Failed to initialize cache: {}", e),
    }
}

fn print_help() {
    println!("jvmcache - Fast compiler cache for javac, kotlinc, kapt, d8, and r8");
    println!();
    println!("USAGE:");
    println!("  jvmcache javac [javac-options] <sources...>");
    println!("  jvmcache kotlinc [kotlinc-options] <sources...>");
    println!("  jvmcache kapt [kapt-options] <sources...>");
    println!("  jvmcache d8 [d8-options] <inputs...>");
    println!("  jvmcache r8 [r8-options] <inputs...>");
    println!("  javac [javac-options] <sources...>   (when symlinked)");
    println!("  kotlinc [kotlinc-options] <sources...> (when symlinked)");
    println!("  kapt [kapt-options] <sources...>     (when symlinked)");
    println!("  d8 [d8-options] <inputs...>           (when symlinked)");
    println!("  r8 [r8-options] <inputs...>           (when symlinked)");
    println!();
    println!("MANAGEMENT OPTIONS:");
    println!("  -s, --show-stats       Show cache hit/miss statistics and cache size");
    println!("  -l, --log              Show recent compilation and caching activity log");
    println!("  -t, --tail [n]         Tail the last N entries of the activity log (default: 25)");
    println!("  -o, --objects [filter] List cached class/object manifests (with optional filter)");
    println!("      --clear-log        Clear the activity log");
    println!("  -p, --show-config      Show resolved configuration and file locations");
    println!("  -k, --get-config <key> Get specific configuration value (e.g. -k cache_dir)");
    println!("  -C, --clear            Clear all cached compilation artifacts");
    println!("  -h, --help             Show this help message");
    println!();
    println!("CONFIGURATION LOCATIONS:");
    println!("  Project config:     .jvmcache.json or .jvmcache/config.json");
    println!("  User config:        ~/.config/jvmcache/config.json");
    println!("  Cache directory:    ~/.cache/jvmcache/ (objects, stats, fingerprints)");
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("  JVMCACHE_DIR        Custom cache directory (default: ~/.cache/jvmcache)");
    println!("  JVMCACHE_CONFIG     Custom configuration JSON file");
    println!("  JVMCACHE_MAXSIZE    Max cache size in megabytes (default: 5120)");
    println!("  JVMCACHE_JAVAC      Path to real javac binary");
    println!("  JVMCACHE_KOTLINC    Path to real kotlinc binary");
    println!("  JVMCACHE_KAPT       Path to real kapt binary");
    println!("  JVMCACHE_D8         Path to real d8 binary");
    println!("  JVMCACHE_R8         Path to real r8 binary");
    println!("  JVMCACHE_HARDLINK   Enable/disable hardlink artifact restoration (1/0, default: 1)");
    println!("  JVMCACHE_DAEMON     Enable persistent JVM daemon worker (1/0, default: 1)");
    println!("  JVMCACHE_CDS        Enable AppCDS shared archive acceleration (1/0, default: 1)");
    println!("  JVMCACHE_AUTO_FLAGS Enable automated compiler flag optimizations (1/0, default: 1)");
    println!("  JVMCACHE_STRICT_ABI Enforce strict ABI hashing with jvm-abi-gen (1/0, default: 0)");
    println!("  JVMCACHE_KOTLINC_THREADS Parallel backend threads for kotlinc (default: logical cores)");
    println!("  JVMCACHE_VERBOSE    Enable verbose debug logging (1/0, default: 0)");
}
