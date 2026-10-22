//! Built-in domain adapters and materialization hooks.

use crate::adapter::Adapters;
use crate::hooks::Hooks;
use crate::manifest::vault::preferences::PREFERENCES_TABLE;

pub mod preferences;
pub mod quick_notes;

/// The adapters the vault manifest names, one line per domain.
pub fn vault_adapters() -> Adapters {
    Adapters::new()
        .with(Box::new(quick_notes::QuickNotesAdapter))
        .with(Box::new(preferences::PreferencesAdapter))
}

/// The hooks of the vault manifest's tables, one line per hooked table.
pub fn vault_hooks() -> Hooks {
    Hooks::new().with(PREFERENCES_TABLE, Box::new(preferences::PreferencesHook))
}
