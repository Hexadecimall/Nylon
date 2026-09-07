# Nylon

Native digital audio workstation under development. The current library contains
a sample clock, stereo gain rendering, and sample-offset parameter events.
It does not yet provide a desktop application or an audio device backend.

## Build

Install the Rust toolchain listed in `rust-toolchain.toml` and Python 3.
Run commands from the repository root:

```sh
python3 -B scripts/bootstrap.py
sh scripts/lint-rules.sh
python3 -B -m unittest discover -s scripts -p 'test_*.py'
cargo fmt --check
cargo build --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo bench --locked --bench render
cargo deny check
```

`cargo-deny` is a separate development prerequisite. CI installs version 0.20.2.
The native compiler wrapper maps build and toolchain locations to relative
diagnostic paths on macOS, Windows, and Linux. Rebuild it after cleaning `target`.
The repository hook directory is `.githooks`.

License: MIT. Copyright (c) Nylon Contributors.
