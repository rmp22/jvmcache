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
                while chunk := fp.read(16384):
                    h.update(chunk)
            hashes[rel] = h.hexdigest()
    return hashes

def run_project_benchmark(name, repo_url, compiler_cmd, source_filter, mutate_target, mutate_content):
    print("\n" + "=" * 76)
    print(f"BENCHMARK: {name}")
    print("=" * 76)
    
    clone_dir = tempfile.mkdtemp(prefix="jvmcache_repo_")
    cache_dir = tempfile.mkdtemp(prefix="jvmcache_cache_")
    out_dir = tempfile.mkdtemp(prefix="jvmcache_out_")
    
    env_nocache = os.environ.copy()
    env_cache = os.environ.copy()
    env_cache["JVMCACHE_DIR"] = cache_dir
    env_cache["PATH"] = f"{BIN_DIR}:{os.environ['PATH']}"
    
    results = {"name": name}
    
    try:
        print(f"  -> Cloning {repo_url} (depth 1)...")
        p_clone = run(f"git clone --depth 1 {repo_url} {clone_dir}")
        assert p_clone.returncode == 0, f"Clone failed: {p_clone.stderr}"
        
        # Collect sources
        sources = []
        for root, _, files in os.walk(clone_dir):
            for f in sorted(files):
                full_path = os.path.join(root, f)
                if source_filter(full_path, f):
                    sources.append(full_path)
                    
        assert len(sources) > 0, "No sources matched filter!"
        argfile = os.path.join(clone_dir, "sources.args")
        with open(argfile, "w") as f:
            for s in sources:
                f.write(f'"{s}"\n')
                
        print(f"  -> Total source files collected: {len(sources)}")
        
        # 1. Baseline Cold (No Cache)
        shutil.rmtree(out_dir); os.makedirs(out_dir)
        t0 = time.perf_counter()
        p_base = run(f"{compiler_cmd} -d {out_dir} @{argfile}", env=env_nocache)
        t_base = time.perf_counter() - t0
        assert p_base.returncode == 0, f"Baseline compile failed: {p_base.stderr}"
        h_baseline = hash_tree(out_dir)
        num_classes = len(h_baseline)
        print(f"  [1] Baseline {compiler_cmd} (no cache):  {t_base*1000:7.1f} ms  ({num_classes} artifacts)")
        results["baseline_ms"] = t_base * 1000
        results["artifacts"] = num_classes
        
        # 2. jvmcache Cold (Cache Miss)
        shutil.rmtree(out_dir); os.makedirs(out_dir)
        t0 = time.perf_counter()
        p_miss = run(f"{compiler_cmd} -d {out_dir} @{argfile}", env=env_cache)
        t_miss = time.perf_counter() - t0
        assert p_miss.returncode == 0, f"jvmcache miss failed: {p_miss.stderr}"
        h_miss = hash_tree(out_dir)
        assert h_baseline == h_miss, "Bytecode divergence on cache miss!"
        overhead = (t_miss - t_base) * 1000
        print(f"  [2] jvmcache Cold (cache miss):   {t_miss*1000:7.1f} ms  (overhead: {overhead:+.1f} ms)")
        results["miss_ms"] = t_miss * 1000
        results["overhead_ms"] = overhead
        
        # 3. jvmcache Rebuild (Cache Hit!)
        shutil.rmtree(out_dir); os.makedirs(out_dir)
        t0 = time.perf_counter()
        p_hit = run(f"{compiler_cmd} -d {out_dir} @{argfile}", env=env_cache)
        t_hit = time.perf_counter() - t0
        assert p_hit.returncode == 0, f"jvmcache hit failed: {p_hit.stderr}"
        h_hit = hash_tree(out_dir)
        assert h_baseline == h_hit, "Bytecode divergence on cache hit!"
        speedup = t_base / max(t_hit, 0.0001)
        print(f"  [3] jvmcache Rebuild (cache hit): {t_hit*1000:7.1f} ms  (Speedup vs baseline: {speedup:.1f}x)")
        results["hit_ms"] = t_hit * 1000
        results["speedup"] = speedup
        
        # 4. Branch Switch Simulation
        target_file = os.path.join(clone_dir, mutate_target)
        if os.path.exists(target_file):
            with open(target_file, "r") as f:
                orig_content = f.read()
                
            # Mutate to branch 'feature'
            with open(target_file, "w") as f:
                f.write(orig_content + mutate_content)
                
            shutil.rmtree(out_dir); os.makedirs(out_dir)
            p_branch = run(f"{compiler_cmd} -d {out_dir} @{argfile}", env=env_cache)
            assert p_branch.returncode == 0
            h_branch = hash_tree(out_dir)
            assert h_branch != h_baseline
            
            # Switch back to 'main'
            with open(target_file, "w") as f:
                f.write(orig_content)
                
            shutil.rmtree(out_dir); os.makedirs(out_dir)
            t0 = time.perf_counter()
            p_switch = run(f"{compiler_cmd} -d {out_dir} @{argfile}", env=env_cache)
            t_switch = time.perf_counter() - t0
            assert p_switch.returncode == 0
            h_switch = hash_tree(out_dir)
            assert h_switch == h_baseline, "Branch switch did not restore exact bytecode!"
            print(f"  [4] Switched back to main branch: {t_switch*1000:7.1f} ms  (Instant cache hit restored!)")
            results["switch_ms"] = t_switch * 1000
            
    finally:
        shutil.rmtree(clone_dir, ignore_errors=True)
        shutil.rmtree(cache_dir, ignore_errors=True)
        shutil.rmtree(out_dir, ignore_errors=True)
        
    return results

