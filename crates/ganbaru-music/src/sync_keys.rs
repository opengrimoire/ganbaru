//! Row keys of the Music tables whose primary key is composite.
//!
//! Each such table carries a `sync_key` column, its replication row key, which is
//! [`ganbaru_sync_contracts::derived_key`] over a per-table prefix and the primary key columns in
//! declaration order. Writers fill it on insert; every device computes the same key for the same
//! row, so concurrent adds of one row converge.

use crate::error::{MusicLibraryError, MusicLibraryResult};
use ganbaru_sync_contracts::derived_key;

/// Column that holds the derived row key.
pub const SYNC_KEY_COLUMN: &str = "sync_key";

/// One table keyed by a derived `sync_key`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositeKeyTable {
    /// SQLite table name.
    pub table: &'static str,
    /// Derived key prefix.
    pub prefix: &'static str,
    /// Primary key columns in declaration order, the derived key parts.
    pub columns: &'static [&'static str],
}

/// Item signals, keyed by item and signal.
pub const SIGNALS: CompositeKeyTable = CompositeKeyTable {
    table: "music_item_signals",
    prefix: "music-signal",
    columns: &["item_id", "signal"],
};

/// Listening statistics, keyed by device and item.
pub const STATISTICS: CompositeKeyTable = CompositeKeyTable {
    table: "music_listening_statistics",
    prefix: "music-statistics",
    columns: &["device_id", "item_id"],
};

/// Source collection items, keyed by collection and item.
pub const COLLECTION_ITEMS: CompositeKeyTable = CompositeKeyTable {
    table: "music_source_collection_items",
    prefix: "music-collection-item",
    columns: &["collection_id", "item_id"],
};

/// Playlist intended uses, keyed by playlist and use.
pub const INTENDED_USES: CompositeKeyTable = CompositeKeyTable {
    table: "music_playlist_intended_uses",
    prefix: "music-intended-use",
    columns: &["playlist_id", "intended_use"],
};

/// Soundscape preset layers, keyed by preset and soundscape.
pub const PRESET_SOUNDS: CompositeKeyTable = CompositeKeyTable {
    table: "music_soundscape_preset_sounds",
    prefix: "music-preset-sound",
    columns: &["preset_id", "soundscape_id"],
};

/// Context assignments, keyed by owner kind, owner, and phase.
pub const ASSIGNMENTS: CompositeKeyTable = CompositeKeyTable {
    table: "music_context_assignments",
    prefix: "music-assignment",
    columns: &["owner_kind", "owner_id", "phase"],
};

/// Every Music table keyed by a derived `sync_key`.
pub const COMPOSITE_KEY_TABLES: [CompositeKeyTable; 6] = [
    SIGNALS,
    STATISTICS,
    COLLECTION_ITEMS,
    INTENDED_USES,
    PRESET_SOUNDS,
    ASSIGNMENTS,
];

impl CompositeKeyTable {
    /// The `sync_key` of the row whose primary key columns hold `parts`, in declaration order.
    pub fn key(&self, parts: &[&str]) -> MusicLibraryResult<String> {
        if parts.len() != self.columns.len() {
            return Err(MusicLibraryError::runtime(
                "derive music row key",
                format!(
                    "{} takes {} key parts, not {}",
                    self.table,
                    self.columns.len(),
                    parts.len()
                ),
            ));
        }
        derived_key(self.prefix, parts)
            .map(ganbaru_sync_contracts::RowKey::into_string)
            .map_err(|error| {
                MusicLibraryError::validation(
                    SYNC_KEY_COLUMN,
                    format!("{} key: {error}", self.table),
                )
            })
    }
}

/// The `sync_key` of an item signal.
pub fn signal_key(item_id: &str, signal: &str) -> MusicLibraryResult<String> {
    SIGNALS.key(&[item_id, signal])
}

/// The `sync_key` of the listening statistics of `item_id` on `device_id`.
pub fn statistics_key(device_id: &str, item_id: &str) -> MusicLibraryResult<String> {
    STATISTICS.key(&[device_id, item_id])
}

/// The `sync_key` of `item_id` in a source collection.
pub fn collection_item_key(collection_id: &str, item_id: &str) -> MusicLibraryResult<String> {
    COLLECTION_ITEMS.key(&[collection_id, item_id])
}

/// The `sync_key` of an intended use of a playlist.
pub fn intended_use_key(playlist_id: &str, intended_use: &str) -> MusicLibraryResult<String> {
    INTENDED_USES.key(&[playlist_id, intended_use])
}

/// The `sync_key` of a soundscape layer of a preset.
pub fn preset_sound_key(preset_id: &str, soundscape_id: &str) -> MusicLibraryResult<String> {
    PRESET_SOUNDS.key(&[preset_id, soundscape_id])
}

/// The `sync_key` of the assignment of one phase to one owner.
pub fn assignment_key(owner_kind: &str, owner_id: &str, phase: &str) -> MusicLibraryResult<String> {
    ASSIGNMENTS.key(&[owner_kind, owner_id, phase])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hash_the_prefix_and_primary_key_parts_in_order() {
        let key = statistics_key("device-a", "item-1").unwrap();
        assert!(key.starts_with("music-statistics:"));
        assert_eq!(key, statistics_key("device-a", "item-1").unwrap());
        assert_ne!(key, statistics_key("item-1", "device-a").unwrap());
        assert_eq!(
            preset_sound_key("preset-generated-white-noise", "generated-white-noise").unwrap(),
            "music-preset-sound:92d6ae2884258f01f7a9208ca6e6cb93"
        );
    }

    #[test]
    fn keys_refuse_wrong_part_counts_and_nul_parts() {
        assert!(SIGNALS.key(&["item"]).is_err());
        assert!(assignment_key("project-default", "a\0b", "focus").is_err());
    }

    #[test]
    fn every_table_has_a_distinct_prefix() {
        let mut prefixes: Vec<_> = COMPOSITE_KEY_TABLES
            .iter()
            .map(|table| table.prefix)
            .collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), COMPOSITE_KEY_TABLES.len());
    }
}
