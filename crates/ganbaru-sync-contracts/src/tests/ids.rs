use super::fixtures::{other_writer_key, writer_key};
use crate::bounds::MAX_ROW_KEY_BYTES;
use crate::ids::{hex, parse_hex};
use crate::{GroupId, GroupMask, RowKey, SpaceId, WriterId};

#[test]
fn personal_space_ids_are_deterministic_per_vault() {
    assert_eq!(SpaceId::personal("vault-a"), SpaceId::personal("vault-a"));
    assert_ne!(SpaceId::personal("vault-a"), SpaceId::personal("vault-b"));
}

#[test]
fn writer_ids_derive_from_public_keys() {
    let first = WriterId::for_public_key(&writer_key().public_key());
    let second = WriterId::for_public_key(&other_writer_key().public_key());
    assert_ne!(first, second);
    assert_eq!(first, WriterId::for_public_key(&writer_key().public_key()));
    assert_ne!(first.as_bytes(), SpaceId::personal("vault-a").as_bytes());
}

#[test]
fn ids_roundtrip_through_hex() {
    let id = WriterId::for_public_key(&writer_key().public_key());
    let text = id.to_hex();
    assert_eq!(text.len(), 32);
    assert_eq!(WriterId::from_hex(&text), Some(id));
    assert_eq!(WriterId::from_hex(&text.to_uppercase()), Some(id));
    assert_eq!(WriterId::from_hex(&text[1..]), None);
    assert_eq!(WriterId::from_hex(&format!("g{}", &text[1..])), None);
    assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    assert_eq!(parse_hex::<3>("00abff"), Some([0x00, 0xab, 0xff]));
}

#[test]
fn row_keys_match_the_domain_id_rule() {
    assert!(RowKey::new("note-1").is_ok());
    assert!(RowKey::new("x".repeat(MAX_ROW_KEY_BYTES)).is_ok());
    assert!(RowKey::new("x".repeat(MAX_ROW_KEY_BYTES + 1)).is_err());
    assert!(RowKey::new("").is_err());
    assert!(RowKey::new("   ").is_err());
    // SQLite `trim` removes only spaces, so other whitespace keys are locally valid.
    assert!(RowKey::new("\t").is_ok());
    assert!(RowKey::new("note\0").is_err());
}

#[test]
fn group_masks_iterate_in_ascending_order() {
    assert_eq!(GroupId::new(63).map(GroupId::get), Some(63));
    assert_eq!(GroupId::new(64), None);
    let mut mask = GroupMask::EMPTY;
    for id in [5, 0, 63] {
        mask.insert(GroupId::new(id).unwrap());
    }
    let ids: Vec<u8> = mask.iter().map(GroupId::get).collect();
    assert_eq!(ids, vec![0, 5, 63]);
    assert_eq!(mask.len(), 3);
    assert!(mask.contains(GroupId::new(5).unwrap()));
    assert!(!mask.contains(GroupId::new(6).unwrap()));
    assert_eq!(GroupMask::first(8).0, 0xff);
    assert_eq!(GroupMask::first(64).0, u64::MAX);
    assert!(GroupMask::EMPTY.is_empty());
}