def main():
    print("#" * 76)
    print("#  JVMCACHE REAL-WORLD OPEN-SOURCE PERFORMANCE BENCHMARKS")
    print("#" * 76)
    
    suite_results = []
    
    # Project 1: Apache Commons IO (Java)
    res_io = run_project_benchmark(
        name="Apache Commons IO (Java)",
        repo_url="https://github.com/apache/commons-io.git",
        compiler_cmd="javac",
        source_filter=lambda path, f: f.endswith(".java") and "src/main/java" in path and f != "module-info.java",
        mutate_target="src/main/java/org/apache/commons/io/FileUtils.java",
        mutate_content="\nclass FileUtilsBranchToken {}\n"
    )
    suite_results.append(res_io)
    
    # Project 2: Apache Commons Lang (Java)
    res_lang = run_project_benchmark(
        name="Apache Commons Lang (Java)",
        repo_url="https://github.com/apache/commons-lang.git",
        compiler_cmd="javac",
        source_filter=lambda path, f: f.endswith(".java") and "src/main/java" in path and f != "module-info.java",
        mutate_target="src/main/java/org/apache/commons/lang3/StringUtils.java",
        mutate_content="\nclass StringUtilsBranchToken {}\n"
    )
    suite_results.append(res_lang)
    
    # Project 3: Ajalt Clikt (Kotlin)
    res_clikt = run_project_benchmark(
        name="Ajalt Clikt (Kotlin)",
        repo_url="https://github.com/ajalt/clikt.git",
        compiler_cmd="kotlinc",
        source_filter=lambda path, f: f.endswith(".kt") and ("clikt/src/commonMain" in path or "clikt/src/jvmMain" in path),
        mutate_target="clikt/src/commonMain/kotlin/com/github/ajalt/clikt/core/CliktCommand.kt",
        mutate_content="\nclass CliktBranchToken\n"
    )
    suite_results.append(res_clikt)
    
    # Project 4: Square JavaPoet (Java)
    res_poet = run_project_benchmark(
        name="Square JavaPoet (Java)",
        repo_url="https://github.com/square/javapoet.git",
        compiler_cmd="javac",
        source_filter=lambda path, f: f.endswith(".java") and "src/main/java" in path,
        mutate_target="src/main/java/com/squareup/javapoet/JavaFile.java",
        mutate_content="\nclass JavaFileBranchToken {}\n"
    )
    suite_results.append(res_poet)
    
    print("\n" + "=" * 76)
    print("CONSOLIDATED REAL-WORLD PERFORMANCE BENCHMARK SUMMARY")
    print("=" * 76)
    print(f"{'Repository / Project':<28} | {'Artifacts':<9} | {'Baseline':<9} | {'Cache Hit':<9} | {'Speedup':<8}")
    print("-" * 76)
    for r in suite_results:
        print(f"{r['name']:<28} | {r['artifacts']:<9} | {r['baseline_ms']:6.1f} ms | {r['hit_ms']:6.1f} ms | {r['speedup']:6.1f}x")
    print("=" * 76)
    print("FIDELITY: 100% BIT-FOR-BIT IDENTICAL BYTECODE VERIFIED ACROSS ALL RUNS.")
    print("=" * 76 + "\n")

if __name__ == "__main__":
    main()
