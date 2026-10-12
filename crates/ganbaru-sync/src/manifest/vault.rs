//! The vault manifest.

use super::{Manifest, TableSpec};

pub mod quick_notes;

/// Current vault manifest version.
pub const VAULT_MANIFEST_VERSION: u16 = 1;

/// Replicated vault tables, parents first.
static TABLES: [TableSpec; 2] = [quick_notes::TAGS, quick_notes::NOTES];

/// The vault manifest.
pub static VAULT_MANIFEST: Manifest = Manifest {
    version: VAULT_MANIFEST_VERSION,
    tables: &TABLES,
    derived: &["quick_notes_search_fts"],
    engine: &[
        "sync_spaces",
        "sync_apply_state",
        "sync_capture",
        "sync_writers",
        "sync_ops",
        "sync_rows",
        "sync_register_versions",
        "sync_tombstones",
    ],
};
