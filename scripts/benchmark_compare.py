"""Compare render benchmarks from two revisions on the same host."""
import json
import math
import pathlib
import re
import statistics
import subprocess
import sys


def regression(baseline, candidate):
    if not baseline or not candidate:
        raise ValueError("empty benchmark sample")
    if any(not math.isfinite(value) or value <= 0 for value in baseline + candidate):
        raise ValueError("invalid benchmark sample")
    return statistics.median(candidate) > statistics.median(baseline) * 1.05


def build(root):
    subprocess.run([sys.executable, "-B", "scripts/bootstrap.py"], cwd=root, check=True)
    output = subprocess.check_output([
        "cargo", "bench", "--locked", "--bench", "render", "--no-run",
        "--message-format=json",
    ], cwd=root, text=True)
    for line in output.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == "render":
                return item["executable"]
    raise RuntimeError("render benchmark executable missing")


def measure(executable):
    output = subprocess.check_output([executable], text=True)
    match = re.search(r"render_ns_per_block=([0-9.]+)", output)
    if not match:
        raise RuntimeError("render benchmark measurement missing")
    return float(match[1])


def main():
    if len(sys.argv) != 2:
        raise ValueError("expected baseline checkout directory")
    baseline_binary = build(pathlib.Path(sys.argv[1]).resolve())
    candidate_binary = build(pathlib.Path.cwd())
    baseline, candidate = [], []
    for index in range(21):
        # Alternate order to reduce systematic thermal and scheduling bias.
        if index % 2:
            candidate.append(measure(candidate_binary))
            baseline.append(measure(baseline_binary))
        else:
            baseline.append(measure(baseline_binary))
            candidate.append(measure(candidate_binary))
    old, new = statistics.median(baseline), statistics.median(candidate)
    print(f"Render median: baseline={old:.2f} ns, candidate={new:.2f} ns")
    print(f"Render change: {(new / old - 1) * 100:+.2f}%")
    return regression(baseline, candidate)


if __name__ == "__main__":
    sys.exit(main())
