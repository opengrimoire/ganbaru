use super::fixtures::{other_writer_key, writer_key};
use crate::bounds::MAX_ROW_KEY_BYTES;
use crate::ids::{MAX_DERIVED_KEY_PREFIX_BYTES, hex, parse_hex};
use crate::{
    DerivedKeyError, GroupId, GroupMask, PENDING_KEY_PREFIX, RowKey, SpaceId, WriterId, derived_key,
};

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

#[test]
fn derived_keys_match_their_vectors() {
    let cases: [(&str, &[&str], &str); 5] = [
        ("m", &["a"], "m:ca978112ca1bbdcafac231b39a23dc4d"),
        (
            "membership",
            &["item-1", "end-1"],
            "membership:e03f0ce26f0252325e779592dc48e1f9",
        ),
        (
            "edge_2",
            &["a", "b", "c"],
            "edge_2:8badde10c760e9b702defb4b5e225de7",
        ),
        ("x", &["a", "", "b"], "x:2d8dbcddf40617a662bcf1f1c70eb523"),
        ("u-n", &["\u{e9}"], "u-n:4a99557e4033c3539de2eb65472017ca"),
    ];
    for (prefix, parts, expected) in cases {
        assert_eq!(derived_key(prefix, parts).unwrap().as_str(), expected);
    }
}

#[test]
fn derived_key_parts_are_separated_and_ordered() {
    let joined = derived_key("k", &["ab", "c"]).unwrap();
    assert_ne!(joined, derived_key("k", &["a", "bc"]).unwrap());
    assert_ne!(joined, derived_key("k", &["c", "ab"]).unwrap());
    assert_ne!(joined, derived_key("j", &["ab", "c"]).unwrap());
    assert_eq!(
        derived_key("k", &[""]).unwrap().as_str(),
        "k:e3b0c44298fc1c149afbf4c8996fb924"
    );
}

#[test]
fn derived_key_refuses_bad_prefixes_and_parts() {
    let long = "p".repeat(MAX_DERIVED_KEY_PREFIX_BYTES + 1);
    for prefix in ["", "Upper", "with space", "a:b", "pending", long.as_str()] {
        assert_eq!(
            derived_key(prefix, &["a"]),
            Err(DerivedKeyError::Prefix),
            "{prefix}"
        );
    }
    let longest = "p".repeat(MAX_DERIVED_KEY_PREFIX_BYTES);
    let key = derived_key(&longest, &["a"]).unwrap();
    assert!(key.as_str().len() <= MAX_ROW_KEY_BYTES);
    assert!(!key.as_str().starts_with(PENDING_KEY_PREFIX));
    assert_eq!(derived_key("k", &[]), Err(DerivedKeyError::NoParts));
    assert_eq!(derived_key("k", &["a\0b"]), Err(DerivedKeyError::Part));
}
