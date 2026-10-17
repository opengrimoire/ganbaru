//! Declares platform cfg aliases for the handoff crate.
//!
//! `mobile` matches Android and iOS. `desktop` matches every other target, the same split the
//! app composition crate uses, so handoff code gated on either alias compiles for the same
//! targets as its callers.

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
