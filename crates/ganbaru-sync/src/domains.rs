//! Built-in domain adapters.

use crate::adapter::Adapters;

pub mod music;
pub mod quick_notes;

/// The adapters the vault manifest names, one line per domain.
pub fn vault_adapters() -> Adapters {
    Adapters::new().with(Box::new(quick_notes::QuickNotesAdapter))
}
