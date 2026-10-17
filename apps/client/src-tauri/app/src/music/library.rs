//! Music library command adapters over `ganbaru-music-library` and device-local root bindings.

pub(crate) mod commands;
#[cfg(target_os = "android")]
mod mobile_refresh;
