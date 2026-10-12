use crate::codec::CodecError;
use crate::vector::validate_dependencies;
use crate::{VersionVector, WriterId};

fn writer(byte: u8) -> WriterId {
    WriterId::from_bytes([byte; 16])
}

#[test]
fn coverage_merge_and_dominance() {
    let mut left = VersionVector::new();
    left.advance(writer(1), 3);
    left.advance(writer(2), 1);
    left.advance(writer(1), 2);
    assert_eq!(left.get(&writer(1)), 3);
    assert!(left.covers(&writer(1), 3));
    assert!(!left.covers(&writer(1), 4));
    assert!(!left.covers(&writer(3), 1));
    assert!(left.covers(&writer(3), 0));

    let right: VersionVector = [(writer(2), 4), (writer(3), 1)].into_iter().collect();
    assert!(!left.dominates(&right));
    assert!(!right.dominates(&left));
    let mut merged = left.clone();
    merged.merge(&right);
    assert!(merged.dominates(&left) && merged.dominates(&right));
    assert_eq!(
        merged.delta_since(&left),
        vec![(writer(2), 4), (writer(3), 1)]
    );
    assert!(merged.delta_since(&merged).is_empty());

    left.set(writer(1), 0);
    assert_eq!(left.len(), 1);
}

#[test]
fn encoding_is_canonical_and_strict() {
    let vector: VersionVector = [(writer(9), 2), (writer(1), 5)].into_iter().collect();
    let encoded = vector.encode();
    assert_eq!(VersionVector::decode(&encoded).unwrap(), vector);
    assert_eq!(
        VersionVector::decode(&VersionVector::new().encode()).unwrap(),
        VersionVector::new()
    );

    let mut unsorted = Vec::new();
    unsorted.extend_from_slice(&2u32.to_be_bytes());
    unsorted.extend_from_slice(&[9u8; 16]);
    unsorted.extend_from_slice(&1u64.to_be_bytes());
    unsorted.extend_from_slice(&[1u8; 16]);
    unsorted.extend_from_slice(&1u64.to_be_bytes());
    assert!(VersionVector::decode(&unsorted).is_err());

    let mut zero = Vec::new();
    zero.extend_from_slice(&1u32.to_be_bytes());
    zero.extend_from_slice(&[1u8; 16]);
    zero.extend_from_slice(&0u64.to_be_bytes());
    assert!(VersionVector::decode(&zero).is_err());

    let mut huge_count = Vec::new();
    huge_count.extend_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        VersionVector::decode(&huge_count),
        Err(CodecError::Truncated)
    );
}

#[test]
fn dependencies_are_sorted_unique_nonzero_and_exclude_the_writer() {
    let own = writer(5);
    assert!(validate_dependencies(&[(writer(1), 1), (writer(2), 9)], &own).is_ok());
    assert!(validate_dependencies(&[(writer(2), 1), (writer(1), 1)], &own).is_err());
    assert!(validate_dependencies(&[(writer(1), 1), (writer(1), 2)], &own).is_err());
    assert!(validate_dependencies(&[(writer(1), 0)], &own).is_err());
    assert!(validate_dependencies(&[(own, 1)], &own).is_err());
}
