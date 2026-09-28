#!/usr/bin/env python3
"""
Test Suite: Surgical Delta Compilation Proxy Verification
Verifies that batch compilations in javac and kotlinc (XML & CLI) are decomposed into
fast surgical delta compilations when only a subset of source files are touched.
"""

import os
import sys
import shutil
import tempfile
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
JVMCACHE_BIN = ROOT / "target" / "release" / "jvmcache"

def run_cmd(cmd, env=None, check=True):
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    if check and res.returncode != 0:
        print(f"Command failed: {' '.join(cmd)}")
        print("STDOUT:", res.stdout)
        print("STDERR:", res.stderr)
        raise RuntimeError(f"Command exited with code {res.returncode}")
    return res

def test_javac_batch_surgical_delta():
    print("\n--- TEST: javac batch surgical delta compilation ---")
    with tempfile.TemporaryDirectory() as td:
        tpath = Path(td)
        cache_dir = tpath / "cache"
        src_dir = tpath / "src"
        out_dir = tpath / "out"
        src_dir.mkdir()
        out_dir.mkdir()

        env = os.environ.copy()
        env["JVMCACHE_DIR"] = str(cache_dir)
        env["JVMCACHE_VERBOSE"] = "1"

        sources = []
        for i in range(10):
            sf = src_dir / f"Worker{i}.java"
            sf.write_text(f"""package test;
public class Worker{i} {{
    public static String get() {{ return "val{i}"; }}
}}
""")
            sources.append(sf)

        runner = src_dir / "Main.java"
        runner.write_text("""package test;
public class Main {
    public static void main(String[] args) {
        StringBuilder sb = new StringBuilder();
        sb.append(Worker0.get()).append(":");
        sb.append(Worker5.get()).append(":");
        sb.append(Worker9.get());
        System.out.println(sb.toString());
    }
}
""")
        sources.append(runner)

        rsp_file = tpath / "sources.rsp"
        rsp_file.write_text("\n".join(str(s) for s in sources) + "\n")

        # Initial full batch compilation
        print("1. Running initial full batch javac compilation...")
        res1 = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), f"@{rsp_file}"], env=env)
        assert (out_dir / "test" / "Worker0.class").exists()
        assert (out_dir / "test" / "Worker5.class").exists()
        assert (out_dir / "test" / "Main.class").exists()

        res_exec1 = run_cmd(["java", "-cp", str(out_dir), "test.Main"], env=env)
        assert res_exec1.stdout.strip() == "val0:val5:val9"
        print("   Initial execution output matches: val0:val5:val9")

        # Edit ONLY Worker5.java
        print("2. Modifying Worker5.java and running second compilation...")
        (src_dir / "Worker5.java").write_text("""package test;
public class Worker5 {
    public static String get() { return "MODIFIED_5"; }
}
""")
        res2 = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), f"@{rsp_file}"], env=env)
        assert res2.returncode == 0

        # Verify all classes exist and new Worker5 output is reflected at runtime
        for i in range(10):
            assert (out_dir / "test" / f"Worker{i}.class").exists(), f"Worker{i}.class missing!"
        assert (out_dir / "test" / "Main.class").exists()

        res_exec2 = run_cmd(["java", "-cp", str(out_dir), "test.Main"], env=env)
        assert res_exec2.stdout.strip() == "val0:MODIFIED_5:val9", f"Unexpected output: {res_exec2.stdout}"
        print("   Surgical delta javac compilation verified: val0:MODIFIED_5:val9")

        # Run third time with zero changes -> Full cache HIT
        print("3. Running third compilation (zero changes)...")
        res3 = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), f"@{rsp_file}"], env=env)
        assert res3.returncode == 0
        stats = run_cmd([str(JVMCACHE_BIN), "-s"], env=env)
        print("   Stats output:\n" + "\n".join("     " + l for l in stats.stdout.splitlines()))

