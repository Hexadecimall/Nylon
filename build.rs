//! Links the platform frameworks the audio backend calls into.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // Set on every platform that has an output backend, so the runtime
    // does not repeat the list of them at each use.
    println!("cargo::rustc-check-cfg=cfg(platform_audio)");
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("macos" | "linux" | "windows")
    ) {
        println!("cargo::rustc-cfg=platform_audio");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        // The output unit and the device queries live in these two.
        println!("cargo::rustc-link-lib=framework=AudioToolbox");
        println!("cargo::rustc-link-lib=framework=CoreAudio");
        println!("cargo::rustc-link-lib=framework=CoreFoundation");
        // Cargo otherwise records the build directory as the dynamic
        // library identity. Consumers resolve the shipped core beside
        // the application through their runtime search path.
        println!("cargo::rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libnylon.dylib");
    }
}
