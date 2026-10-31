//! Golden vectors freeze the operation encodings of every format version, and the charter,
//! certificate, proposal, and recovery encodings beside them. Changing any byte breaks every stored
//! log, so a mismatch means the format changed by accident. Set `GANBARU_SYNC_BLESS_VECTORS=1`
//! only when introducing a new format version deliberately.

use super::fixtures::{golden_cause, golden_chain, golden_group, writer_key};
use crate::bounds::{MAX_GROUP_BYTES, MAX_OPERATIONS_PER_GROUP};
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::hex;
use crate::op::{
    BASE_FORMAT_VERSION, CAUSE_FORMAT_VERSION, ENVELOPE_OVERHEAD, FORMAT_VERSION,
    GROUP_FORMAT_VERSION, OPERATION_MAGIC,
};
use crate::signing::WriterPublicKey;
use crate::{
    Cause, ChangeAction, Content, Envelope, Field, GroupId, GroupValue, OperationError,
    OperationKind, PayloadError, SignedCertificate, Value, ends_group,
};
use std::path::PathBuf;

const VECTOR_NAMES: [&str; 3] = ["genesis", "changes", "acknowledgement"];
/// Wire kind of the retired hub revocation, unassigned in every format.
const UNASSIGNED_KIND: u8 = 2;
/// The version 2 vector: a tombstone naming its cause.
const CAUSE_VECTOR: &str = "changes-cause";
/// The version 4 vector: the cause operation marked as continuing its apply group.
const GROUP_VECTOR: &str = "changes-group";
/// Header offset of the version 4 flags byte: clock, manifest version, revision, key epoch.
const FLAGS_OFFSET: usize = 8 + 4 + 8 + 4;
const BLESS_VARIABLE: &str = "GANBARU_SYNC_BLESS_VECTORS";

fn vector_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/tests/vectors")
        .join(format!("{name}.hex"))
}

fn expected_vector(name: &str) -> String {
    std::fs::read_to_string(vector_path(name))
        .unwrap_or_else(|error| panic!("golden vector {name} is missing: {error}"))
        .trim()
        .to_string()
}

/// Compares `actual` text with the golden vector `name`, or rewrites the vector when blessing.
pub(super) fn assert_vector(name: &str, actual: &str) {
    if std::env::var_os(BLESS_VARIABLE).is_some() {
        std::fs::write(vector_path(name), format!("{actual}\n")).unwrap();
        return;
    }
    assert_eq!(
        actual,
        expected_vector(name),
        "golden vector {name} changed; actual:\n{actual}"
    );
}

#[test]
fn golden_operations_encode_to_frozen_bytes() {
    let chain = golden_chain();
    let bless = std::env::var_os(BLESS_VARIABLE).is_some();
    for (name, (_, bytes)) in VECTOR_NAMES.iter().zip(&chain) {
        let actual = hex(bytes);
        if bless {
            std::fs::write(vector_path(name), format!("{actual}\n")).unwrap();
            continue;
        }
        assert_eq!(
            actual,
            expected_vector(name),
            "golden vector {name} changed; actual bytes:\n{actual}"
        );
    }
}

#[test]
fn golden_operations_decode_and_reencode_byte_identically() {
    let key = writer_key();
    let public_key = key.public_key();
    let mut previous_hash = [0u8; 32];
    for (operation, bytes) in golden_chain() {
        let envelope = Envelope::decode(&bytes).unwrap();
        assert_eq!(envelope.previous_hash, previous_hash);
        let decoded = if envelope.kind == OperationKind::Genesis.as_u8() {
            let (decoded, certificate) = envelope.genesis().unwrap();
            assert_eq!(certificate.certificate.writer_key, public_key);
            decoded
        } else {
            assert!(envelope.verify(&public_key));
            envelope.operation().unwrap()
        };
        assert_eq!(decoded, operation);
        let resealed = decoded.seal(&key).unwrap();
        assert_eq!(resealed.bytes(), bytes.as_slice());
        assert_eq!(resealed.hash(), envelope.hash());
        previous_hash = envelope.hash();
    }
}

#[test]
fn golden_certificate_roundtrips() {
    let chain = golden_chain();
    let Content::Genesis(certificate) = &chain[0].0.content else {
        panic!("first golden operation is not a genesis");
    };
    let encoded = certificate.encode();
    assert_eq!(&SignedCertificate::decode(&encoded).unwrap(), certificate);
}

