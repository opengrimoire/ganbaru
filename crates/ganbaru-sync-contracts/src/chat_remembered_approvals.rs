//! Keys and bounds of Chat remembered approvals.
//!
//! A remembered approval lets an agent repeat one kind of request in one channel without asking
//! again. Sync admission and the Chat commands both read these, so an approval two devices make
//! for one channel, agent, and subject is one row.

/// Prefix of remembered approval keys, over the conversation, agent, subject kind, and subject key.
pub const REMEMBERED_APPROVAL_KEY_PREFIX: &str = "remembered-approval";
/// Longest subject key an approval names, in bytes.
pub const MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES: usize = 2_048;
/// Subject kind of an approval that covers every page of one web origin.
pub const SUBJECT_KIND_URL_ORIGIN: &str = "url_origin";
/// Subject kind of an approval that covers one exact command.
pub const SUBJECT_KIND_COMMAND: &str = "command";
/// Subject kind of an approval that covers every call of one tool.
pub const SUBJECT_KIND_TOOL: &str = "tool";
/// Every subject kind a remembered approval may name.
pub const SUBJECT_KINDS: [&str; 3] = [
    SUBJECT_KIND_URL_ORIGIN,
    SUBJECT_KIND_COMMAND,
    SUBJECT_KIND_TOOL,
];

/// Whether a subject key is within bounds and free of control characters.
///
/// This is the structural rule sync admission shares with the Chat commands. The Chat commands
/// derive keys with stricter rules of their own, so a key that passes here but that no device
/// would derive never matches a request.
#[must_use]
pub fn subject_key_is_well_formed(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES
        && !key.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_keys_are_bounded_and_printable() {
        assert!(subject_key_is_well_formed("https://example.com"));
        assert!(subject_key_is_well_formed(
            &"a".repeat(MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES)
        ));
        assert!(!subject_key_is_well_formed(""));
        assert!(!subject_key_is_well_formed(
            &"a".repeat(MAX_REMEMBERED_APPROVAL_SUBJECT_BYTES + 1)
        ));
        assert!(!subject_key_is_well_formed("npm test\nrm -rf ."));
        assert!(!subject_key_is_well_formed("npm\u{7}"));
    }
}
