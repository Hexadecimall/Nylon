use nylon::plugin::probe::{inspect_clap, write_protocol};
use std::io::{self, Write};
use std::path::Path;

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.len() != 2 || arguments[0] != "clap" {
        fail("Usage: nylon-plugin-probe clap <plugin>");
    }
    match inspect_clap(Path::new(&arguments[1])) {
        Ok(descriptors) => {
            if write_protocol(&descriptors, io::stdout().lock()).is_err() {
                fail("Could not write probe output");
            }
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn fail(message: &str) -> ! {
    let mut error = io::stderr().lock();
    let _ = writeln!(error, "{message}");
    std::process::exit(1)
}