#[test]
fn signature_excludes_the_body_but_value_hashes_bind_it() {
    let chain = golden_chain();
    let bytes = &chain[1].1;
    let envelope = Envelope::decode(bytes).unwrap();
    let replacement = Value::new(vec![Field::Blob(vec![9, 9, 9, 9, 9])]).unwrap();
    let mut body = Writer::default();
    let mut original = Reader::new(envelope.body);
    // Keep the tag create and lifecycle values; swap the body run list for another value.
    let mut values = Vec::new();
    while original.remaining() > 0 {
        assert_eq!(original.u8().unwrap(), 1);
        values.push(Value::decode_from(&mut original).unwrap());
    }
    values[4] = replacement;
    for value in &values {
        body.u8(1);
        value.encode_into(&mut body);
    }
    let mut forged = envelope.signed.to_vec();
    forged.extend_from_slice(&(body.len() as u32).to_be_bytes());
    forged.extend_from_slice(body.as_bytes());
    forged.extend_from_slice(envelope.signature);
    let forged_envelope = Envelope::decode(&forged).unwrap();
    assert!(forged_envelope.verify(&writer_key().public_key()));
    assert_eq!(
        forged_envelope.operation(),
        Err(PayloadError::Body(CodecError::Malformed("value hash")))
    );
}

fn with_body(bytes: &[u8], body: &[u8]) -> Vec<u8> {
    let envelope = Envelope::decode(bytes).unwrap();
    let mut forged = envelope.signed.to_vec();
    forged.extend_from_slice(&(body.len() as u32).to_be_bytes());
    forged.extend_from_slice(body);
    forged.extend_from_slice(envelope.signature);
    forged
}

#[test]
fn unsigned_body_damage_is_reported_apart_from_header_errors() {
    let chain = golden_chain();
    let envelope = Envelope::decode(&chain[1].1).unwrap();
    let truncated = with_body(&chain[1].1, &envelope.body[..envelope.body.len() - 1]);
    assert!(matches!(
        Envelope::decode(&truncated).unwrap().operation(),
        Err(PayloadError::Body(_))
    ));
    let mut trailing = envelope.body.to_vec();
    trailing.push(0);
    let trailing = with_body(&chain[1].1, &trailing);
    assert_eq!(
        Envelope::decode(&trailing).unwrap().operation(),
        Err(PayloadError::Body(CodecError::Trailing))
    );
    for index in [0, 2] {
        let padded = with_body(&chain[index].1, &[1]);
        assert_eq!(
            Envelope::decode(&padded).unwrap().operation(),
            Err(PayloadError::Body(CodecError::Malformed("body")))
        );
    }

    let header_damage = signed_envelope(1, 0, &envelope.header[..envelope.header.len() - 1], b"");
    assert!(matches!(
        Envelope::decode(&header_damage).unwrap().operation(),
        Err(PayloadError::Invalid(OperationError::Codec(_)))
    ));
}

#[test]
fn an_acknowledgement_carries_no_changes_and_seals_at_the_base_format() {
    let chain = golden_chain();
    let (operation, bytes) = &chain[2];
    assert_eq!(operation.kind(), OperationKind::Acknowledgement);
    assert_eq!(operation.format_version(), BASE_FORMAT_VERSION);
    let envelope = Envelope::decode(bytes).unwrap();
    assert_eq!(envelope.kind, 3);
    assert!(envelope.body.is_empty());

    // A changes payload after the dependencies is trailing header data for kind 3.
    let mut header = envelope.header.to_vec();
    header.extend_from_slice(&0u16.to_be_bytes());
    let with_changes = signed_envelope(BASE_FORMAT_VERSION, 3, &header, b"");
    assert!(matches!(
        Envelope::decode(&with_changes).unwrap().operation(),
        Err(PayloadError::Invalid(OperationError::Codec(_)))
    ));
    let with_body = signed_envelope(BASE_FORMAT_VERSION, 3, envelope.header, &[1]);
    assert_eq!(
        Envelope::decode(&with_body).unwrap().operation(),
        Err(PayloadError::Body(CodecError::Malformed("body")))
    );
    let plain = signed_envelope(BASE_FORMAT_VERSION, 3, envelope.header, b"");
    let decoded = Envelope::decode(&plain).unwrap().operation().unwrap();
    assert_eq!(decoded.content, Content::Acknowledgement);
    assert_eq!(decoded.header, operation.header);
}

fn signed_envelope(format_version: u8, kind: u8, header: &[u8], body: &[u8]) -> Vec<u8> {
    let key = writer_key();
    let chain = golden_chain();
    let template = Envelope::decode(&chain[1].1).unwrap();
    let mut writer = Writer::default();
    writer.raw(OPERATION_MAGIC);
    writer.u8(format_version);
    writer.u8(kind);
    writer.raw(template.space.as_bytes());
    writer.raw(template.writer.as_bytes());
    writer.u64(template.seq);
    writer.raw(&template.previous_hash);
    writer.bytes_u32(header);
    let signature = key.sign_operation(writer.as_bytes());
    writer.bytes_u32(body);
    writer.raw(&signature);
    writer.into_bytes()
}

