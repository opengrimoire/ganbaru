//! Declares the `desktop` platform cfg alias for the Music library crate.
//!
//! `desktop` matches every target except Android and iOS, which mirrors the
//! `cfg(not(any(target_os = "android", target_os = "ios")))` dependency table in `Cargo.toml`
//! so code gated on `desktop` always has the desktop-only dependencies available.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if !matches!(target_os.as_str(), "android" | "ios") {
        println!("cargo:rustc-cfg=desktop");
    }
}
