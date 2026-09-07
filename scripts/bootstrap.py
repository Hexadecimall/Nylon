"""Compile the native compiler wrapper on the build host."""
import os
import pathlib
import subprocess

root = pathlib.Path.cwd()
target = pathlib.Path("target")
target.mkdir(exist_ok=True)
executable = target / ("pathmap.exe" if os.name == "nt" else "pathmap")
subprocess.run([
    "rustc", "--edition=2024", "scripts/pathmap.rs", "-o", str(executable),
    "--remap-path-prefix=" + str(root) + "=.",
    "--remap-path-prefix=" + str(pathlib.Path.home()) + "=host",
], check=True)
