//! Links the platform frameworks the audio backend calls into.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // The output unit and the device queries live in these two.
        println!("cargo::rustc-link-lib=framework=AudioToolbox");
        println!("cargo::rustc-link-lib=framework=CoreAudio");
        println!("cargo::rustc-link-lib=framework=CoreFoundation");
    }
}