#[test]
fn newer_formats_and_unknown_kinds_verify_but_are_held() {
    let public_key: WriterPublicKey = writer_key().public_key();
    let newer = signed_envelope(FORMAT_VERSION + 1, 0, b"future header", b"future body");
    let envelope = Envelope::decode(&newer).unwrap();
    assert!(envelope.verify(&public_key));
    assert_eq!(envelope.operation(), Err(PayloadError::NewerFormat));

    let unknown = OperationKind::Authorization.as_u8() + 1;
    let future_kind = signed_envelope(FORMAT_VERSION, unknown, b"future kind", b"");
    let envelope = Envelope::decode(&future_kind).unwrap();
    assert!(envelope.verify(&public_key));
    assert_eq!(envelope.operation(), Err(PayloadError::UnknownKind));
    assert_eq!(newer.len(), ENVELOPE_OVERHEAD + 13 + 11);
}

#[test]
fn the_unassigned_kind_decodes_as_unknown_in_every_format() {
    assert_eq!(OperationKind::from_u8(UNASSIGNED_KIND), None);
    for format_version in BASE_FORMAT_VERSION..=FORMAT_VERSION {
        let bytes = signed_envelope(format_version, UNASSIGNED_KIND, b"revocation", b"");
        let envelope = Envelope::decode(&bytes).unwrap();
        assert!(envelope.verify(&writer_key().public_key()));
        assert_eq!(envelope.operation(), Err(PayloadError::UnknownKind));
    }
}

#[test]
fn a_kind_is_unknown_in_formats_older_than_the_one_that_defines_it() {
    let kind = OperationKind::Authorization;
    for format_version in BASE_FORMAT_VERSION..kind.minimum_format_version() {
        let envelope = signed_envelope(format_version, kind.as_u8(), b"authorization", b"");
        let envelope = Envelope::decode(&envelope).unwrap();
        assert!(envelope.verify(&writer_key().public_key()));
        assert_eq!(envelope.operation(), Err(PayloadError::UnknownKind));
    }
}

#[test]
fn a_cause_tombstone_seals_at_version_2_and_reencodes_byte_identically() {
    let (operation, bytes) = golden_cause();
    let actual = hex(&bytes);
    if std::env::var_os(BLESS_VARIABLE).is_some() {
        std::fs::write(vector_path(CAUSE_VECTOR), format!("{actual}\n")).unwrap();
    } else {
        assert_eq!(
            actual,
            expected_vector(CAUSE_VECTOR),
            "golden vector {CAUSE_VECTOR} changed; actual bytes:\n{actual}"
        );
    }
    let envelope = Envelope::decode(&bytes).unwrap();
    assert_eq!(envelope.format_version, CAUSE_FORMAT_VERSION);
    assert!(envelope.verify(&writer_key().public_key()));
    let decoded = envelope.operation().unwrap();
    assert_eq!(decoded, operation);
    assert_eq!(
        decoded.seal(&writer_key()).unwrap().bytes(),
        bytes.as_slice()
    );

    for (_, bytes) in golden_chain() {
        assert_eq!(
            Envelope::decode(&bytes).unwrap().format_version,
            BASE_FORMAT_VERSION
        );
    }
}

#[test]
fn the_cause_successor_is_rejected_in_a_version_1_envelope() {
    let (_, bytes) = golden_cause();
    let envelope = Envelope::decode(&bytes).unwrap();
    let downgraded = signed_envelope(BASE_FORMAT_VERSION, 0, envelope.header, envelope.body);
    let downgraded = Envelope::decode(&downgraded).unwrap();
    assert!(downgraded.verify(&writer_key().public_key()));
    assert_eq!(
        downgraded.operation(),
        Err(PayloadError::Invalid(OperationError::Codec(
            CodecError::Malformed("tombstone successor")
        )))
    );
}

#[test]
fn version_2_readers_accept_version_1_successors_in_either_format() {
    let chain = golden_chain();
    let envelope = Envelope::decode(&chain[1].1).unwrap();
    let upgraded = signed_envelope(CAUSE_FORMAT_VERSION, 0, envelope.header, envelope.body);
    let upgraded = Envelope::decode(&upgraded).unwrap().operation().unwrap();
    assert_eq!(upgraded.content, chain[1].0.content);
}

