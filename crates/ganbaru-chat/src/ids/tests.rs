use super::*;
use ganbaru_sync_contracts::bounds::MAX_ROW_KEY_BYTES;
use std::collections::BTreeSet;

#[test]
fn new_id_is_prefix_and_32_lowercase_hex_digits() {
    let id = new_id("conversation-item").expect("random id");
    let (prefix, digits) = id.split_once(':').expect("separator");

    assert_eq!(prefix, "conversation-item");
    assert_eq!(digits.len(), 32);
    assert!(
        digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert!(id.len() <= MAX_ROW_KEY_BYTES);
}

#[test]
fn new_id_does_not_repeat() {
    let ids: BTreeSet<String> = (0..256)
        .map(|_| new_id("message-revision").expect("random id"))
        .collect();

    assert_eq!(ids.len(), 256);
}

#[test]
fn derived_id_equals_derived_key_for_the_same_parts() {
    let parts = ["conversation-item:00112233445566778899aabbccddeeff"];
    let expected = derived_key(REPLY_THREAD_ID_PREFIX, &parts)
        .expect("derived key")
        .into_string();

    assert_eq!(
        derived_id(REPLY_THREAD_ID_PREFIX, &parts).expect("derived id"),
        expected
    );
    assert_eq!(
        derived_id(REPLY_THREAD_ID_PREFIX, &parts).expect("derived id"),
        derived_id(REPLY_THREAD_ID_PREFIX, &parts).expect("derived id")
    );
    assert_ne!(
        derived_id(ASSIGNMENT_INPUT_ID_PREFIX, &parts).expect("derived id"),
        expected
    );
}

#[test]
fn derived_id_rejects_invalid_inputs() {
    assert!(derived_id("Reply Thread", &["root"]).is_err());
    assert!(derived_id(REPLY_THREAD_ID_PREFIX, &[]).is_err());
    assert!(derived_id(REPLY_THREAD_ID_PREFIX, &["root\0item"]).is_err());
}
