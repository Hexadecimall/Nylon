use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(compiler) = args.next() else { return ExitCode::FAILURE };
    let mut command = Command::new(compiler);
    command.args(args);
    let Ok(root) = env::current_dir() else { return ExitCode::FAILURE };
    command.arg(format!("--remap-path-prefix={}={}", root.display(), "."));
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE"));
    for (variable, folder, replacement) in [
        ("CARGO_HOME", ".cargo", "cargo"),
        ("RUSTUP_HOME", ".rustup", "rust"),
    ] {
        let path = env::var_os(variable).map(PathBuf::from)
            .or_else(|| home.as_ref().map(|value| PathBuf::from(value).join(folder)));
        if let Some(path) = path {
            command.arg(format!("--remap-path-prefix={}={replacement}", path.display()));
        }
    }
    match command.status() {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1) as u8),
        Err(_) => ExitCode::FAILURE,
    }
}
