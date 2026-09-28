import os
import sys
import shutil
import tempfile
import subprocess

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BIN_DIR = os.path.join(REPO_ROOT, "bin")

def run(cmd, env=None, cwd=None):
    merged = os.environ.copy()
    if env:
        merged.update(env)
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=merged, cwd=cwd)

def test_cas_blob_deduplication():
    print("[CAS DEDUP TEST] Centralized Blob Inode Deduplication across targets...")
    test_cache = tempfile.mkdtemp(prefix="jvmcache_cas_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_cas_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        # Module 1: app1 compiles FooBarHelper.java
        src1_dir = os.path.join(test_work, "app1", "src")
        out1_dir = os.path.join(test_work, "app1", "out")
        os.makedirs(src1_dir)
        os.makedirs(out1_dir)
        with open(os.path.join(src1_dir, "FooBarHelper.java"), "w") as f:
            f.write("package com.foobar.common; public class FooBarHelper { public static int compute() { return 42; } }")

        p1 = run(f"javac -d {out1_dir} {os.path.join(src1_dir, 'FooBarHelper.java')}", env=env)
        assert p1.returncode == 0

        # Module 2: app2 compiles identical FooBarHelper.java but in different directory
        src2_dir = os.path.join(test_work, "app2", "src")
        out2_dir = os.path.join(test_work, "app2", "out")
        os.makedirs(src2_dir)
        os.makedirs(out2_dir)
        with open(os.path.join(src2_dir, "FooBarHelper.java"), "w") as f:
            f.write("package com.foobar.common; public class FooBarHelper { public static int compute() { return 42; } }")

        p2 = run(f"javac -d {out2_dir} {os.path.join(src2_dir, 'FooBarHelper.java')}", env=env)
        assert p2.returncode == 0

        # Inspect CAS directory
        cas_dir = os.path.join(test_cache, "cas")
        assert os.path.exists(cas_dir), "CAS blob pool directory was not created!"

        blobs = []
        for root, _, files in os.walk(cas_dir):
            for f in files:
                blobs.append(os.path.join(root, f))

        assert len(blobs) == 1, f"Expected exactly 1 shared blob in CAS, found {len(blobs)}"
        shared_blob = blobs[0]
        blob_stat = os.stat(shared_blob)
        print(f"  -> Shared CAS Blob: {os.path.basename(shared_blob)[:12]}.. (Inode: {blob_stat.st_ino}, Link count: {blob_stat.st_nlink})")
        assert blob_stat.st_nlink >= 2, "Blob was not hardlink-deduplicated across cache entries!"
        print("  -> Inode deduplication verified: identical bytecodes share physical disk blocks!")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

if __name__ == "__main__":
    print("=" * 76)
    print("JVMCACHE CONTENT-ADDRESSABLE STORAGE DEDUPLICATION SUITE")
    print("=" * 76)
    test_cas_blob_deduplication()
    print("=" * 76)
    print("ALL CAS DEDUPLICATION TESTS PASSED WITH 100% INODE SHARING!")
    print("=" * 76)
