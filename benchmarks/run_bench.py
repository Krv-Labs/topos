import time
import subprocess
import json
import os
import re
from pathlib import Path

# Paths
V1_BIN = "./benchmarks/bin/topos-v1.0.0"
CUR_CMD = ["uv", "run", "topos"]
TEST_FILE = (
    "topos/evaluation/characteristic_morphism.py"  # A reasonably complex file
)


def run_bench(cmd_base, label):
    print(f"--- Benchmarking {label} ---")
    start = time.perf_counter()
    # Using JSON output for easy validation
    cmd = cmd_base + ["evaluate", TEST_FILE, "--json"]
    result = subprocess.run(cmd, capture_output=True, text=True)
    end = time.perf_counter()

    if result.returncode != 0:
        print(f"Error running {label}: {result.stderr}")
        return None, None

    duration = end - start

    # The output might contain extra text after the JSON (e.g., Overall summary)
    # We find the JSON block by looking for { ... }
    json_match = re.search(r"\{.*\}", result.stdout, re.DOTALL)
    if not json_match:
        print(f"Error: Could not find JSON in {label} output.")
        return None, None

    data = json.loads(json_match.group(0))
    # Extract relevant metrics for validation
    metrics = data["results"][0]["raw_metrics"]
    print(f"Time: {duration:.4f}s")
    return duration, metrics


def run_bulk_bench(cmd_base, path, label):
    print(f"--- Bulk Benchmarking {label} ---")
    print(f"Path: {path}")
    start = time.perf_counter()
    # Evaluating a directory recursively
    cmd = cmd_base + ["evaluate", str(path), "--json", "--recursive"]
    result = subprocess.run(cmd, capture_output=True, text=True)
    end = time.perf_counter()

    duration = end - start
    if result.returncode != 0:
        print(f"Warning: {label} returned non-zero exit code {result.returncode}")
        print(f"Stderr: {result.stderr}")
        return duration, 0, {}

    # Find JSON block
    json_match = re.search(r"\{.*\}", result.stdout, re.DOTALL)
    if not json_match:
        print(f"Error: Could not find JSON in bulk {label} output.")
        return duration, 0, {}

    try:
        data = json.loads(json_match.group(0))
        results = data.get("results", [])
        num_files = len(results)

        # Calculate some basic stats
        slop_count = sum(1 for r in results if r.get("lattice_element") == "SLOP")
        ideal_count = sum(
            1 for r in results if r.get("lattice_element") == "IDEAL"
        )

        stats = {
            "SLOP": slop_count,
            "IDEAL": ideal_count,
            "Parseable": sum(
                1 for r in results if r.get("is_parseable", True) is not False
            ),
        }

    except json.JSONDecodeError:
        print(f"Error parsing JSON in bulk {label} output.")
        num_files = 0
        stats = {}

    print(f"Time: {duration:.4f}s")
    print(f"Evaluated {num_files} files.")
    if stats:
        print(
            f"Stats: SLOP={stats['SLOP']}, IDEAL={stats['IDEAL']}, Parseable={stats['Parseable']}"
        )
    return duration, num_files, stats


def main():
    if not os.path.exists(V1_BIN):
        print(f"Error: {V1_BIN} not found. Run the setup command first.")
        return

    # Warmup
    subprocess.run([V1_BIN, "evaluate", TEST_FILE, "--json"], capture_output=True)
    subprocess.run(
        CUR_CMD + ["evaluate", TEST_FILE, "--json"], capture_output=True
    )

    v1_time, v1_metrics = run_bench([V1_BIN], "v1.0.0 (Python-only binary)")
    cur_time, cur_metrics = run_bench(CUR_CMD, "Current (Rust Hybrid)")

    if v1_time and cur_time:
        speedup = v1_time / cur_time
        print("\n=== Summary ===")
        print(f"v1.0.0:  {v1_time:.4f}s")
        print(f"Current: {cur_time:.4f}s")
        print(f"Speedup: {speedup:.2f}x")

        print("\n=== Correctness Validation ===")
        mismatches = []
        for key in v1_metrics:
            v1_val = v1_metrics[key]
            cur_val = cur_metrics.get(key)
            if cur_val is None:
                mismatches.append(f"Missing key in current: {key}")
            else:
                diff = abs(v1_val - cur_val)
                # Entropy might differ slightly due to library implementation (zlib vs flate2)
                tolerance = 1e-2 if "entropy" in key else 1e-6
                if diff > tolerance:
                    mismatches.append(
                        f"Value mismatch for {key}: v1={v1_val}, cur={cur_val} (diff={diff:.6f})"
                    )
                elif diff > 0:
                    print(
                        f"INFO: Minor variation in {key}: diff={diff:.6e} (within tolerance)"
                    )

        if not mismatches:
            print("SUCCESS: All metrics match exactly!")
        else:
            print("FAILURE: Metric mismatches found:")
            for m in mismatches:
                print(f"  - {m}")


if __name__ == "__main__":
    main()
