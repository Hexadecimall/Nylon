"""Compare benchmarks from two revisions on the same host.

A shared runner's timings wander by a few per cent between runs, so a
median that moved past the threshold is not on its own evidence of a
regression: at forty nanoseconds a block, five per cent is two
nanoseconds. A run counts as a regression when the median moved past the
threshold and the two samples separate, meaning the candidate's lower
quartile sits at or above the baseline's upper quartile. Noise fails the
second test; a real slowdown passes both.
"""
import json
import math
import pathlib
import re
import statistics
import subprocess
import sys

# Benchmarks compared, each with the name it prints its figure under.
BENCHMARKS = (
    ("render", "render_ns_per_block"),
    ("mixer", "mixer_ns_per_block"),
    ("audio_timeline", "audio_timeline_ns_per_block"),
    ("routed_playback", "routed_playback_ns_per_block"),
)
THRESHOLD = 1.05
SAMPLES = 21


def quantile(values, fraction):
    """Linearly interpolated quantile, so small samples still separate."""
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = fraction * (len(ordered) - 1)
    low = math.floor(position)
    high = math.ceil(position)
    if low == high:
        return ordered[low]
    return ordered[low] + (ordered[high] - ordered[low]) * (position - low)


def regression(baseline, candidate):
    """True when the candidate is slower past the threshold and the two
    samples do not overlap."""
    if not baseline or not candidate:
        raise ValueError("empty benchmark sample")
    if any(not math.isfinite(value) or value <= 0 for value in baseline + candidate):
        raise ValueError("invalid benchmark sample")
    moved = statistics.median(candidate) > statistics.median(baseline) * THRESHOLD
    separated = quantile(candidate, 0.25) >= quantile(baseline, 0.75)
    return moved and separated


class Unbuildable(Exception):
    """The revision does not compile, so it cannot be measured."""


def build(root, name):
    subprocess.run([sys.executable, "-B", "scripts/bootstrap.py"], cwd=root, check=True)
    try:
        output = subprocess.check_output([
            "cargo", "bench", "--locked", "--bench", name, "--no-run",
            "--message-format=json",
        ], cwd=root, text=True)
    except subprocess.CalledProcessError as error:
        raise Unbuildable(f"{root} does not build") from error
    for line in output.splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item.get("executable"):
            if item["target"]["name"] == name:
                return item["executable"]
    raise RuntimeError(f"{name} benchmark executable missing")


def measure(executable, key):
    output = subprocess.check_output([executable], text=True)
    match = re.search(rf"{key}=([0-9.]+)", output)
    if not match:
        raise RuntimeError(f"{key} measurement missing")
    return float(match[1])


def compare(root, name, key):
    """Measures both revisions and reports whether the candidate regressed.

    A baseline that does not build gives nothing to compare against. That
    is reported and skipped rather than failing the run, since the point
    of this check is the change under test, not the state of the revision
    behind it. A candidate that does not build is a failure.
    """
    try:
        baseline_binary = build(root, name)
    except Unbuildable:
        print(f"{name}: the baseline revision does not build, skipped")
        return False
    candidate_binary = build(pathlib.Path.cwd(), name)
    baseline, candidate = [], []
    for index in range(SAMPLES):
        # Alternate order to reduce systematic thermal and scheduling bias.
        if index % 2:
            candidate.append(measure(candidate_binary, key))
            baseline.append(measure(baseline_binary, key))
        else:
            baseline.append(measure(baseline_binary, key))
            candidate.append(measure(candidate_binary, key))
    old, new = statistics.median(baseline), statistics.median(candidate)
    print(f"{name} median: baseline={old:.2f} ns, candidate={new:.2f} ns")
    print(f"{name} change: {(new / old - 1) * 100:+.2f}%")
    print(
        f"{name} spread: baseline upper quartile={quantile(baseline, 0.75):.2f} ns, "
        f"candidate lower quartile={quantile(candidate, 0.25):.2f} ns"
    )
    slower = regression(baseline, candidate)
    print(f"{name} verdict: {'regression' if slower else 'within noise'}")
    return slower


def main():
    if len(sys.argv) != 2:
        raise ValueError("expected baseline checkout directory")
    root = pathlib.Path(sys.argv[1]).resolve()
    failed = False
    for name, key in BENCHMARKS:
        # A benchmark the baseline does not carry cannot be compared.
        if not (root / "benches" / f"{name}.rs").exists():
            print(f"{name}: absent from the baseline, skipped")
            continue
        if compare(root, name, key):
            failed = True
    return failed


if __name__ == "__main__":
    sys.exit(main())
