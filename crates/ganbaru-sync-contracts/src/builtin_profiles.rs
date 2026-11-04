//! Identity of the built-in Chat access profiles.
//!
//! Every vault seeds these rows under fixed ids. Sync admission and the Chat seed definitions
//! read this table, so a built-in key stays bound to the one row seeded with it.

/// The seeded access profiles as row id and built-in key.
pub const BUILTIN_ACCESS_PROFILES: [(&str, &str); 5] = [
    ("access-profile:conversation-only", "conversation_only"),
    ("access-profile:read-only", "read_only"),
    ("access-profile:edit-files", "edit_files"),
    ("access-profile:build-and-test", "build_and_test"),
    ("access-profile:publish-changes", "publish_changes"),
];

/// The built-in key of the seeded profile `id`, or `None` for any other id.
pub fn builtin_profile_key(id: &str) -> Option<&'static str> {
    BUILTIN_ACCESS_PROFILES
        .iter()
        .find(|(seeded, _)| *seeded == id)
        .map(|(_, key)| *key)
}
