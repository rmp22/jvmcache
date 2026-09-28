# jvmcache

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![JVM](https://img.shields.io/badge/jvm-Java%208--25%20%7C%20Kotlin%202.x-brightgreen.svg)](https://openjdk.org/)

**jvmcache** is a fast, transparent drop-in compiler cache for `javac`, `kotlinc`, `kapt`, `d8`, and `r8`, modeled after `ccache` and `sccache`.

It intercepts compiler invocations, hashes input sources, semantic compiler flags, and classpath dependencies, and restores compiled bytecode artifacts from content-addressable storage (CAS) in **1 to 10 milliseconds**.

---

## Performance Highlights

Evaluated against real-world, widely used open-source libraries:

| Project | Compiler | Sources | Artifacts | Cold Compile | jvmcache Hit | Speedup |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **Ajalt Clikt** | `kotlinc` 2.4 | 61 `.kt` | 241 `.class`/`.module` | 23,117.5 ms | **30.5 ms** | **756.9x** |
| **Apache Commons Lang** | `javac` 21 | 264 `.java` | 394 `.class` | 7,478.9 ms | **63.1 ms** | **118.5x** |
| **Apache Commons IO** | `javac` 21 | 277 `.java` | 372 `.class` | 3,917.2 ms | **104.7 ms** | **37.4x** |
| **Square JavaPoet** | `javac` 21 | 17 `.java` | 35 `.class` | 1,380.1 ms | **6.2 ms** | **221.8x** |
| **AOSP Soong Javac** | `javac` 21 | multi-file | `$outDir` + `$annoDir` | 546.3 ms | **2.6 ms** | **208.7x** |
| **AOSP Soong Kotlinc** | `kotlinc` 21 | multi-file | classes + headers | 4,738.1 ms | **2.8 ms** | **1669.1x** |

*Bytecode Fidelity: 100% bit-for-bit identical SHA-256 tree matches verified across all runs.*

---

## Core Capabilities

- **Broad Compiler Support:** Transparent drop-in support for `javac`, `kotlinc`, `kapt` (Kotlin Annotation Processing), `d8` (Android Dexer), and `r8` (Android Optimizer / Shrinker).
- **Persistent In-Process JVM Daemon:** Background worker daemon communicating over Unix Domain Sockets (`StandardProtocolFamily.UNIX`) with thread pooling, completely avoiding repeated JVM bootstrap startup penalties.
- **Surgical Delta Compilation:** When only a subset of files change in a large target, `jvmcache` compiles only the modified files and atomically merges them with baseline cached class outputs.
- **Two-Level CAS Storage & Inode Deduplication:** Centralized Content-Addressable Storage (CAS) with SHA-256 blob deduplication. Identical `.class` files across different modules or targets share the exact same underlying disk inode via hardlinks.
- **AppCDS Class Data Sharing Acceleration:** Automatically generates and loads JVM Application Class Data Sharing archives (`cds.rs`) for sub-second startup when running standalone compiler processes.
- **Strict ABI Caching:** Optional ABI-based hashing (`JVMCACHE_STRICT_ABI=1` with `jvm-abi-gen`) that prevents rebuilding downstream consumers when internal implementation details change without affecting public API contracts.
- **Fail-Safe Passthrough:** If cache storage encounters any I/O errors, read-only permissions, or disk limits, `jvmcache` transparently falls back to direct compiler execution without failing the build.
- **Sandboxed Build & Container Parity:** Implements `jvmcache -k cache_dir`, matching `ccache`'s contract for container and sandbox bind mounting (e.g., AOSP `nsjail` and Docker).
- **Dynamic Portability:** Zero hardcoded machine paths. Automatically resolves project roots, standard toolchains, and AOSP hermetic prebuilts.
- **Bytecode Determinism Safety:** Preserves left-to-right classpath precedence, tracks anonymous inner class numbering, handles `--release` bytecode targets, and captures multi-directory outputs (`-d`, `-s`, `-h`).
- **Compiler Fingerprint Memoization:** Avoids the multi-second startup penalty of querying `kotlinc -version` via inode/mtime stat memoization.

---

## Quickstart

### 1. Build and Deploy
Use the automated atomic rebuild and deployment wrapper:
```bash
./deploy.sh            # Rebuild release binary and atomically refresh bin/
./deploy.sh --test     # Rebuild, run all verification test suites, and redeploy
./deploy.sh --install  # Rebuild and atomically install to ~/.local/bin
```

Or build manually with Cargo:
```bash
cargo build --release
```
The compiled native binary is located at `target/release/jvmcache`.

### 2. Enable via PATH Interception
Prepend `bin/` to `$PATH`, or deploy to `~/.local/bin`:
```bash
# Using project symlinks:
export PATH="/path/to/jvmcache/bin:$PATH"

# Or install to user directory:
./deploy.sh --install
export PATH="$HOME/.local/bin:$PATH"
```
Any build tool invoking `javac`, `kotlinc`, `kapt`, `d8`, or `r8` will now automatically route through `jvmcache`.

---

## Build System Integrations

### Android Open Source Project (AOSP / Soong)
`jvmcache` integrates directly with AOSP toolchain overrides:
```bash
export ALTERNATE_JAVAC=/path/to/jvmcache/bin/javac
export ALTERNATE_KOTLINC=/path/to/jvmcache/bin/kotlinc
export ALTERNATE_D8=/path/to/jvmcache/bin/d8
export ALTERNATE_R8=/path/to/jvmcache/bin/r8
```

### Apache Maven
In `pom.xml`, configure `maven-compiler-plugin`:
```xml
<plugin>
    <groupId>org.apache.maven.plugins</groupId>
    <artifactId>maven-compiler-plugin</artifactId>
    <version>3.13.0</version>
    <configuration>
        <fork>true</fork>
        <executable>/path/to/jvmcache/bin/javac</executable>
    </configuration>
</plugin>
```

### Gradle
In `build.gradle`:
```groovy
tasks.withType(JavaCompile).configureEach {
    options.fork = true
    options.forkOptions.executable = '/path/to/jvmcache/bin/javac'
}
```

---

## CLI Management & Inspection

```bash
# View cache statistics, hit rate, and storage summary
jvmcache --show-stats       # or -s

# View recent compilation and caching activity log
jvmcache --log              # or -l

# Tail the last N entries of the activity log (default: 25)
jvmcache --tail 50          # or -t 50

# Inspect cached artifacts/manifests matching a filter
jvmcache --objects          # or -o <filter>

# Clear the activity log
jvmcache --clear-log

# View active resolved configuration and detected compiler paths
jvmcache --show-config      # or -p

# Query specific configuration value (e.g. for build scripts)
jvmcache -k cache_dir
jvmcache -k max_size

# Clear all cached objects and reset statistics
jvmcache --clear            # or -C
```

---

## Configuration

`jvmcache` resolves configuration in the following precedence order:

1. **Environment Variables:**
   - `JVMCACHE_DIR`: Cache storage directory (default: `~/.cache/jvmcache`).
   - `JVMCACHE_CONFIG`: Custom configuration file path.
   - `JVMCACHE_MAXSIZE`: Maximum cache size in MB (default: `5120`).
   - `JVMCACHE_JAVAC`: Explicit override for the real `javac` binary.
   - `JVMCACHE_KOTLINC`: Explicit override for the real `kotlinc` binary.
   - `JVMCACHE_KAPT`: Explicit override for the real `kapt` binary.
   - `JVMCACHE_D8`: Explicit override for the real `d8` binary.
   - `JVMCACHE_R8`: Explicit override for the real `r8` binary.
   - `JVMCACHE_HARDLINK`: Enable hardlink artifact restoration (`1` or `0`, default: `1`).
   - `JVMCACHE_DAEMON`: Enable persistent JVM worker daemon (`1` or `0`, default: `1`).
   - `JVMCACHE_AUTO_SPAWN`: Automatically spawn daemon worker on demand (`1` or `0`, default: `1`).
   - `JVMCACHE_CDS`: Enable AppCDS shared archive acceleration (`1` or `0`, default: `1`).
   - `JVMCACHE_AUTO_FLAGS`: Enable automated compiler flag optimizations (`1` or `0`, default: `1`).
   - `JVMCACHE_STRICT_ABI`: Enforce strict ABI hashing when `jvm-abi-gen` is present (`1` or `0`, default: `0`).
   - `JVMCACHE_KOTLINC_THREADS`: Thread count for kotlinc parallel bytecode generation (default: logical CPU cores).
   - `JVMCACHE_VERBOSE`: Enable verbose debug logging (`1` or `0`, default: `0`).

2. **Project Configuration:** `.jvmcache.json` or `.jvmcache/config.json` in the current working directory or any parent directory.
3. **User Configuration:** `${XDG_CONFIG_HOME}/jvmcache/config.json` or `~/.config/jvmcache/config.json`.
4. **Defaults:** Portable user cache directory.

---

## Technical Architecture & Whitepaper

For an in-depth first-principles analysis comparing native C/C++ compilation (`ccache`) to JVM compilation models, bytecode determinism proofs, and full technical specifications, see [RESEARCH_AND_SPECIFICATION.md](RESEARCH_AND_SPECIFICATION.md).

---

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
