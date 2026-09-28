import os
import sys
import shutil
import tempfile
import time
import subprocess
import hashlib

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BIN_DIR = os.path.join(REPO_ROOT, "bin")

def run(cmd, env=None):
    merged = os.environ.copy()
    if env:
        merged.update(env)
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=merged)

def hash_tree(directory):
    hashes = {}
    for root, _, files in sorted(os.walk(directory)):
        for f in sorted(files):
            p = os.path.join(root, f)
            rel = os.path.relpath(p, directory)
            h = hashlib.sha256()
            with open(p, "rb") as fp:
                while chunk := fp.read(8192):
                    h.update(chunk)
            hashes[rel] = h.hexdigest()
    return hashes

def test_aosp_soong_javac_pipeline():
    print("[AOSP SIMULATION 1] Soong javac rule: -d $outDir -s $annoDir @$out.inc.rsp")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_aosp_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_aosp_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src_dir = os.path.join(test_work, "src")
        out_dir = os.path.join(test_work, "out", "classes")
        anno_dir = os.path.join(test_work, "out", "anno")
        os.makedirs(src_dir)
        os.makedirs(out_dir)
        os.makedirs(anno_dir)

        # Source file
        service_java = os.path.join(src_dir, "FooBarService.java")
        with open(service_java, "w") as f:
            f.write("""package com.foobar.service;
public class FooBarService {
    public void startProcessLocked(String name) {
        System.out.println("Starting process: " + name);
    }
}""")

        rsp_file = os.path.join(test_work, "classes.inc.rsp")
        with open(rsp_file, "w") as f:
            f.write(f'"{service_java}"\n')

        # Run 1: Miss
        t0 = time.perf_counter()
        p1 = run(f"javac -source 17 -target 17 -d {out_dir} -s {anno_dir} @{rsp_file}", env=env)
        t_miss = time.perf_counter() - t0
        assert p1.returncode == 0, f"Compilation failed: {p1.stderr}"
        assert os.path.exists(os.path.join(out_dir, "com", "foobar", "service", "FooBarService.class"))
        h_classes = hash_tree(out_dir)

        # Simulate Soong clean intermediate step
        shutil.rmtree(out_dir); os.makedirs(out_dir)
        shutil.rmtree(anno_dir); os.makedirs(anno_dir)

        # Run 2: Hit!
        t0 = time.perf_counter()
        p2 = run(f"javac -source 17 -target 17 -d {out_dir} -s {anno_dir} @{rsp_file}", env=env)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        assert h_classes == hash_tree(out_dir), "Classes not restored identically!"
        print(f"  -> AOSP Soong javac rule simulated: Miss {t_miss*1000:.1f}ms | Hit {t_hit*1000:.1f}ms ({t_miss/max(t_hit, 0.001):.1f}x)")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_aosp_kotlinc_pipeline():
    print("[AOSP SIMULATION 2] Soong kotlinc rule: -jvm-target 17 -d $classesDir @$rsp")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_aosp_ktcache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_aosp_ktwork_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src_dir = os.path.join(test_work, "src")
        classes_dir = os.path.join(test_work, "out", "classes")
        os.makedirs(src_dir)
        os.makedirs(classes_dir)

        kt_src = os.path.join(src_dir, "FooBarComponent.kt")
        with open(kt_src, "w") as f:
            f.write("""package com.foobar.component
data class FooBarState(val isVisible: Boolean, val alpha: Float)
class FooBarComponent {
    fun updateState(state: FooBarState) = state.isVisible
}""")

        rsp_file = os.path.join(test_work, "kotlinc.rsp")
        with open(rsp_file, "w") as f:
            f.write(f'"{kt_src}"\n')

        # Run 1: Miss
        t0 = time.perf_counter()
        p1 = run(f"kotlinc -jvm-target 17 -d {classes_dir} @{rsp_file}", env=env)
        t_miss = time.perf_counter() - t0
        assert p1.returncode == 0, f"Kotlin compilation failed: {p1.stderr}"
        h_classes = hash_tree(classes_dir)

        # Simulate clean
        shutil.rmtree(classes_dir); os.makedirs(classes_dir)

        # Run 2: Hit!
        t0 = time.perf_counter()
        p2 = run(f"kotlinc -jvm-target 17 -d {classes_dir} @{rsp_file}", env=env)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        assert h_classes == hash_tree(classes_dir), "Kotlin classes not restored identically!"
        print(f"  -> AOSP Soong kotlinc rule simulated: Miss {t_miss*1000:.1f}ms | Hit {t_hit*1000:.1f}ms ({t_miss/max(t_hit, 0.001):.1f}x)")

    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

if __name__ == "__main__":
    print("=" * 76)
    print("AOSP SOONG / NINJA PIPELINE VERIFICATION SUITE")
    print("=" * 76)
    test_aosp_soong_javac_pipeline()
    test_aosp_kotlinc_pipeline()
    print("=" * 76)
    print("ALL AOSP PIPELINES FULLY VERIFIED & COMPATIBLE!")
    print("=" * 76)
