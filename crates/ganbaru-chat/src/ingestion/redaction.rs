//! Exact-value redaction of secrets a person provided to a run, applied before Chat persists
//! any text that came from the run's provider.
//!
//! A registered value is replaced by `[secret: <label>]` wherever it occurs whole. Streamed
//! text arrives in pieces, so the ingestor also holds back the end of a piece while it could
//! still be the start of a registered value; see [`held_tail_len`].

use crate::credentials::SecretValue;
use crate::models::ChatThreadId;
use serde_json::{Map, Value};
use std::sync::Arc;

/// A secret value a person provided for one request, with the label that replaces it.
pub struct RegisteredSecret {
    marker: String,
    value: SecretValue,
}

impl RegisteredSecret {
    /// Registers `value` under the request `label` the teammate gave.
    pub fn new(label: &str, value: SecretValue) -> Self {
        Self {
            marker: format!("[secret: {label}]"),
            value,
        }
    }

    /// The text that stands for the value in persisted text.
    pub fn marker(&self) -> &str {
        &self.marker
    }

    fn value(&self) -> &str {
        self.value.expose()
    }
}

/// Source of the secrets registered for the runs of one provider thread.
pub trait SecretRedactor: Send + Sync {
    /// The secrets provided to runs on `thread_id`. Order does not matter.
    fn registered(&self, thread_id: &ChatThreadId) -> Vec<Arc<RegisteredSecret>>;
}

/// Orders `secrets` longest value first, so a value that contains another is replaced whole.
pub fn longest_first(mut secrets: Vec<Arc<RegisteredSecret>>) -> Vec<Arc<RegisteredSecret>> {
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.value().len()));
    secrets
}

/// Returns `text` with every whole occurrence of a registered value replaced by its marker, or
/// `None` when no value occurs. `secrets` must be ordered by [`longest_first`].
pub fn redact_text(text: &str, secrets: &[Arc<RegisteredSecret>]) -> Option<String> {
    let mut redacted: Option<String> = None;
    for secret in secrets {
        let current = redacted.as_deref().unwrap_or(text);
        if current.contains(secret.value()) {
            redacted = Some(current.replace(secret.value(), secret.marker()));
        }
    }
    redacted
}

