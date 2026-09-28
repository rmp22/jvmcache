use crate::constants::{
    DAEMON_CONNECT_INTERVAL_MS, DAEMON_CONNECT_RETRIES, DAEMON_MAIN_CLASS, FILE_DAEMON_JAR,
    FILE_DAEMON_SOCK, FLAG_CP, FLAG_SOCKET,
};
use crate::daemon::client::DaemonClient;
use crate::domain::JvmCacheError;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const EMBEDDED_DAEMON_JAR: &[u8] = include_bytes!("../../daemon-jvm/jvmcache-daemon.jar");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub enabled: bool,
    pub auto_spawn: bool,
    pub socket_path: PathBuf,
    pub daemon_jar_path: PathBuf,
}

impl DaemonConfig {
    pub fn new(cache_root: &Path) -> Self {
        let socket_path = cache_root.join(FILE_DAEMON_SOCK);
        let daemon_jar_path = cache_root.join(FILE_DAEMON_JAR);
        Self {
            enabled: true,
            auto_spawn: true,
            socket_path,
            daemon_jar_path,
        }
    }
}

pub struct DaemonLauncher;

impl DaemonLauncher {
    pub fn ensure_daemon_running(config: &DaemonConfig) -> Result<DaemonClient, JvmCacheError> {
        let jar_updated = Self::unpack_daemon_jar(&config.daemon_jar_path)?;
        let client = DaemonClient::new(&config.socket_path);

        if !jar_updated && client.is_alive() {
            return Ok(client);
        }

        if jar_updated {
            let _ = fs::remove_file(&config.socket_path);
        }

        if !config.auto_spawn {
            return Err(JvmCacheError::Execution(
                "jvmcache daemon is not running and auto_spawn is disabled".to_string(),
            ));
        }

        Self::spawn_daemon_process(config)?;

        for _ in 0..DAEMON_CONNECT_RETRIES {
            if client.is_alive() {
                return Ok(client);
            }
            sleep(Duration::from_millis(DAEMON_CONNECT_INTERVAL_MS));
        }

        Err(JvmCacheError::Execution(
            "Timed out waiting for jvmcache daemon socket".to_string(),
        ))
    }

    fn unpack_daemon_jar(target_path: &Path) -> Result<bool, JvmCacheError> {
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let needs_write = match fs::read(target_path) {
            Ok(content) => content != EMBEDDED_DAEMON_JAR,
            Err(_) => true,
        };
        if needs_write {
            fs::write(target_path, EMBEDDED_DAEMON_JAR)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn spawn_daemon_process(config: &DaemonConfig) -> Result<(), JvmCacheError> {
        let aosp_root = crate::args::find_aosp_root();

        let java_bin = aosp_root
            .as_ref()
            .map(|r| r.join("prebuilts/jdk/jdk21/linux-x86/bin/java"))
            .filter(|p| p.is_file())
            .or_else(|| {
                std::env::var("JAVA_HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join("bin").join("java"))
            })
            .unwrap_or_else(|| PathBuf::from("java"));

        let mut cp_parts = vec![config.daemon_jar_path.display().to_string()];

        let kotlin_home = aosp_root
            .as_ref()
            .map(|r| r.join("external/kotlinc"))
            .filter(|p| p.is_dir())
            .or_else(|| {
                std::env::var("KOTLIN_HOME")
                    .ok()
                    .map(PathBuf::from)
            })
            .or_else(|| {
                let sdkman_path = PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join(".sdkman/candidates/kotlin/current");
                if sdkman_path.exists() {
                    Some(sdkman_path)
                } else {
                    None
                }
            });

        if let Some(kh) = kotlin_home {
            let compiler_jar = kh.join("lib/kotlin-compiler.jar");
            let stdlib_jar = kh.join("lib/kotlin-stdlib.jar");
            if compiler_jar.exists() {
                cp_parts.push(compiler_jar.display().to_string());
            }
            if stdlib_jar.exists() {
                cp_parts.push(stdlib_jar.display().to_string());
            }
        }

        if let Some(ref aosp) = aosp_root {
            let r8_jar = aosp.join("prebuilts/r8/r8.jar");
            if r8_jar.is_file() {
                cp_parts.push(r8_jar.display().to_string());
            }
        }

        let full_cp = cp_parts.join(":");

        let mut cmd = Command::new(&java_bin);
        if let Some(ref aosp) = aosp_root {
            cmd.current_dir(aosp);
        }

        cmd.arg("--add-exports=jdk.compiler/com.sun.tools.javac.file=ALL-UNNAMED")
            .arg("--add-exports=jdk.compiler/com.sun.tools.javac.tree=ALL-UNNAMED")
            .arg("--add-exports=jdk.compiler/com.sun.tools.javac.main=ALL-UNNAMED")
            .arg("--add-opens=java.base/sun.net.www.protocol.jar=ALL-UNNAMED")
            .arg("--add-opens=java.base/java.util=ALL-UNNAMED")
            .arg("-Xmx4096m")
            .arg("-Xms512m")
            .arg(FLAG_CP)
            .arg(&full_cp)
            .arg(DAEMON_MAIN_CLASS)
            .arg(FLAG_SOCKET)
            .arg(&config.socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(())
    }
}
