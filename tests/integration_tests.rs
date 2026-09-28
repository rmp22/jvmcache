use jvmcache::cds::{CdsConfig, CdsManager};
use jvmcache::daemon::protocol::{read_message, write_message, DaemonRequest, DaemonResponse};
use jvmcache::daemon::DaemonClient;
use jvmcache::domain::CompilerKind;
use jvmcache::flags::{FlagConfig, FlagOptimizer};
use jvmcache::run_compiler_cache;
use jvmcache::storage::CacheStorage;
use std::fs;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::thread;

fn make_temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("jvmcache_it_{}_{}", prefix, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn find_test_aosp_root() -> Option<PathBuf> {
    if let Some(top) = jvmcache::args::find_aosp_root() {
        return Some(top);
    }
    if let Ok(top) = std::env::var("ANDROID_BUILD_TOP") {
        let p = PathBuf::from(top);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

#[test]
fn test_flag_optimizer_integration() {
    let config = FlagConfig {
        kotlinc_backend_threads: Some(4),
        ..Default::default()
    };

    let kotlinc_raw = vec![
        "Foo.kt".to_string(),
        "-d".to_string(),
        "build/classes".to_string(),
    ];
    let kotlinc_opt = FlagOptimizer::optimize_args(CompilerKind::Kotlinc, &kotlinc_raw, &config);
    assert!(kotlinc_opt.contains(&"-Xbackend-threads=4".to_string()));

    let javac_raw = vec![
        "Foo.java".to_string(),
        "-d".to_string(),
        "build/classes".to_string(),
    ];
    let javac_opt = FlagOptimizer::optimize_args(CompilerKind::Javac, &javac_raw, &config);
    assert!(javac_opt.contains(&"-XDcompilePolicy=simple".to_string()));
}

#[test]
fn test_cds_manager_integration() {
    let tmp = make_temp_dir("cds");
    let mut config = CdsConfig::new(&tmp);
    config.tier1_c1_jit = true;
    config.parallel_gc = true;

    assert_eq!(
        CdsManager::archive_path(&config.archive_dir, CompilerKind::Kotlinc),
        tmp.join("cds").join("kotlinc.jsa")
    );
    assert_eq!(
        CdsManager::archive_path(&config.archive_dir, CompilerKind::Javac),
        tmp.join("cds").join("javac.jsa")
    );

    let flags = CdsManager::get_jvm_tuning_args(CompilerKind::Kotlinc, &config);
    assert!(flags.contains(&"-XX:TieredStopAtLevel=1".to_string()));
    assert!(flags.contains(&"-XX:ReservedCodeCacheSize=512m".to_string()));
    assert!(flags.contains(&"-XX:+UseParallelGC".to_string()));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_daemon_uds_client_server_integration() {
    let tmp = make_temp_dir("daemon_uds");
    let sock_path = tmp.join("test_daemon.sock");

    let server_sock = sock_path.clone();
    let server_handle = thread::spawn(move || {
        let listener = UnixListener::bind(&server_sock).unwrap();
        while let Ok((mut stream, _)) = listener.accept() {
            let req_bytes = match read_message(&mut stream) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let req: DaemonRequest = serde_json::from_slice(&req_bytes).unwrap();
            assert_eq!(req.compiler, "javac");
            assert_eq!(req.args, vec!["Test.java"]);

            let resp = DaemonResponse {
                exit_code: 0,
                stdout: "Mock daemon compiled successfully".to_string(),
                stderr: String::new(),
            };
            let resp_bytes = serde_json::to_vec(&resp).unwrap();
            write_message(&mut stream, &resp_bytes).unwrap();
            break;
        }
    });

    for _ in 0..50 {
        if sock_path.exists() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }

    let client = DaemonClient::new(&sock_path);
    assert!(client.is_alive());

    let req = DaemonRequest {
        compiler: "javac".to_string(),
        working_dir: tmp.to_string_lossy().to_string(),
        args: vec!["Test.java".to_string()],
    };

    let resp = client.send_request(&req).expect("Failed to send request");
    assert_eq!(resp.exit_code, 0);
    assert_eq!(resp.stdout, "Mock daemon compiled successfully");

    server_handle.join().unwrap();
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_multiplex_concurrent_daemon_requests() {
    let tmp = make_temp_dir("daemon_multiplex");
    let sock_path = tmp.join("multiplex_daemon.sock");

    let server_sock = sock_path.clone();
    let server_handle = thread::spawn(move || {
        let listener = UnixListener::bind(&server_sock).unwrap();
        let mut handled = 0;
        let mut threads = Vec::new();

        while handled < 5 {
            if let Ok((mut stream, _)) = listener.accept() {
                let t = thread::spawn(move || {
                    let req_bytes = match read_message(&mut stream) {
                        Ok(b) => b,
                        Err(_) => return,
                    };
                    let req: DaemonRequest = serde_json::from_slice(&req_bytes).unwrap();
                    let resp = DaemonResponse {
                        exit_code: 0,
                        stdout: format!("Compiled {}", req.args[0]),
                        stderr: String::new(),
                    };
                    let resp_bytes = serde_json::to_vec(&resp).unwrap();
                    let _ = write_message(&mut stream, &resp_bytes);
                });
                threads.push(t);
                handled += 1;
            }
        }

        for t in threads {
            let _ = t.join();
        }
    });

    for _ in 0..50 {
        if sock_path.exists() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }

    let mut client_handles = Vec::new();
    for i in 0..5 {
        let sock = sock_path.clone();
        let target_file = format!("Module{}.java", i);
        client_handles.push(thread::spawn(move || {
            let client = DaemonClient::new(&sock);
            let req = DaemonRequest {
                compiler: "javac".to_string(),
                working_dir: "/tmp".to_string(),
                args: vec![target_file.clone()],
            };
            let resp = client.send_request(&req).unwrap();
            assert_eq!(resp.exit_code, 0);
            assert_eq!(resp.stdout, format!("Compiled {}", target_file));
        }));
    }

    for h in client_handles {
        h.join().unwrap();
    }

    server_handle.join().unwrap();
    let _ = fs::remove_dir_all(&tmp);
}

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_end_to_end_cache_miss_and_hit_with_real_javac() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = make_temp_dir("e2e_javac");
    let cache_dir = tmp.join("cache");
    let src_dir = tmp.join("src");
    let out_dir = tmp.join("out");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    let java_file = src_dir.join("HelloJvmCache.java");
    fs::write(
        &java_file,
        b"public class HelloJvmCache { public static void main(String[] args) {} }",
    )
    .unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "javac".to_string(),
        "-d".to_string(),
        out_dir.to_str().unwrap().to_string(),
        java_file.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First compilation failed");
    assert_eq!(exit1, 0);
    assert!(out_dir.join("HelloJvmCache.class").is_file());

    let storage = CacheStorage::new().expect("Failed to open storage");
    let stats1 = storage.load_stats();
    assert_eq!(stats1.misses, 1);
    assert_eq!(stats1.hits, 0);

    let exit2 = run_compiler_cache(&argv).expect("Second compilation failed");
    assert_eq!(exit2, 0);
    assert!(out_dir.join("HelloJvmCache.class").is_file());

    let stats2 = storage.load_stats();
    assert_eq!(stats2.misses, 1);
    assert_eq!(stats2.hits, 1);

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_end_to_end_cache_miss_and_hit_with_real_kotlinc() {
    let _guard = ENV_LOCK.lock().unwrap();
    if std::process::Command::new("kotlinc").arg("-version").output().is_err() {
        eprintln!("kotlinc not available in PATH, skipping kotlinc e2e test");
        return;
    }

    let tmp = make_temp_dir("e2e_kotlinc");
    let cache_dir = tmp.join("cache");
    let src_dir = tmp.join("src");
    let out_dir = tmp.join("out");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();

    let kt_file = src_dir.join("HelloKt.kt");
    fs::write(
        &kt_file,
        b"fun main() { println(\"jvmcache kotlinc test\") }",
    )
    .unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "kotlinc".to_string(),
        "-d".to_string(),
        out_dir.to_str().unwrap().to_string(),
        kt_file.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First kotlinc compilation failed");
    assert_eq!(exit1, 0);

    let storage = CacheStorage::new().expect("Failed to open storage");
    let stats1 = storage.load_stats();
    assert_eq!(stats1.misses, 1);
    assert_eq!(stats1.hits, 0);

    let exit2 = run_compiler_cache(&argv).expect("Second kotlinc compilation failed");
    assert_eq!(exit2, 0);

    let stats2 = storage.load_stats();
    assert_eq!(stats2.misses, 1);
    assert_eq!(stats2.hits, 1);

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_end_to_end_cache_miss_and_hit_with_real_d8() {
    let _guard = ENV_LOCK.lock().unwrap();
    let aosp = find_test_aosp_root();
    let d8_path = aosp
        .as_ref()
        .map(|r| r.join("out/host/linux-x86/bin/d8"))
        .filter(|p| p.is_file())
        .or_else(|| {
            aosp.as_ref()
                .map(|r| r.join("prebuilts/r8/d8"))
                .filter(|p| p.is_file())
        })
        .or_else(|| std::env::var("JVMCACHE_D8").ok().map(PathBuf::from));

    let d8_path = match d8_path {
        Some(p) => p,
        None => {
            eprintln!("d8 binary not found, skipping d8 e2e test");
            return;
        }
    };

    let tmp = make_temp_dir("e2e_d8");
    let cache_dir = tmp.join("cache");
    let src_dir = tmp.join("src");
    let classes_dir = tmp.join("classes");
    let dex_out = tmp.join("dex_out");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();
    fs::create_dir_all(&dex_out).unwrap();

    let java_file = src_dir.join("TestDex.java");
    fs::write(&java_file, b"public class TestDex { public static void main(String[] args) {} }").unwrap();

    let javac_status = std::process::Command::new("javac")
        .arg("-d")
        .arg(&classes_dir)
        .arg(&java_file)
        .status()
        .expect("javac failed to compile TestDex");
    assert!(javac_status.success());
    let class_file = classes_dir.join("TestDex.class");
    assert!(class_file.is_file());

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    let prev_d8 = std::env::var("JVMCACHE_D8").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
        std::env::set_var("JVMCACHE_D8", d8_path.to_str().unwrap());
    }

    let argv = vec![
        "d8".to_string(),
        "--output".to_string(),
        dex_out.to_str().unwrap().to_string(),
        class_file.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First d8 compilation failed");
    assert_eq!(exit1, 0);
    assert!(dex_out.join("classes.dex").is_file());

    let storage = CacheStorage::new().expect("Failed to open storage");
    let stats1 = storage.load_stats();
    assert_eq!(stats1.misses, 1);
    assert_eq!(stats1.hits, 0);

    let exit2 = run_compiler_cache(&argv).expect("Second d8 compilation failed");
    assert_eq!(exit2, 0);
    assert!(dex_out.join("classes.dex").is_file());

    let stats2 = storage.load_stats();
    assert_eq!(stats2.misses, 1);
    assert_eq!(stats2.hits, 1);

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }
    if let Some(d8) = prev_d8 {
        unsafe { std::env::set_var("JVMCACHE_D8", d8); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_D8"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_kapt_stubs_full_compilation_and_cache() {
    let _guard = ENV_LOCK.lock().unwrap();
    if std::process::Command::new("kotlinc").arg("-version").output().is_err() {
        eprintln!("kotlinc not available in PATH, skipping kapt test");
        return;
    }

    let tmp = make_temp_dir("kapt_cache");
    let cache_dir = tmp.join("cache");
    let src_dir = tmp.join("src");
    let stubs_dir = tmp.join("stubs");
    let classes_dir = tmp.join("classes");

    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&stubs_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();

    let f1 = src_dir.join("A.kt");
    let f2 = src_dir.join("B.kt");
    fs::write(&f1, b"package test\nclass A").unwrap();
    fs::write(&f2, b"package test\nclass B").unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "kapt".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        format!("-P=plugin:org.jetbrains.kotlin.kapt3:stubs={}", stubs_dir.to_str().unwrap()),
        "-P=plugin:org.jetbrains.kotlin.kapt3:aptMode=stubs".to_string(),
        f1.to_str().unwrap().to_string(),
        f2.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First kapt compilation failed");
    assert_eq!(exit1, 0);

    let storage = CacheStorage::new().expect("Failed to open storage");
    let stats1 = storage.load_stats();
    assert_eq!(stats1.misses, 1);
    assert_eq!(stats1.hits, 0);

    let exit2 = run_compiler_cache(&argv).expect("Second kapt compilation failed");
    assert_eq!(exit2, 0);

    let stats2 = storage.load_stats();
    assert_eq!(stats2.misses, 1);
    assert_eq!(stats2.hits, 1);

    fs::write(&f1, b"package com.test\nclass Kapt1 { val v = 999 }\n").unwrap();
    let exit3 = run_compiler_cache(&argv).expect("Third kapt delta compilation failed");
    assert_eq!(exit3, 0);

    let log_path = cache_dir.join("activity.log");
    let log_content = fs::read_to_string(&log_path).expect("Failed to read activity log");
    assert!(log_content.contains("[DELTA]"));
    assert!(log_content.contains("Kapt"));

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_end_to_end_delta_compilation_with_javac() {
    let _lock = ENV_LOCK.lock().unwrap();

    let tmp = make_temp_dir("delta_javac");
    let src_dir = tmp.join("src");
    let classes_dir = tmp.join("classes");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();

    let f1 = src_dir.join("ServiceA.java");
    let f2 = src_dir.join("ServiceB.java");
    fs::write(&f1, b"public class ServiceA { public static String get() { return \"A1\"; } }").unwrap();
    fs::write(&f2, b"public class ServiceB { public static String get() { return \"B1\"; } }").unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "javac".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        f1.to_str().unwrap().to_string(),
        f2.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First compile failed");
    assert_eq!(exit1, 0);
    assert!(classes_dir.join("ServiceA.class").is_file());
    assert!(classes_dir.join("ServiceB.class").is_file());

    let storage = CacheStorage::new().expect("Failed to open storage");
    let baseline_files: Vec<_> = fs::read_dir(storage.baselines_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(baseline_files.len(), 1);

    fs::write(&f1, b"public class ServiceA { public static String get() { return \"A2_MODIFIED\"; } }").unwrap();

    let exit2 = run_compiler_cache(&argv).expect("Delta compile failed");
    assert_eq!(exit2, 0);
    assert!(classes_dir.join("ServiceA.class").is_file());
    assert!(classes_dir.join("ServiceB.class").is_file());

    let log_path = cache_dir.join("activity.log");
    let log_content = fs::read_to_string(&log_path).expect("Failed to read activity log");
    assert!(log_content.contains("[DELTA]"));
    assert!(log_content.contains("modified=1/2"));

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_end_to_end_delta_compilation_with_kotlinc() {
    let _lock = ENV_LOCK.lock().unwrap();

    let tmp = make_temp_dir("delta_kotlinc");
    let src_dir = tmp.join("src");
    let classes_dir = tmp.join("classes");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();

    let f1 = src_dir.join("ModelA.kt");
    let f2 = src_dir.join("ModelB.kt");
    fs::write(&f1, b"class ModelA { val v = 1 }").unwrap();
    fs::write(&f2, b"class ModelB { val v = 2 }").unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "kotlinc".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        f1.to_str().unwrap().to_string(),
        f2.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First kotlinc compile failed");
    assert_eq!(exit1, 0);
    assert!(classes_dir.join("ModelA.class").is_file());
    assert!(classes_dir.join("ModelB.class").is_file());

    fs::write(&f1, b"class ModelA { val v = 100 }").unwrap();

    let exit2 = run_compiler_cache(&argv).expect("Delta kotlinc compile failed");
    assert_eq!(exit2, 0);
    assert!(classes_dir.join("ModelA.class").is_file());
    assert!(classes_dir.join("ModelB.class").is_file());

    let log_path = cache_dir.join("activity.log");
    let log_content = fs::read_to_string(&log_path).expect("Failed to read activity log");
    assert!(log_content.contains("[DELTA]"));
    assert!(log_content.contains("modified=1/2"));

    if let Some(d) = prev_cache_dir {
        unsafe { std::env::set_var("JVMCACHE_DIR", d); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DIR"); }
    }
    if let Some(dm) = prev_daemon {
        unsafe { std::env::set_var("JVMCACHE_DAEMON", dm); }
    } else {
        unsafe { std::env::remove_var("JVMCACHE_DAEMON"); }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_abi_change_triggers_full_recompile_and_prevents_nosuchmethoderror() {
    let _lock = ENV_LOCK.lock().unwrap();

    let tmp = make_temp_dir("abi_safety");
    let src_dir = tmp.join("src");
    let classes_dir = tmp.join("classes");
    let header_dir = tmp.join("header_classes");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();
    fs::create_dir_all(&header_dir).unwrap();

    let f_callee = src_dir.join("Callee.kt");
    let f_caller = src_dir.join("Caller.kt");
    fs::write(&f_callee, b"fun compute(x: Int): Int = x + 1\n").unwrap();
    fs::write(&f_caller, b"fun runTest(): Int = compute(42)\n").unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    let prev_kotlinc = std::env::var("JVMCACHE_KOTLINC").ok();
    let prev_strict_abi = std::env::var("JVMCACHE_STRICT_ABI").ok();

    let aosp = find_test_aosp_root();
    let target_kotlinc = aosp
        .as_ref()
        .map(|r| r.join("external/kotlinc/bin/kotlinc"))
        .filter(|p| p.is_file())
        .or_else(|| std::env::var("JVMCACHE_KOTLINC").ok().map(PathBuf::from));
    let abi_jar = aosp
        .as_ref()
        .map(|r| r.join("external/kotlinc/lib/jvm-abi-gen.jar"))
        .filter(|p| p.is_file());

    let (target_kotlinc, abi_jar) = match (target_kotlinc, abi_jar) {
        (Some(k), Some(a)) => (k, a),
        _ => {
            eprintln!("AOSP kotlinc or jvm-abi-gen.jar not found, skipping test");
            return;
        }
    };

    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
        std::env::set_var("JVMCACHE_KOTLINC", target_kotlinc.to_str().unwrap());
        std::env::set_var("JVMCACHE_STRICT_ABI", "1");
    }

    let argv = vec![
        "kotlinc".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        format!("-Xplugin={}", abi_jar.display()),
        format!(
            "-P=plugin:org.jetbrains.kotlin.jvm.abi:outputDir={}",
            header_dir.to_str().unwrap()
        ),
        f_callee.to_str().unwrap().to_string(),
        f_caller.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv).expect("First compile failed");
    assert_eq!(exit1, 0);
    assert!(classes_dir.join("CalleeKt.class").is_file());
    assert!(classes_dir.join("CallerKt.class").is_file());
    assert!(header_dir.join("CalleeKt.class").is_file());

    fs::write(&f_callee, b"fun compute(x: Int): Int = x + 100\n").unwrap();
    let exit2 = run_compiler_cache(&argv).expect("Delta compile failed");
    assert_eq!(exit2, 0);

    let log_path = cache_dir.join("activity.log");
    let log1 = fs::read_to_string(&log_path).expect("Read log failed");
    assert!(log1.contains("[DELTA]"));

    let _ = fs::write(&log_path, "");

    fs::write(
        &f_callee,
        b"fun compute(x: Int, multiplier: Int = 1): Int = (x + 100) * multiplier\n",
    )
    .unwrap();

    let exit3 = run_compiler_cache(&argv).expect("ABI change compile failed");
    assert_eq!(exit3, 0);

    let log2 = fs::read_to_string(&log_path).expect("Read log failed");
    assert!(log2.contains("[MISS]"));
    assert!(!log2.contains("[DELTA]"));

    let caller_bytes = fs::read(classes_dir.join("CallerKt.class")).unwrap();
    assert!(caller_bytes.windows(15).any(|w| w == b"compute$default"));

    if let Some(d) = prev_cache_dir {
        unsafe {
            std::env::set_var("JVMCACHE_DIR", d);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DIR");
        }
    }
    if let Some(dm) = prev_daemon {
        unsafe {
            std::env::set_var("JVMCACHE_DAEMON", dm);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DAEMON");
        }
    }
    if let Some(k) = prev_kotlinc {
        unsafe {
            std::env::set_var("JVMCACHE_KOTLINC", k);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_KOTLINC");
        }
    }
    if let Some(s) = prev_strict_abi {
        unsafe {
            std::env::set_var("JVMCACHE_STRICT_ABI", s);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_STRICT_ABI");
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_deleted_source_file_recursively_prunes_classes() {
    let _lock = ENV_LOCK.lock().unwrap();

    let tmp = make_temp_dir("del_prune");
    let src_dir = tmp.join("src/com/foobar/nested");
    let classes_dir = tmp.join("classes");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&classes_dir).unwrap();

    let f_foo = src_dir.join("Foo.kt");
    let f_bar = src_dir.join("Bar.kt");
    fs::write(&f_foo, b"package com.foobar.nested\nclass Foo\n").unwrap();
    fs::write(&f_bar, b"package com.foobar.nested\nclass Bar\n").unwrap();

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv1 = vec![
        "kotlinc".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        f_foo.to_str().unwrap().to_string(),
        f_bar.to_str().unwrap().to_string(),
    ];

    let exit1 = run_compiler_cache(&argv1).expect("First compile failed");
    assert_eq!(exit1, 0);
    assert!(classes_dir.join("com/foobar/nested/Foo.class").is_file());
    assert!(classes_dir.join("com/foobar/nested/Bar.class").is_file());

    let _ = fs::remove_file(&f_foo);

    let argv2 = vec![
        "kotlinc".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        f_bar.to_str().unwrap().to_string(),
    ];

    let exit2 = run_compiler_cache(&argv2).expect("Second compile failed");
    assert_eq!(exit2, 0);

    assert!(
        !classes_dir.join("com/foobar/nested/Foo.class").exists(),
        "Deleted class Foo.class must be recursively pruned from disk"
    );
    assert!(
        classes_dir.join("com/foobar/nested/Bar.class").is_file(),
        "Remaining class Bar.class must remain present"
    );

    if let Some(d) = prev_cache_dir {
        unsafe {
            std::env::set_var("JVMCACHE_DIR", d);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DIR");
        }
    }
    if let Some(dm) = prev_daemon {
        unsafe {
            std::env::set_var("JVMCACHE_DAEMON", dm);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DAEMON");
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_missing_source_file_graceful_fallback() {
    let _lock = ENV_LOCK.lock().unwrap();

    let tmp = make_temp_dir("missing_fallback");
    let classes_dir = tmp.join("classes");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&classes_dir).unwrap();

    let missing_file = tmp.join("NonExistent.java");

    let prev_cache_dir = std::env::var("JVMCACHE_DIR").ok();
    let prev_daemon = std::env::var("JVMCACHE_DAEMON").ok();
    unsafe {
        std::env::set_var("JVMCACHE_DIR", cache_dir.to_str().unwrap());
        std::env::set_var("JVMCACHE_DAEMON", "0");
    }

    let argv = vec![
        "javac".to_string(),
        "-d".to_string(),
        classes_dir.to_str().unwrap().to_string(),
        missing_file.to_str().unwrap().to_string(),
    ];

    let result = run_compiler_cache(&argv);
    assert!(result.is_ok(), "Must not panic or abort with I/O error");
    let exit_code = result.unwrap();
    assert_ne!(exit_code, 0, "Underlying javac must fail on missing file");

    if let Some(d) = prev_cache_dir {
        unsafe {
            std::env::set_var("JVMCACHE_DIR", d);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DIR");
        }
    }
    if let Some(dm) = prev_daemon {
        unsafe {
            std::env::set_var("JVMCACHE_DAEMON", dm);
        }
    } else {
        unsafe {
            std::env::remove_var("JVMCACHE_DAEMON");
        }
    }

    let _ = fs::remove_dir_all(&tmp);
}
