//! Derived row keys for Calendar rows identified by other rows.
//!
//! Every device derives the same key from the same identity, so the same link written on two
//! devices addresses one row.

use ganbaru_sync_contracts::derived_key;

/// Prefix of archive import object keys, over the archive and the iCalendar object.
pub const ARCHIVE_IMPORT_OBJECT_KEY_PREFIX: &str = "archive-import-object";

/// Key of the link between an archived event and an iCalendar object it preserves.
pub fn archive_import_object_key(
    archive_event_id: &str,
    object_id: &str,
) -> Result<String, String> {
    derived_key(
        ARCHIVE_IMPORT_OBJECT_KEY_PREFIX,
        &[archive_event_id, object_id],
    )
    .map(|key| key.as_str().to_string())
    .map_err(|error| format!("derive {ARCHIVE_IMPORT_OBJECT_KEY_PREFIX} key: {error}"))
}

#[cfg(test)]
mod tests {
    use ganbaru_sync::manifest::vault::calendar;

    #[test]
    fn archive_object_key_prefix_matches_the_sync_manifest() {
        assert_eq!(
            super::ARCHIVE_IMPORT_OBJECT_KEY_PREFIX,
            calendar::ARCHIVE_OBJECT_KEY_PREFIX
        );
    }
}
