# Nylon

Native digital audio workstation under development for macOS, Windows, and Linux.
The C++ frontend uses Qt Widgets. Rust provides the project command history,
sample clock, stereo gain rendering, sample-offset parameter events, and bounded
thread communication. The frontend links through a C interface without a Rust Qt
binding. Audio device playback and project persistence are not implemented yet.

## Build

Install the Rust toolchain listed in `rust-toolchain.toml` and Python 3.
Run commands from the repository root:

```sh
python3 -B scripts/bootstrap.py
sh scripts/lint-rules.sh
python3 -B scripts/rt_lint.py
python3 -B -m unittest discover -s scripts -p 'test_*.py'
cargo fmt --check
cargo build --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo bench --locked --bench render
cargo deny check
```

## Desktop

Install Qt 6 with the Widgets and Test components, a C++17 compiler, and CMake.
Use the installed native Qt package. Build the core before the desktop target:

```sh
cargo build --locked --release
cmake -S desktop -B target/desktop -DCMAKE_BUILD_TYPE=Release
cmake --build target/desktop --config Release --parallel 2
ctest --test-dir target/desktop -C Release --output-on-failure
```

If Qt is not in the package search path, pass its runtime-discovered prefix as
`CMAKE_PREFIX_PATH` when configuring CMake. macOS produces `Nylon.app` in the
desktop build directory. Windows and Linux produce the native executable.

The initial desktop supports tempo edits, adding audio tracks, undo/redo,
Session/Arrangement navigation, and four themes. Clip editing and playback are
not connected yet.

`cargo-deny` is a separate development prerequisite. CI installs version 0.20.2.
The native compiler wrapper maps build and toolchain locations to relative
diagnostic paths on macOS, Windows, and Linux. Rebuild it after cleaning `target`.
The repository hook directory is `.githooks`.

License: MIT. Copyright (c) Nylon Contributors.
