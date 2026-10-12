//! Golden vectors freeze the version 1 encodings. Changing any byte breaks every stored log, so
//! a mismatch means the format changed by accident. Set `GANBARU_SYNC_BLESS_VECTORS=1` only when
//! introducing a new format version deliberately.

use super::fixtures::{golden_chain, writer_key};
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::hex;
use crate::op::{ENVELOPE_OVERHEAD, OPERATION_MAGIC};
use crate::signing::WriterPublicKey;
use crate::{
    Content, Envelope, Field, OperationError, OperationKind, PayloadError, SignedCertificate, Value,
};
use std::path::PathBuf;

const VECTOR_NAMES: [&str; 3] = ["genesis", "changes", "revoke"];
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
    let newer = signed_envelope(2, 0, b"future header", b"future body");
    let envelope = Envelope::decode(&newer).unwrap();
    assert!(envelope.verify(&public_key));
    assert_eq!(envelope.operation(), Err(PayloadError::NewerFormat));

    let checkpoint = signed_envelope(1, 3, b"checkpoint", b"");
    let envelope = Envelope::decode(&checkpoint).unwrap();
    assert!(envelope.verify(&public_key));
    assert_eq!(envelope.operation(), Err(PayloadError::UnknownKind));
    assert_eq!(newer.len(), ENVELOPE_OVERHEAD + 13 + 11);
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
fn verification_requires_the_writer_key_of_the_envelope() {
    let chain = golden_chain();
    let envelope = Envelope::decode(&chain[1].1).unwrap();
    let other = super::fixtures::other_writer_key().public_key();
    assert!(!envelope.verify(&other));
}
