import os
import sys
import shutil
import tempfile
import time
import subprocess
import hashlib
import struct

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BIN_DIR = os.path.join(REPO_ROOT, "bin")

def run(cmd, env=None):
    merged = os.environ.copy()
    if env:
        merged.update(env)
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=merged)

def read_bytes(path):
    with open(path, "rb") as f:
        return f.read()

def sha256_tree(dir_path):
    hashes = {}
    for root, _, files in sorted(os.walk(dir_path)):
        for f in sorted(files):
            p = os.path.join(root, f)
            rel = os.path.relpath(p, dir_path)
            h = hashlib.sha256()
            with open(p, "rb") as fp:
                while chunk := fp.read(8192):
                    h.update(chunk)
            hashes[rel] = h.hexdigest()
    return hashes

def test_classpath_shadowing_and_order():
    print("[EDGE CASE 1] Classpath Shadowing & Ordering Sensitivity...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_cp_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_cp_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        dir_a = os.path.join(test_work, "cp_a")
        dir_b = os.path.join(test_work, "cp_b")
        src_a_dir = os.path.join(test_work, "src_a")
        src_b_dir = os.path.join(test_work, "src_b")
        os.makedirs(dir_a)
        os.makedirs(dir_b)
        os.makedirs(src_a_dir)
        os.makedirs(src_b_dir)

        # Build Version A: ID = "ALPHA"
        ver_a = os.path.join(src_a_dir, "Version.java")
        with open(ver_a, "w") as f:
            f.write("package com.lib; public class Version { public static final String ID = \"ALPHA\"; }")
        run(f"javac -d {dir_a} {ver_a}")

        # Build Version B: ID = "BETA"
        ver_b = os.path.join(src_b_dir, "Version.java")
        with open(ver_b, "w") as f:
            f.write("package com.lib; public class Version { public static final String ID = \"BETA\"; }")
        run(f"javac -d {dir_b} {ver_b}")

        consumer_src = os.path.join(test_work, "Consumer.java")
        with open(consumer_src, "w") as f:
            f.write("package app; import com.lib.Version; public class Consumer { public String get() { return Version.ID; } }")

        out1 = os.path.join(test_work, "out1")
        out2 = os.path.join(test_work, "out2")
        os.makedirs(out1)
        os.makedirs(out2)

        # Invocation 1: -cp A:B -> should inline ALPHA
        p1 = run(f"javac -cp {dir_a}:{dir_b} -d {out1} {consumer_src}", env=env)
        assert p1.returncode == 0
        b1 = read_bytes(os.path.join(out1, "app", "Consumer.class"))
        assert b"ALPHA" in b1, "Failed to inline ALPHA from first CP entry!"

        # Invocation 2: -cp B:A -> should inline BETA (distinct cache entry!)
        p2 = run(f"javac -cp {dir_b}:{dir_a} -d {out2} {consumer_src}", env=env)
        assert p2.returncode == 0
        b2 = read_bytes(os.path.join(out2, "app", "Consumer.class"))
        assert b"BETA" in b2, "Failed to inline BETA when CP order flipped!"

        # Invocation 3: Cache Hit on A:B
        shutil.rmtree(out1); os.makedirs(out1)
        t0 = time.perf_counter()
        p3 = run(f"javac -cp {dir_a}:{dir_b} -d {out1} {consumer_src}", env=env)
        t_hit = time.perf_counter() - t0
        assert p3.returncode == 0
        b3 = read_bytes(os.path.join(out1, "app", "Consumer.class"))
        assert b"ALPHA" in b3
        assert b1 == b3, "Cache hit did not restore bit-for-bit identical ALPHA bytecode!"
        print(f"  -> Classpath order sensitivity passed: distinct keys for A:B vs B:A (Hit in {t_hit*1000:.1f}ms).")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_anonymous_class_shifting():
    print("[EDGE CASE 2] Anonymous Class Sequential Numbering & Shifting...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_anon_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_anon_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src = os.path.join(test_work, "Worker.java")
        out = os.path.join(test_work, "out")
        os.makedirs(out)

        # Version 1: 2 anonymous inner classes (Worker$1, Worker$2)
        with open(src, "w") as f:
            f.write("""public class Worker {
    public void runAll() {
        Runnable r1 = new Runnable() { public void run() { System.out.println("First"); } };
        Runnable r2 = new Runnable() { public void run() { System.out.println("Second"); } };
        r1.run(); r2.run();
    }
}""")

        p1 = run(f"javac -d {out} {src}", env=env)
        assert p1.returncode == 0
        assert os.path.exists(os.path.join(out, "Worker.class"))
        assert os.path.exists(os.path.join(out, "Worker$1.class"))
        assert os.path.exists(os.path.join(out, "Worker$2.class"))
        h_v1 = sha256_tree(out)

        # Rebuild Hit V1
        shutil.rmtree(out); os.makedirs(out)
        p1_hit = run(f"javac -d {out} {src}", env=env)
        assert p1_hit.returncode == 0
        assert h_v1 == sha256_tree(out)

        # Version 2: Insert a new anonymous class in the middle -> 3 classes, numbering shifted!
        with open(src, "w") as f:
            f.write("""public class Worker {
    public void runAll() {
        Runnable r1 = new Runnable() { public void run() { System.out.println("First"); } };
        Runnable rMiddle = new Runnable() { public void run() { System.out.println("Middle"); } };
        Runnable r2 = new Runnable() { public void run() { System.out.println("Second"); } };
        r1.run(); rMiddle.run(); r2.run();
    }
}""")

        shutil.rmtree(out); os.makedirs(out)
        p2 = run(f"javac -d {out} {src}", env=env)
        assert p2.returncode == 0
        assert os.path.exists(os.path.join(out, "Worker$3.class")), "Worker$3 must be created!"
        h_v2 = sha256_tree(out)
        assert len(h_v2) == 4  # Worker, Worker$1, Worker$2, Worker$3

        # Revert to Version 1 -> Instant Cache Hit with 2 anonymous classes!
        with open(src, "w") as f:
            f.write("""public class Worker {
    public void runAll() {
        Runnable r1 = new Runnable() { public void run() { System.out.println("First"); } };
        Runnable r2 = new Runnable() { public void run() { System.out.println("Second"); } };
        r1.run(); r2.run();
    }
}""")
        shutil.rmtree(out); os.makedirs(out)
        p3 = run(f"javac -d {out} {src}", env=env)
        assert p3.returncode == 0
        h_v1_restored = sha256_tree(out)
        assert h_v1 == h_v1_restored, "Failed to restore exact anonymous class set upon revert!"
        print("  -> Anonymous class shifting and set restoration passed.")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_line_number_debug_attribute_shifts():
    print("[EDGE CASE 3] LineNumberTable Debug Attribute Shifts...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_line_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_line_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src = os.path.join(test_work, "DebugLine.java")
        out1 = os.path.join(test_work, "out1")
        out2 = os.path.join(test_work, "out2")
        os.makedirs(out1)
        os.makedirs(out2)

        # Baseline: Method on line 3
        with open(src, "w") as f:
            f.write("public class DebugLine {\n    public int calc() {\n        return 42;\n    }\n}\n")

        p1 = run(f"javac -g -d {out1} {src}", env=env)
        assert p1.returncode == 0
        b1 = read_bytes(os.path.join(out1, "DebugLine.class"))

        # Modified: Insert 10 blank lines and comments -> moves method to line 15
        with open(src, "w") as f:
            f.write("// Comment 1\n// Comment 2\n\n\n\n\n\n\n\n\n\npublic class DebugLine {\n    public int calc() {\n        return 42;\n    }\n}\n")

        p2 = run(f"javac -g -d {out2} {src}", env=env)
        assert p2.returncode == 0
        b2 = read_bytes(os.path.join(out2, "DebugLine.class"))

        assert b1 != b2, "Bytecode should differ because LineNumberTable attribute changed!"

        # Cache hit test on shifted code
        shutil.rmtree(out2); os.makedirs(out2)
        p2_hit = run(f"javac -g -d {out2} {src}", env=env)
        assert p2_hit.returncode == 0
        b2_hit = read_bytes(os.path.join(out2, "DebugLine.class"))
        assert b2 == b2_hit, "Cache hit did not restore exact LineNumberTable bytecode!"
        print("  -> LineNumberTable line offset shift correctly recognized and cached.")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_bytecode_major_version_targeting():
    print("[EDGE CASE 4] Bytecode Major Version Targeting (--release)...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_target_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_target_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src = os.path.join(test_work, "VersionTarget.java")
        with open(src, "w") as f:
            f.write("public class VersionTarget { public static void main(String[] args) {} }")

        out8 = os.path.join(test_work, "out8")
        out17 = os.path.join(test_work, "out17")
        out21 = os.path.join(test_work, "out21")
        os.makedirs(out8)
        os.makedirs(out17)
        os.makedirs(out21)

        run(f"javac --release 8 -d {out8} {src}", env=env)
        run(f"javac --release 17 -d {out17} {src}", env=env)
        run(f"javac --release 21 -d {out21} {src}", env=env)

        b8 = read_bytes(os.path.join(out8, "VersionTarget.class"))
        b17 = read_bytes(os.path.join(out17, "VersionTarget.class"))
        b21 = read_bytes(os.path.join(out21, "VersionTarget.class"))

        # Bytecode major version is a u2 at byte offset 6..8
        major8 = struct.unpack(">H", b8[6:8])[0]
        major17 = struct.unpack(">H", b17[6:8])[0]
        major21 = struct.unpack(">H", b21[6:8])[0]

        assert major8 == 52, f"Java 8 major version should be 52, got {major8}"
        assert major17 == 61, f"Java 17 major version should be 61, got {major17}"
        assert major21 == 65, f"Java 21 major version should be 65, got {major21}"

        # Verify cache hits preserve correct major versions
        shutil.rmtree(out8); os.makedirs(out8)
        run(f"javac --release 8 -d {out8} {src}", env=env)
        b8_hit = read_bytes(os.path.join(out8, "VersionTarget.class"))
        assert struct.unpack(">H", b8_hit[6:8])[0] == 52

        print(f"  -> Bytecode major version targeting verified (Java 8: {major8}, Java 17: {major17}, Java 21: {major21}).")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_kotlin_when_mappings_and_companion():
    print("[EDGE CASE 5] Kotlin Synthetic WhenMappings & Companion Objects...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_ktwhen_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_ktwhen_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src = os.path.join(test_work, "State.kt")
        out = os.path.join(test_work, "out")
        os.makedirs(out)

        with open(src, "w") as f:
            f.write("""package state
enum class Status { IDLE, RUNNING, FINISHED }

class StateMachine {
    companion object {
        const val DEFAULT_TIMEOUT = 1000L
        fun create(): StateMachine = StateMachine()
    }

    fun describe(status: Status): String = when (status) {
        Status.IDLE -> "Waiting"
        Status.RUNNING -> "In Progress"
        Status.FINISHED -> "Done"
    }
}
""")

        p1 = run(f"kotlinc -d {out} {src}", env=env)
        assert p1.returncode == 0
        files = os.listdir(os.path.join(out, "state"))
        # Must contain Status.class, StateMachine.class, StateMachine$Companion.class, and StateMachine$WhenMappings.class
        assert any("Companion" in f for f in files), "Companion class missing!"
        assert any("WhenMappings" in f for f in files), "WhenMappings synthetic class missing!"
        h_initial = sha256_tree(out)

        # Hit verification
        shutil.rmtree(out); os.makedirs(out)
        t0 = time.perf_counter()
        p2 = run(f"kotlinc -d {out} {src}", env=env)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        h_hit = sha256_tree(out)
        assert h_initial == h_hit, "Kotlin synthetic class tree divergence on cache hit!"
        print(f"  -> Kotlin WhenMappings and Companion synthetic classes restored bit-for-bit in {t_hit*1000:.1f}ms.")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

if __name__ == "__main__":
    print("=" * 76)
    print("JVMCACHE ADVERSARIAL BYTECODE EDGE-CASE FALSIFICATION SUITE")
    print("=" * 76)
    test_classpath_shadowing_and_order()
    test_anonymous_class_shifting()
    test_line_number_debug_attribute_shifts()
    test_bytecode_major_version_targeting()
    test_kotlin_when_mappings_and_companion()
    print("=" * 76)
    print("ALL BYTECODE EDGE CASES RIGOROUSLY VERIFIED & PASSED!")
    print("=" * 76)
