//! Keys of Chat agents a person named.
//!
//! Sync admission and the Chat agent removal both read these, so a deletion one replica seals is
//! a deletion every replica accepts.

/// What the random id of an agent a person named starts with.
pub const NAMED_AGENT_KEY_PREFIX: &str = "agent:";
/// Lowercase hexadecimal digits after [`NAMED_AGENT_KEY_PREFIX`] in a named agent's id.
pub const NAMED_AGENT_KEY_DIGITS: usize = 32;

/// Whether `key` is the random id of an agent a person named: [`NAMED_AGENT_KEY_PREFIX`]
/// followed by [`NAMED_AGENT_KEY_DIGITS`] lowercase hexadecimal digits.
///
/// Only such an agent may be deleted. The local person keeps the participant id the vault seeds,
/// and a built-in agent's id is derived from its provider, so a device that detects the provider
/// again would recreate it under a key a published deletion keeps from coming back.
pub fn is_named_agent_key(key: &str) -> bool {
    key.strip_prefix(NAMED_AGENT_KEY_PREFIX)
        .is_some_and(|digits| {
            digits.len() == NAMED_AGENT_KEY_DIGITS
                && digits
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_random_agent_id_names_a_deletable_agent() {
        assert!(is_named_agent_key("agent:0123456789abcdef0123456789abcdef"));
        for key in [
            "participant:local-owner",
            "agent:0123456789ABCDEF0123456789ABCDEF",
            "agent:0123456789abcdef0123456789abcde",
            "agent:0123456789abcdef0123456789abcdef0",
            "agent:0123456789abcdef0123456789abcdeg",
            "agent-model:0123456789abcdef0123456789abcdef",
            "agent-catalog:0123456789abcdef0123456789abcdef",
            "agent:",
        ] {
            assert!(!is_named_agent_key(key), "{key}");
        }
    }
}
