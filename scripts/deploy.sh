#!/usr/bin/env bash
#
# Copyright (C) 2026 jvmcache contributors
# Licensed under the Apache License, Version 2.0
#
# Atomic Rebuild and Redeployment Wrapper for jvmcache
#

set -euo pipefail

SOURCE="${BASH_SOURCE[0]}"
while [ -L "$SOURCE" ]; do
    DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"
    SOURCE="$(readlink "$SOURCE")"
    [[ $SOURCE != /* ]] && SOURCE="$DIR/$SOURCE"
done
SCRIPT_DIR="$(cd -P "$(dirname "$SOURCE")" && pwd)"
PROJECT_ROOT="$(cd -P "${SCRIPT_DIR}/.." && pwd)"
if [ ! -f "${PROJECT_ROOT}/Cargo.toml" ] && [ -f "${SCRIPT_DIR}/Cargo.toml" ]; then
    PROJECT_ROOT="${SCRIPT_DIR}"
fi

BUILD_MODE="release"
RUN_TESTS=false
RUN_CLEAN=false
INSTALL_DIR=""

function print_usage() {
    cat <<EOF
Usage: $(basename "$0") [options]

Atomic rebuild and redeployment wrapper for jvmcache.

Options:
  -r, --release          Build in release mode with optimizations (default)
  -d, --debug            Build in debug mode (faster compile, unoptimized)
  -t, --test             Run test suite before deploying
  -c, --clean            Run cargo clean before rebuilding
  -i, --install [PATH]   Deploy binary and symlinks to directory (default: ~/.local/bin)
  -h, --help             Show this help message

Examples:
  $(basename "$0")                      # Rebuild release binary and refresh bin/
  $(basename "$0") --test               # Rebuild, run tests, and redeploy
  $(basename "$0") --install            # Rebuild and install into ~/.local/bin
  $(basename "$0") -i /usr/local/bin    # Rebuild and install into custom prefix
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -r|--release)
            BUILD_MODE="release"
            shift
            ;;
        -d|--debug)
            BUILD_MODE="debug"
            shift
            ;;
        -t|--test)
            RUN_TESTS=true
            shift
            ;;
        -c|--clean)
            RUN_CLEAN=true
            shift
            ;;
        -i|--install)
            if [[ $# -gt 1 && ! "$2" =~ ^- ]]; then
                INSTALL_DIR="$2"
                shift 2
            else
                INSTALL_DIR="${HOME}/.local/bin"
                shift
            fi
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        *)
            echo "Error: Unknown argument '$1'" >&2
            print_usage >&2
            exit 1
            ;;
    esac
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "Error: 'cargo' not found in PATH. Please install Rust toolchain." >&2
    exit 1
fi

echo "==> Building jvmcache (${BUILD_MODE})..."
cd "${PROJECT_ROOT}"

if [ "${RUN_CLEAN}" = true ]; then
    echo "  -> Cleaning target directory..."
    cargo clean
fi

CARGO_FLAGS=()
TARGET_DIR="${PROJECT_ROOT}/target/debug"
if [ "${BUILD_MODE}" = "release" ]; then
    CARGO_FLAGS+=("--release")
    TARGET_DIR="${PROJECT_ROOT}/target/release"
fi

cargo build "${CARGO_FLAGS[@]}"

COMPILED_BIN="${TARGET_DIR}/jvmcache"
if [ ! -f "${COMPILED_BIN}" ]; then
    echo "Error: Expected compiled binary at ${COMPILED_BIN} not found." >&2
    exit 1
fi

if [ "${RUN_TESTS}" = true ]; then
    echo "==> Running verification test suites..."
    python3 tests/integration_tests.py
    python3 tests/bytecode_edge_cases_test.py
    python3 tests/aosp_pipeline_test.py
    python3 tests/kapt_coverage_test.py
    python3 tests/cas_deduplication_test.py
    python3 tests/minute_change_falsification_test.py
    python3 tests/surgical_delta_compilation_test.py
    echo "==> All tests passed!"
fi

echo "==> Redeploying local project symlinks (bin/)..."
mkdir -p "${PROJECT_ROOT}/bin"

# Terminate legacy daemon workers so redeployed version initializes cleanly
pkill -f 'org.jvmcache.daemon.WorkerMain' 2>/dev/null || true
rm -f /tmp/jvmcache-*.sock 2>/dev/null || true

# Atomic symlink replacement to avoid ETXTBSY on active compilation jobs
for tool in javac kotlinc kapt d8 r8; do
    ln -sf "../target/${BUILD_MODE}/jvmcache" "${PROJECT_ROOT}/bin/${tool}.tmp"
    mv -f "${PROJECT_ROOT}/bin/${tool}.tmp" "${PROJECT_ROOT}/bin/${tool}"
done

# If an explicit install directory is requested (e.g. ~/.local/bin)
if [ -n "${INSTALL_DIR}" ]; then
    echo "==> Deploying to user install directory (${INSTALL_DIR})..."
    mkdir -p "${INSTALL_DIR}"

    # Atomic binary installation: copy to temp file, then atomic rename
    install -m 755 -T "${COMPILED_BIN}" "${INSTALL_DIR}/jvmcache.tmp"
    mv -f "${INSTALL_DIR}/jvmcache.tmp" "${INSTALL_DIR}/jvmcache"

    for tool in javac kotlinc kapt d8 r8; do
        ln -sf "jvmcache" "${INSTALL_DIR}/${tool}.tmp"
        mv -f "${INSTALL_DIR}/${tool}.tmp" "${INSTALL_DIR}/${tool}"
        echo "  Symlinked: ${INSTALL_DIR}/${tool} -> jvmcache"
    done

    echo "  Installed: ${INSTALL_DIR}/jvmcache"
fi

echo "==> Verifying deployed binary..."
"${COMPILED_BIN}" -k cache_dir >/dev/null

echo "==> Successfully rebuilt and redeployed jvmcache!"
echo "    Binary:      ${COMPILED_BIN}"
echo "    Local bin:   ${PROJECT_ROOT}/bin/{javac,kotlinc,kapt,d8,r8}"
if [ -n "${INSTALL_DIR}" ]; then
    echo "    Install dir: ${INSTALL_DIR}"
fi
"${COMPILED_BIN}" --show-config
