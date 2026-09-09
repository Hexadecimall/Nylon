"""Run native tests and collect explicit Qt logs when a platform hides stdout."""
import os
import pathlib
import subprocess
import sys

root = pathlib.Path("target/desktop")
result = subprocess.run([
    "ctest", "--test-dir", str(root), "-C", "Release", "--output-on-failure",
], check=False)
if result.returncode:
    names = sorted(root.rglob("test_*.exe")) if os.name == "nt" else sorted(root.glob("test_*"))
    for executable in names:
        if not executable.is_file():
            continue
        log = root / (executable.stem + "-diagnostic.txt")
        diagnostic = subprocess.run([
            str(executable.resolve()), "-o", str(log.resolve()) + ",txt",
        ], env={**os.environ, "QT_QPA_PLATFORM": "offscreen"}, check=False)
        print(f"{executable.name}: diagnostic exit {diagnostic.returncode}", flush=True)
        if log.exists():
            print(log.read_text(encoding="utf-8", errors="replace"), flush=True)
sys.exit(result.returncode)
