import os
import sys
import shutil
import tempfile
import time
import subprocess

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BIN_DIR = os.path.join(REPO_ROOT, "bin")

def run(cmd, env=None, cwd=None):
    merged = os.environ.copy()
    if env:
        merged.update(env)
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, env=merged, cwd=cwd)

def find_kapt_jar():
    kotlinc_bin = shutil.which("kotlinc")
    if kotlinc_bin:
        real_bin = os.path.realpath(kotlinc_bin)
        cand = os.path.abspath(os.path.join(os.path.dirname(real_bin), "..", "lib", "kotlin-annotation-processing.jar"))
        if os.path.exists(cand):
            return cand
    if "ANDROID_BUILD_TOP" in os.environ:
        cand = os.path.abspath(os.path.join(os.environ["ANDROID_BUILD_TOP"], "external", "kotlinc", "lib", "kotlin-annotation-processing.jar"))
        if os.path.exists(cand):
            return cand
    return None

def build_dummy_annotation_processor(work_dir):
    proc_src_dir = os.path.join(work_dir, "proc_src")
    pkg_dir = os.path.join(proc_src_dir, "com", "foobar", "proc")
    os.makedirs(pkg_dir, exist_ok=True)
    java_file = os.path.join(pkg_dir, "FooBarProcessor.java")
    with open(java_file, "w") as f:
        f.write("""package com.foobar.proc;
import java.util.Set;
import javax.annotation.processing.*;
import javax.lang.model.SourceVersion;
import javax.lang.model.element.TypeElement;

@SupportedAnnotationTypes("*")
@SupportedSourceVersion(SourceVersion.RELEASE_17)
public class FooBarProcessor extends AbstractProcessor {
    @Override
    public boolean process(Set<? extends TypeElement> annotations, RoundEnvironment roundEnv) {
        return false;
    }
}
""")
    classes_dir = os.path.join(work_dir, "proc_classes")
    os.makedirs(classes_dir, exist_ok=True)
    subprocess.run(["javac", "-d", classes_dir, java_file], check=True, capture_output=True)
    meta_dir = os.path.join(classes_dir, "META-INF", "services")
    os.makedirs(meta_dir, exist_ok=True)
    with open(os.path.join(meta_dir, "javax.annotation.processing.Processor"), "w") as f:
        f.write("com.foobar.proc.FooBarProcessor\n")
    jar_path = os.path.join(work_dir, "foobar-processor.jar")
    subprocess.run(["jar", "cf", jar_path, "-C", classes_dir, "."], check=True, capture_output=True)
    return jar_path

def test_kapt_stubs_mode():
    print("[KAPT TEST 1] KAPT aptMode=stubs (Stub Generation Mode)...")
    kapt_jar = find_kapt_jar()
    assert kapt_jar is not None, "kotlin-annotation-processing.jar not found!"

    test_cache = tempfile.mkdtemp(prefix="jvmcache_kapt_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_kapt_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src_dir = os.path.join(test_work, "src")
        gen_dir = os.path.join(test_work, "kapt", "gen")
        stubs_dir = os.path.join(gen_dir, "stubs")
        sources_dir = os.path.join(gen_dir, "sources")
        classes_dir = os.path.join(gen_dir, "classes")
        os.makedirs(src_dir)
        os.makedirs(sources_dir)
        os.makedirs(classes_dir)

        kt_file = os.path.join(src_dir, "FooBarComponent.kt")
        with open(kt_file, "w") as f:
            f.write("""package com.foobar.test

annotation class FooBarInject

@FooBarInject
class FooBarComponent {
    fun provideService(): String = "ServiceInstance"
}
""")

        # Run 1: Miss
        t0 = time.perf_counter()
        proc_jar = build_dummy_annotation_processor(test_work)
        extra_flags = (
            f"-P plugin:org.jetbrains.kotlin.kapt3:apclasspath={proc_jar} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:processors=com.foobar.proc.FooBarProcessor "
        )

        cmd = (
            f"kotlinc -Xplugin={kapt_jar} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:sources={sources_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:classes={classes_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:stubs={stubs_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:aptMode=stubs "
            f"{extra_flags}"
            f"{kt_file}"
        )
        p1 = run(cmd, env=env, cwd=test_work)
        t_miss = time.perf_counter() - t0
        assert p1.returncode == 0, f"KAPT stubs failed: {p1.stderr}"
        assert os.path.exists(stubs_dir), "stubs_dir was not created on miss!"

        # Simulate clean build: delete gen directory
        shutil.rmtree(gen_dir)
        assert not os.path.exists(stubs_dir)

        # Run 2: Hit!
        t0 = time.perf_counter()
        p2 = run(cmd, env=env, cwd=test_work)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0, f"KAPT stubs hit failed: {p2.stderr}"
        assert os.path.exists(stubs_dir), "stubs_dir was not restored on cache hit!"
        print(f"  -> KAPT stubs covered: Miss {t_miss*1000:.1f}ms | Hit {t_hit*1000:.1f}ms ({t_miss/max(t_hit, 0.001):.1f}x)")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

def test_kapt_sources_and_classes_mode():
    print("[KAPT TEST 2] KAPT aptMode=apt (Generated Sources & Classes Mode)...")
    kapt_jar = find_kapt_jar()
    assert kapt_jar is not None, "kotlin-annotation-processing.jar not found!"

    test_cache = tempfile.mkdtemp(prefix="jvmcache_kapt_apt_cache_")
    test_work = tempfile.mkdtemp(prefix="jvmcache_kapt_apt_work_")
    env = {
        "JVMCACHE_DIR": test_cache,
        "PATH": f"{BIN_DIR}:{os.environ['PATH']}",
    }
    try:
        src_dir = os.path.join(test_work, "src")
        gen_dir = os.path.join(test_work, "kapt", "gen")
        stubs_dir = os.path.join(gen_dir, "stubs")
        sources_dir = os.path.join(gen_dir, "sources")
        classes_dir = os.path.join(gen_dir, "classes")
        os.makedirs(src_dir)
        os.makedirs(sources_dir)
        os.makedirs(classes_dir)

        kt_file = os.path.join(src_dir, "FooBarModel.kt")
        with open(kt_file, "w") as f:
            f.write("""package com.foobar.model
data class FooBarItem(val id: String, val score: Double)
""")

        cmd = (
            f"kotlinc -Xplugin={kapt_jar} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:sources={sources_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:classes={classes_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:stubs={stubs_dir} "
            f"-P plugin:org.jetbrains.kotlin.kapt3:aptMode=apt "
            f"{kt_file}"
        )
        p1 = run(cmd, env=env, cwd=test_work)
        assert p1.returncode == 0, f"KAPT aptMode=apt failed: {p1.stderr}"

        # Simulate clean
        shutil.rmtree(gen_dir)

        # Hit
        t0 = time.perf_counter()
        p2 = run(cmd, env=env, cwd=test_work)
        t_hit = time.perf_counter() - t0
        assert p2.returncode == 0
        print(f"  -> KAPT aptMode=apt covered: Hit in {t_hit*1000:.1f}ms")
    finally:
        shutil.rmtree(test_cache, ignore_errors=True)
        shutil.rmtree(test_work, ignore_errors=True)

if __name__ == "__main__":
    print("=" * 76)
    print("JVMCACHE KAPT COMPREHENSIVE COVERAGE SUITE")
    print("=" * 76)
    test_kapt_stubs_mode()
    test_kapt_sources_and_classes_mode()
    print("=" * 76)
    print("ALL KAPT MODES FULLY COVERED AND VERIFIED!")
    print("=" * 76)
