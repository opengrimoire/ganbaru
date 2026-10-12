//! Declares platform cfg aliases for the app composition crate.
//!
//! `mobile` matches Android and iOS. `desktop` matches every other target, which mirrors the
//! `cfg(not(any(target_os = "android", target_os = "ios")))` dependency tables in `Cargo.toml`
//! so code gated on `desktop` always has the desktop-only dependencies available.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(target_os.as_str(), "android" | "ios") {
        println!("cargo:rustc-cfg=mobile");
    } else {
        println!("cargo:rustc-cfg=desktop");
    }
}
