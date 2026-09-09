# Nylon

Native digital audio workstation core for macOS, Windows, and Linux, written in
Rust with C and C++ interfaces. It carries the project model and its command
history, the mixer, the transport, the instruments and note scheduling, device
playback, and project persistence. A front end links it through the C interface
without a Rust binding of its own.

A Qt front end was written against it and is parked in `failedAttempts/`. The
work now is the core.

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

## Command line

`cli/` holds a headless tool that drives the core: it creates and opens
project bundles, edits tracks and clips, imports audio, bounces, and lists
output devices. It needs Qt 6 Core for its argument and reply handling, a
C++17 compiler, and CMake.

```sh
cargo build --locked --release
cmake -S cli -B target/cli -DCMAKE_BUILD_TYPE=Release
cmake --build target/cli --parallel 2
ctest --test-dir target/cli --output-on-failure
```

`cargo-deny` is a separate development prerequisite. CI installs version 0.20.2.
The native compiler wrapper maps build and toolchain locations to relative
diagnostic paths on macOS, Windows, and Linux. Rebuild it after cleaning `target`.
The repository hook directory is `.githooks`.

License: MIT. Copyright (c) Nylon Contributors.
