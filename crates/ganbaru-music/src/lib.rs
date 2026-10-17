//! Music domain contracts independent of Tauri and the WebView: per-phase context assignments
//! for projects, Calendar events, and work environments, and the Music library error shape.
//! Callers supply an authorized vault pool or transaction.

pub mod assignments;
pub mod error;
