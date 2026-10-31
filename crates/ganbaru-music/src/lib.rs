//! Music domain contracts independent of Tauri and the WebView: per-phase context assignments
//! for projects, Calendar events, and work environments, the Music library error shape, and the
//! derived row keys of Music tables with composite primary keys.
//! Callers supply an authorized vault pool or transaction.

pub mod assignments;
pub mod error;
pub mod sync_keys;