#[test]
fn a_tombstone_names_one_successor_other_than_itself() {
    let (operation, _) = golden_cause();
    let Content::Changes(changes) = &operation.content else {
        panic!("cause vector is a changes operation");
    };
    let child = changes[1].clone();
    let cause = child.cause.clone().unwrap();
    let mut both = child.clone();
    both.replaced_by = Some(cause.row.clone());
    let mut itself = child.clone();
    itself.cause = Some(Cause {
        table: child.table,
        row: child.row.clone(),
    });
    let mut on_write = changes[0].clone();
    on_write.action = ChangeAction::Write;
    on_write.groups = vec![GroupValue::new(
        on_write.table,
        GroupId::new(0).unwrap(),
        Value::new(vec![Field::Integer(1)]).unwrap(),
    )];
    on_write.cause = Some(cause);
    for change in [both, itself, on_write] {
        let mut invalid = operation.clone();
        invalid.content = Content::Changes(vec![change]);
        assert!(matches!(
            invalid.validate(),
            Err(OperationError::Codec(CodecError::Malformed(_)))
        ));
    }
}

#[test]
fn envelope_rejects_format_zero_wrong_magic_and_sequence_zero() {
    let chain = golden_chain();
    let mut bytes = chain[1].1.clone();
    bytes[4] = 0;
    assert!(Envelope::decode(&bytes).is_err());
    let mut bytes = chain[1].1.clone();
    bytes[0] = b'X';
    assert!(Envelope::decode(&bytes).is_err());
    let mut bytes = chain[1].1.clone();
    bytes[38..46].copy_from_slice(&0u64.to_be_bytes());
    assert!(Envelope::decode(&bytes).is_err());
}

#[test]
fn a_continuing_group_seals_at_version_4_and_reencodes_byte_identically() {
    let (operation, bytes) = golden_group();
    assert_vector(GROUP_VECTOR, &hex(&bytes));
    let envelope = Envelope::decode(&bytes).unwrap();
    assert_eq!(envelope.format_version, GROUP_FORMAT_VERSION);
    assert!(envelope.verify(&writer_key().public_key()));
    // The flags byte follows the key epoch.
    assert_eq!(envelope.header[FLAGS_OFFSET], 1);
    let decoded = envelope.operation().unwrap();
    assert!(decoded.header.group_continues);
    assert_eq!(decoded, operation);
    assert_eq!(
        decoded.seal(&writer_key()).unwrap().bytes(),
        bytes.as_slice()
    );

    let (_, cause) = golden_cause();
    assert_eq!(
        Envelope::decode(&cause).unwrap().format_version,
        CAUSE_FORMAT_VERSION
    );
}

#[test]
fn header_flags_reject_unknown_bits_and_older_formats_lack_them() {
    let (_, bytes) = golden_group();
    let envelope = Envelope::decode(&bytes).unwrap();
    let mut unknown = envelope.header.to_vec();
    unknown[FLAGS_OFFSET] = 0b10;
    let unknown = signed_envelope(GROUP_FORMAT_VERSION, 0, &unknown, envelope.body);
    assert_eq!(
        Envelope::decode(&unknown).unwrap().operation(),
        Err(PayloadError::Invalid(OperationError::Codec(
            CodecError::Malformed("header flags")
        )))
    );

    // An older reader of the same header reads the flags byte as part of the dependency count.
    let downgraded = signed_envelope(CAUSE_FORMAT_VERSION, 0, envelope.header, envelope.body);
    assert!(Envelope::decode(&downgraded).unwrap().operation().is_err());

    // A clear flags byte at version 4 decodes as an operation that ends its group.
    let mut clear = envelope.header.to_vec();
    clear[FLAGS_OFFSET] = 0;
    let clear = signed_envelope(GROUP_FORMAT_VERSION, 0, &clear, envelope.body);
    let decoded = Envelope::decode(&clear).unwrap().operation().unwrap();
    assert!(!decoded.header.group_continues);
}

#[test]
fn only_a_changes_operation_continues_a_group() {
    let chain = golden_chain();
    for index in [0, 2] {
        let mut operation = chain[index].0.clone();
        operation.header.group_continues = true;
        assert_eq!(
            operation.validate(),
            Err(OperationError::Codec(CodecError::Malformed("header flags")))
        );
    }
    let mut empty = chain[1].0.clone();
    empty.content = Content::Changes(Vec::new());
    empty.header.group_continues = true;
    assert!(empty.validate().is_ok());
}

#[test]
fn a_group_ends_at_a_clear_flag_or_either_bound() {
    assert!(ends_group(false, 1, 0));
    assert!(!ends_group(true, 1, 0));
    assert!(!ends_group(
        true,
        MAX_OPERATIONS_PER_GROUP - 1,
        MAX_GROUP_BYTES - 1
    ));
    assert!(ends_group(true, MAX_OPERATIONS_PER_GROUP, 0));
    assert!(ends_group(true, 1, MAX_GROUP_BYTES));
}

#[test]
fn verification_requires_the_writer_key_of_the_envelope() {
    let chain = golden_chain();
    let envelope = Envelope::decode(&chain[1].1).unwrap();
    let other = super::fixtures::other_writer_key().public_key();
    assert!(!envelope.verify(&other));
}