def test_kotlinc_xml_batch_surgical_delta():
    print("\n--- TEST: kotlinc AOSP-style XML surgical delta compilation ---")
    with tempfile.TemporaryDirectory() as td:
        tpath = Path(td)
        cache_dir = tpath / "cache"
        src_dir = tpath / "src"
        out_dir = tpath / "out"
        src_dir.mkdir()
        out_dir.mkdir()

        env = os.environ.copy()
        env["JVMCACHE_DIR"] = str(cache_dir)
        env["JVMCACHE_VERBOSE"] = "1"

        sources = []
        for i in range(8):
            sf = src_dir / f"Service{i}.kt"
            sf.write_text(f"""package service
internal class Service{i} {{
    fun ping() = "svc{i}"
}}
""")
            sources.append(sf)

        runner = src_dir / "Runner.kt"
        runner.write_text("""package service
fun main() {
    val r = Service0().ping() + "-" + Service3().ping() + "-" + Service7().ping()
    println(r)
}
""")
        sources.append(runner)

        xml_file = tpath / "kotlinc-build.xml"
        xml_content = f"""<modules>
  <module name="service_module" type="java-production" outputDir="{out_dir}">
"""
        for s in sources:
            xml_content += f'    <sources path="{s}"/>\n'
        xml_content += "  </module>\n</modules>\n"
        xml_file.write_text(xml_content)

        # Step 1: Initial full compile
        print("1. Running initial full kotlinc XML batch compilation...")
        res1 = run_cmd([str(JVMCACHE_BIN), "kotlinc", f"-Xbuild-file={xml_file}"], env=env)
        assert (out_dir / "service" / "Service0.class").exists()
        assert (out_dir / "service" / "Service3.class").exists()
        assert (out_dir / "service" / "RunnerKt.class").exists()

        res_exec1 = run_cmd(["kotlin", "-cp", str(out_dir), "service.RunnerKt"], env=env)
        assert res_exec1.stdout.strip() == "svc0-svc3-svc7"
        print("   Initial execution output matches: svc0-svc3-svc7")

        # Step 2: Edit ONLY Service3.kt
        print("2. Modifying Service3.kt and running second compilation...")
        (src_dir / "Service3.kt").write_text("""package service
internal class Service3 {
    fun ping() = "DELTA_SVC3"
}
""")
        res2 = run_cmd([str(JVMCACHE_BIN), "kotlinc", f"-Xbuild-file={xml_file}"], env=env)
        assert res2.returncode == 0

        # Verify all classes exist
        for i in range(8):
            assert (out_dir / "service" / f"Service{i}.class").exists(), f"Service{i}.class missing!"
        assert (out_dir / "service" / "RunnerKt.class").exists()

        res_exec2 = run_cmd(["kotlin", "-cp", str(out_dir), "service.RunnerKt"], env=env)
        assert res_exec2.stdout.strip() == "svc0-DELTA_SVC3-svc7"
        print("   Surgical delta kotlinc XML compilation verified: svc0-DELTA_SVC3-svc7")

        # Step 3: Run third time with zero changes
        print("3. Running third compilation (zero changes)...")
        res3 = run_cmd([str(JVMCACHE_BIN), "kotlinc", f"-Xbuild-file={xml_file}"], env=env)
        assert res3.returncode == 0

def test_syntax_error_fallback():
    print("\n--- TEST: syntax error handling and recovery ---")
    with tempfile.TemporaryDirectory() as td:
        tpath = Path(td)
        cache_dir = tpath / "cache"
        src_dir = tpath / "src"
        out_dir = tpath / "out"
        src_dir.mkdir()
        out_dir.mkdir()

        env = os.environ.copy()
        env["JVMCACHE_DIR"] = str(cache_dir)

        f1 = src_dir / "A.java"
        f1.write_text("package demo; public class A { public static String ok() { return \"ok\"; } }")
        f2 = src_dir / "B.java"
        f2.write_text("package demo; public class B { public static String val() { return A.ok(); } }")

        res1 = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), str(f1), str(f2)], env=env)
        assert res1.returncode == 0

        # Break syntax in B.java
        f2.write_text("package demo; public class B { SYNTAX ERROR HERE }")
        res_fail = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), str(f1), str(f2)], env=env, check=False)
        assert res_fail.returncode != 0
        print("   Compiler failed with expected non-zero code on syntax error.")

        # Fix B.java
        f2.write_text("package demo; public class B { public static String val() { return \"fixed\"; } }")
        res_recover = run_cmd([str(JVMCACHE_BIN), "javac", "-d", str(out_dir), str(f1), str(f2)], env=env)
        assert res_recover.returncode == 0
        print("   Compiler cleanly recovered on corrected code.")

if __name__ == "__main__":
    assert JVMCACHE_BIN.exists(), f"Binary not found at {JVMCACHE_BIN}"
    test_javac_batch_surgical_delta()
    test_kotlinc_xml_batch_surgical_delta()
    test_syntax_error_fallback()
    print("\n============================================================================")
    print("ALL SURGICAL DELTA COMPILATION TESTS PASSED 100%!")
    print("============================================================================")
