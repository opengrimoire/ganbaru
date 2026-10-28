//! Organizational Chat row identifiers.
//!
//! Rows created on one device and replicated to others need ids that never collide across
//! devices, so random ids come from the operating system's secure generator instead of clocks or
//! counters. Rows whose identity is their content (a reply thread per root message, an input per
//! message) use derived ids, so every device that creates the row agrees on its id.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_sync_contracts::derived_key;
use std::fmt::Write as _;

/// Number of random bytes behind every random id.
const RANDOM_ID_BYTES: usize = 16;

/// Derived id prefix of a reply thread, keyed by its root conversation item id.
pub const REPLY_THREAD_ID_PREFIX: &str = "reply-thread";
/// Derived id prefix of a work assignment input, keyed by its message item id.
pub const ASSIGNMENT_INPUT_ID_PREFIX: &str = "assignment-input";
/// Derived id prefix of a work assignment, keyed by its triggering message, its teammate, and
/// the assignment it follows when it has one.
pub const WORK_ASSIGNMENT_ID_PREFIX: &str = "work-assignment";

/// Returns `{prefix}:{32 lowercase hex}` built from 16 bytes of the operating system's secure
/// random generator.
///
/// # Errors
///
/// Returns a persistence error when the secure random generator is unavailable.
pub fn new_id(prefix: &str) -> ChatResult<String> {
    let mut bytes = [0_u8; RANDOM_ID_BYTES];
    ganbaru_atomic_files::fill_random(&mut bytes).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "The secure random generator is unavailable",
            true,
        )
    })?;
    let mut id = String::with_capacity(prefix.len() + 1 + RANDOM_ID_BYTES * 2);
    id.push_str(prefix);
    id.push(':');
    for byte in bytes {
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}

/// Returns the id derived from `parts` under `prefix`, equal to
/// [`ganbaru_sync_contracts::derived_key`] for the same arguments.
///
/// # Errors
///
/// Returns a validation error when `prefix` is not a valid derived key prefix, `parts` is empty,
/// or a part contains a NUL character.
pub fn derived_id(prefix: &str, parts: &[&str]) -> ChatResult<String> {
    derived_key(prefix, parts)
        .map(|key| key.into_string())
        .map_err(|_| ChatError::validation("id", "The derived Chat id inputs are invalid"))
}

#[cfg(test)]
mod tests;