/// Redacts every string and object key inside `value`, returning whether anything changed.
/// `secrets` must be ordered by [`longest_first`].
pub fn redact_json(value: &mut Value, secrets: &[Arc<RegisteredSecret>]) -> bool {
    match value {
        Value::String(text) => match redact_text(text, secrets) {
            Some(redacted) => {
                *text = redacted;
                true
            }
            None => false,
        },
        Value::Array(values) => {
            // Every element is redacted, so this must not stop at the first change.
            let mut changed = false;
            for value in values {
                changed |= redact_json(value, secrets);
            }
            changed
        }
        Value::Object(fields) => {
            let mut changed = false;
            let mut redacted = Map::with_capacity(fields.len());
            for (key, mut field) in std::mem::take(fields) {
                changed |= redact_json(&mut field, secrets);
                let key = match redact_text(&key, secrets) {
                    Some(redacted_key) => {
                        changed = true;
                        redacted_key
                    }
                    None => key,
                };
                redacted.insert(key, field);
            }
            *fields = redacted;
            changed
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Bytes at the end of `text` that are a proper prefix of a registered value, so the next
/// piece of the same stream could complete it. The ingestor holds them until it knows.
///
/// The returned length always starts a character, because a value prefix starts with the
/// value's first character.
pub fn held_tail_len(text: &str, secrets: &[Arc<RegisteredSecret>]) -> usize {
    secrets
        .iter()
        .map(|secret| longest_suffix_prefix(text.as_bytes(), secret.value().as_bytes()))
        .max()
        .unwrap_or(0)
}

/// Length of the longest suffix of `text` that is a proper prefix of `pattern`, found with
/// the Knuth, Morris, and Pratt automaton over the last `pattern.len() - 1` bytes of `text`,
/// so the cost stays linear in the value length whatever the text holds.
fn longest_suffix_prefix(text: &[u8], pattern: &[u8]) -> usize {
    if pattern.len() < 2 {
        return 0;
    }
    let failure = prefix_function(pattern);
    let start = text.len().saturating_sub(pattern.len() - 1);
    let mut state = 0;
    for &byte in &text[start..] {
        while state > 0 && pattern[state] != byte {
            state = failure[state - 1];
        }
        if pattern[state] == byte {
            state += 1;
        }
    }
    state
}

/// The prefix function of `pattern`: for each position, the length of the longest proper
/// prefix that is also a suffix of the pattern up to that position.
fn prefix_function(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0; pattern.len()];
    let mut length = 0;
    for index in 1..pattern.len() {
        while length > 0 && pattern[index] != pattern[length] {
            length = failure[length - 1];
        }
        if pattern[index] == pattern[length] {
            length += 1;
        }
        failure[index] = length;
    }
    failure
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn secrets(values: &[(&str, &str)]) -> Vec<Arc<RegisteredSecret>> {
        longest_first(
            values
                .iter()
                .map(|(label, value)| {
                    Arc::new(RegisteredSecret::new(
                        label,
                        SecretValue::new(*value).unwrap(),
                    ))
                })
                .collect(),
        )
    }

    #[test]
    fn every_whole_occurrence_is_replaced_by_its_label() {
        let registered = secrets(&[("token", "s3cr3t-value")]);
        assert_eq!(
            redact_text("a s3cr3t-value and s3cr3t-value", &registered).as_deref(),
            Some("a [secret: token] and [secret: token]")
        );
        assert_eq!(redact_text("nothing here", &registered), None);
    }

    #[test]
    fn a_value_containing_another_is_replaced_whole() {
        let registered = secrets(&[("short", "abcdef"), ("long", "xxabcdefyy")]);
        assert_eq!(
            redact_text("xxabcdefyy then abcdef", &registered).as_deref(),
            Some("[secret: long] then [secret: short]")
        );
    }

    #[test]
    fn json_strings_and_keys_are_redacted_at_every_depth() {
        let registered = secrets(&[("key", "hunter22")]);
        let mut value = json!({
            "output": ["ok", {"nested": "pw=hunter22"}],
            "hunter22": 1,
            "count": 3
        });
        assert!(redact_json(&mut value, &registered));
        assert_eq!(
            value,
            json!({
                "output": ["ok", {"nested": "pw=[secret: key]"}],
                "[secret: key]": 1,
                "count": 3
            })
        );
        assert!(!redact_json(&mut value, &registered));
    }

    #[test]
    fn a_tail_that_could_start_a_value_is_held() {
        let registered = secrets(&[("key", "abcabd")]);
        assert_eq!(held_tail_len("text abcab", &registered), 5);
        assert_eq!(held_tail_len("text abca", &registered), 4);
        assert_eq!(held_tail_len("text abc", &registered), 3);
        assert_eq!(held_tail_len("text ab", &registered), 2);
        assert_eq!(held_tail_len("text a", &registered), 1);
        assert_eq!(held_tail_len("text abx", &registered), 0);
        assert_eq!(held_tail_len("", &registered), 0);
    }

    #[test]
    fn a_held_tail_starts_a_character() {
        let registered = secrets(&[("key", "日本語の鍵")]);
        let text = "前置き日本";
        let held = held_tail_len(text, &registered);
        assert_eq!(&text[text.len() - held..], "日本");
    }

    #[test]
    fn the_tail_check_reads_only_the_value_length() {
        let value = "a".repeat(16 * 1024);
        let registered = secrets(&[("big", &value)]);
        let text = "a".repeat(4 * 1024 * 1024);
        assert_eq!(held_tail_len(&text, &registered), value.len() - 1);
    }
}
