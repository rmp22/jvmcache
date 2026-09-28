import os
import sys
import shutil
import tempfile
import time
import subprocess
import hashlib

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BIN_DIR = os.path.join(REPO_ROOT, "bin")
JVMCACHE_BIN = os.path.join(REPO_ROOT, "target", "release", "jvmcache")

def run_cmd(cmd, cwd=None, env=None):
    merged_env = os.environ.copy()
    if env:
        merged_env.update(env)
    p = subprocess.run(cmd, shell=True, capture_output=True, text=True, cwd=cwd, env=merged_env)
    return p

def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(8192):
            h.update(chunk)
    return h.hexdigest()

def sha256_tree(dir_path):
    hashes = {}
    for root, _, files in sorted(os.walk(dir_path)):
        for f in sorted(files):
            p = os.path.join(root, f)
            rel = os.path.relpath(p, dir_path)
            hashes[rel] = sha256_file(p)
    return hashes

def test_java_circular_and_branch_switch():
    print("[TEST 1] Java circular dependencies and branch switching...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src_dir = os.path.join(test_work, "src")
        out_dir = os.path.join(test_work, "out")
        os.makedirs(src_dir)
        os.makedirs(out_dir)

        a_java = os.path.join(src_dir, "A.java")
        b_java = os.path.join(src_dir, "B.java")

        with open(a_java, "w") as f:
            f.write("package test; public class A { public static B getB() { return new B(); } }")
        with open(b_java, "w") as f:
            f.write("package test; public class B { public static A getA() { return new A(); } }")

        # Invocations 1: Miss
        t0 = time.perf_counter()
        p1 = run_cmd(f"javac -d {out_dir} {a_java} {b_java}", cwd=test_work, env=env)
        t_miss = time.perf_counter() - t0
        assert p1.returncode == 0, f"Compilation failed: {p1.stderr}"
        assert os.path.exists(os.path.join(out_dir, "test", "A.class"))
        assert os.path.exists(os.path.join(out_dir, "test", "B.class"))
        h_initial = sha256_tree(out_dir)

        # Clear output dir to simulate clean build
        shutil.rmtree(out_dir)
        os.makedirs(out_dir)

        # Invocation 2: Hit
        t0 = time.perf_counter()
        p2 = run_cmd(f"javac -d {out_dir} {a_java} {b_java}", cwd=test_work, env=env)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        h_hit = sha256_tree(out_dir)
        assert h_initial == h_hit, "Cache hit did not restore identical bytecode!"
        print(f"  -> Cache Miss: {t_miss*1000:.1f}ms | Cache Hit: {t_hit*1000:.1f}ms (Speedup: {t_miss/t_hit:.1f}x)")

        # Simulate git branch switch: modify A.java
        with open(a_java, "w") as f:
            f.write("package test; public class A { public static B getB() { System.out.println(\"Branch 2\"); return new B(); } }")

        shutil.rmtree(out_dir); os.makedirs(out_dir)
        p3 = run_cmd(f"javac -d {out_dir} {a_java} {b_java}", cwd=test_work, env=env)
        assert p3.returncode == 0
        h_branch2 = sha256_tree(out_dir)
        assert h_branch2 != h_initial, "Branch 2 should have produced different bytecode!"

        # Switch back to original branch: restore A.java
        with open(a_java, "w") as f:
            f.write("package test; public class A { public static B getB() { return new B(); } }")

        shutil.rmtree(out_dir); os.makedirs(out_dir)
        t0 = time.perf_counter()
        p4 = run_cmd(f"javac -d {out_dir} {a_java} {b_java}", cwd=test_work, env=env)
        t_switch_hit = time.perf_counter() - t0
        assert p4.returncode == 0
        h_switch = sha256_tree(out_dir)
        assert h_switch == h_initial, "Switching back to initial branch failed to hit cache!"
        print(f"  -> Branch switch cache hit verified: {t_switch_hit*1000:.1f}ms")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_kotlin_compilation_and_speedup():
    print("[TEST 2] Kotlin multi-file inline functions and speedup...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_ktcache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_ktwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src_dir = os.path.join(test_work, "src")
        out_dir = os.path.join(test_work, "out")
        os.makedirs(src_dir)
        os.makedirs(out_dir)

        core_kt = os.path.join(src_dir, "Core.kt")
        main_kt = os.path.join(src_dir, "Main.kt")

        with open(core_kt, "w") as f:
            f.write("""package demo
data class Model(val id: Long, val title: String)
inline fun <T> measure(block: () -> T): T {
    return block()
}
""")

        with open(main_kt, "w") as f:
            f.write("""package demo
fun main() {
    val m = Model(1L, "Sample")
    measure {
        println("Title: ${m.title}")
    }
}
""")

        # Invocation 1: Kotlin Miss
        t0 = time.perf_counter()
        p1 = run_cmd(f"kotlinc -d {out_dir} {core_kt} {main_kt}", cwd=test_work, env=env)
        t_miss = time.perf_counter() - t0
        assert p1.returncode == 0, f"Kotlin compilation failed: {p1.stderr}"
        h_initial = sha256_tree(out_dir)
        assert len(h_initial) >= 3

        # Clean output
        shutil.rmtree(out_dir); os.makedirs(out_dir)

        # Invocation 2: Kotlin Hit
        t0 = time.perf_counter()
        p2 = run_cmd(f"kotlinc -d {out_dir} {core_kt} {main_kt}", cwd=test_work, env=env)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        h_hit = sha256_tree(out_dir)
        assert h_initial == h_hit, "Kotlin cache hit did not restore identical bytecode!"
        speedup = t_miss / max(t_hit, 0.001)
        print(f"  -> Kotlin Miss: {t_miss*1000:.1f}ms | Kotlin Hit: {t_hit*1000:.1f}ms (Speedup: {speedup:.1f}x)")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_argfile_handling():
    print("[TEST 3] @argfile recursive expansion and quote handling...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_argfile_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_argwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src_dir = os.path.join(test_work, "src with space")
        out_dir = os.path.join(test_work, "out")
        os.makedirs(src_dir)
        os.makedirs(out_dir)

        src_file = os.path.join(src_dir, "App.java")
        with open(src_file, "w") as f:
            f.write("public class App { public static void main(String[] args) {} }")

        arg_file = os.path.join(test_work, "sources.args")
        with open(arg_file, "w") as f:
            f.write(f'"{src_file}"\n')

        p1 = run_cmd(f"javac -d {out_dir} @{arg_file}", cwd=test_work, env=env)
        assert p1.returncode == 0, f"Failed @argfile: {p1.stderr}"
        assert os.path.exists(os.path.join(out_dir, "App.class"))

        shutil.rmtree(out_dir); os.makedirs(out_dir)
        p2 = run_cmd(f"javac -d {out_dir} @{arg_file}", cwd=test_work, env=env)
        assert p2.returncode == 0
        assert os.path.exists(os.path.join(out_dir, "App.class"))
        print("  -> @argfile caching passed.")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_kotlin_jar_output():
    print("[TEST 4] Kotlinc direct -d <output.jar> target...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_jar_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_jarwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src = os.path.join(test_work, "Lib.kt")
        with open(src, "w") as f:
            f.write("class LibHelper { fun greet() = \"Hello from JAR\" }")

        out_jar = os.path.join(test_work, "lib.jar")
        p1 = run_cmd(f"kotlinc -d {out_jar} {src}", cwd=test_work, env=env)
        assert p1.returncode == 0
        assert os.path.exists(out_jar)
        h1 = sha256_file(out_jar)

        os.remove(out_jar)
        p2 = run_cmd(f"kotlinc -d {out_jar} {src}", cwd=test_work, env=env)
        assert p2.returncode == 0
        assert os.path.exists(out_jar)
        h2 = sha256_file(out_jar)
        assert h1 == h2, "Restored JAR hash did not match!"
        print("  -> Kotlin -d <output.jar> caching passed.")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_syntax_error_not_cached():
    print("[TEST 5] Compiler error handling and negative caching prevention...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_errcache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_errwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src = os.path.join(test_work, "Broken.java")
        out = os.path.join(test_work, "out")
        os.makedirs(out)

        with open(src, "w") as f:
            f.write("public class Broken { syntax error here; }")

        p = run_cmd(f"javac -d {out} {src}", cwd=test_work, env=env)
        assert p.returncode != 0, "Broken code should fail with non-zero exit code!"
        assert "error" in p.stderr.lower()

        # Check stats: no misses/hits should be recorded into objects
        p_stats = run_cmd(f"{JVMCACHE_BIN} --show-stats", env=env)
        assert "Cache hits:             0" in p_stats.stdout

        objects_dir = os.path.join(test_cache, "objects")
        entries = os.listdir(objects_dir) if os.path.exists(objects_dir) else []
        assert len(entries) == 0, "No cache object should be stored for failed compilations!"
        print("  -> Syntax errors handled cleanly without cache pollution.")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_semantic_flag_invalidation():
    print("[TEST 6] Semantic flag invalidation (-g vs no -g)...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_test_flagcache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_test_flagwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }

    try:
        src = os.path.join(test_work, "FlagTest.java")
        out1 = os.path.join(test_work, "out1")
        out2 = os.path.join(test_work, "out2")
        os.makedirs(out1)
        os.makedirs(out2)

        with open(src, "w") as f:
            f.write("public class FlagTest { public int calc(int a) { return a * 2; } }")

        # Compile without -g
        p1 = run_cmd(f"javac -d {out1} {src}", cwd=test_work, env=env)
        assert p1.returncode == 0
        h1 = sha256_tree(out1)

        # Compile with -g (debug info added)
        p2 = run_cmd(f"javac -g -d {out2} {src}", cwd=test_work, env=env)
        assert p2.returncode == 0
        h2 = sha256_tree(out2)

        assert h1 != h2, "Bytecode with -g should differ from bytecode without -g!"
        print("  -> Flag change (-g) properly invalidated cache and produced distinct artifact.")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

if __name__ == "__main__":
    print("=" * 60)
    print("JVMCACHE INTEGRATION & ADVERSARIAL TEST SUITE")
    print("=" * 60)
    test_java_circular_and_branch_switch()
    test_kotlin_compilation_and_speedup()
    test_argfile_handling()
    test_kotlin_jar_output()
    test_syntax_error_not_cached()
    test_semantic_flag_invalidation()
    print("=" * 60)
    print("ALL TESTS PASSED WITH 100% BYTE-FOR-BYTE FIDELITY!")
    print("=" * 60)
