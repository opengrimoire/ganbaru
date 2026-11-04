use super::fixtures::{
    GOLDEN_CREATED_AT_MS, GOLDEN_DEVICE_ID, certificate_for, device_key, other_writer_id, person,
    writer_key,
};
use crate::bounds::{MAX_CERTIFICATE_BYTES, MAX_DEVICE_ID_BYTES};
use crate::codec::CodecError;
use crate::signing::{CERTIFICATE_DOMAIN, CHARTER_DOMAIN, domain_message};
use crate::{
    CertificateError, DeviceKeyPair, SignedCertificate, WriterCertificate, sign_certificate,
    validate_device_id,
};

#[test]
fn certificates_roundtrip_and_authenticate_every_byte() {
    for predecessor in [None, Some(other_writer_id())] {
        let signed = certificate_for(&writer_key().public_key(), predecessor);
        let encoded = signed.encode();
        let decoded = SignedCertificate::decode(&encoded).unwrap();
        assert_eq!(decoded, signed);
        assert_eq!(
            decoded.certificate.writer_id(),
            super::fixtures::writer_id()
        );
        for index in 0..encoded.len() {
            let mut mutated = encoded.clone();
            mutated[index] ^= 0x01;
            assert!(
                SignedCertificate::decode(&mutated).is_err(),
                "byte {index} is not authenticated"
            );
        }
        let mut extended = encoded.clone();
        extended.push(0);
        assert!(SignedCertificate::decode(&extended).is_err());
        assert!(SignedCertificate::decode(&encoded[..encoded.len() - 1]).is_err());
    }
}

#[test]
fn signing_requires_the_named_device_key() {
    let mut certificate = certificate_for(&writer_key().public_key(), None).certificate;
    let stranger = DeviceKeyPair::from_seed(&[0x47; 32]).unwrap();
    assert_eq!(
        sign_certificate(&certificate, &stranger),
        Err(CertificateError::WrongDevice)
    );
    certificate.device_id = "bad id".to_string();
    assert_eq!(
        sign_certificate(&certificate, &device_key()),
        Err(CertificateError::DeviceId)
    );
}

#[test]
fn certificate_signatures_are_domain_separated() {
    let signed = certificate_for(&writer_key().public_key(), None);
    let encoded = signed.encode();
    let unsigned = &encoded[..encoded.len() - 64];
    let public_key = device_key().public_key();
    assert!(public_key.verify(CERTIFICATE_DOMAIN, unsigned, &signed.signature));
    assert!(!public_key.verify(CHARTER_DOMAIN, unsigned, &signed.signature));
}

#[test]
fn device_ids_follow_the_handoff_identifier_rule() {
    for valid in ["a", "device-1", "device_1.desktop", &"d".repeat(160)] {
        assert!(validate_device_id(valid).is_ok(), "{valid} should be valid");
    }
    for invalid in ["", "with space", "slash/name", "acentó", &"d".repeat(161)] {
        assert_eq!(
            validate_device_id(invalid),
            Err(CertificateError::DeviceId),
            "{invalid:?} should be invalid"
        );
    }
}

#[test]
fn unsupported_versions_and_predecessor_flags_are_rejected() {
    let encoded = certificate_for(&writer_key().public_key(), None).encode();
    for version in [0, 1, 3] {
        let mut changed = encoded.clone();
        changed[0] = version;
        assert_eq!(
            SignedCertificate::decode(&changed),
            Err(CertificateError::Version),
            "version {version}"
        );
    }
    let flag_index = 1 + 32 + 1 + GOLDEN_DEVICE_ID.len() + 32 + 8;
    let mut flag = encoded.clone();
    flag[flag_index] = 2;
    assert!(matches!(
        SignedCertificate::decode(&flag),
        Err(CertificateError::Codec(_))
    ));
}

#[test]
fn small_order_writer_keys_never_verify_an_operation() {
    let mut identity = [0u8; 32];
    identity[0] = 1;
    let mut forgery = [0u8; 64];
    forgery[0] = 1;

    assert!(crate::WriterPublicKey::from_slice(&identity).is_err());
    assert!(!crate::WriterPublicKey::from_bytes(identity).verify_operation(b"operation", &forgery));
    assert!(crate::WriterPublicKey::from_slice(writer_key().public_key().as_bytes()).is_ok());
}

#[test]
fn every_person_key_signature_domain_is_distinct_and_prefix_free() {
    let person = person();
    let card = ganbaru_contacts::sign_card(
        &ganbaru_contacts::ContactCard {
            public_key: person.public_key(),
            display_name: "Ana".to_string(),
            color: 1,
            nonce: [2u8; 16],
            endpoint_hint: "192.168.1.20:43821".to_string(),
            coordinator_fingerprint: None,
            issued_at_ms: GOLDEN_CREATED_AT_MS,
        },
        &person,
    )
    .expect("sign card");
    let charter_domain = domain_message(CHARTER_DOMAIN, b"");
    let signed_messages: [(&[u8], Vec<u8>); 4] = [
        (ganbaru_contacts::CARD_MAGIC, card.bytes.clone()),
        (
            ganbaru_contacts::REQUEST_SIGNATURE_DOMAIN,
            ganbaru_contacts::request_signature_payload(&[3u8; 16], "request-1"),
        ),
        (
            ganbaru_contacts::STATUS_SIGNATURE_DOMAIN,
            ganbaru_contacts::status_signature_payload("request-1", GOLDEN_CREATED_AT_MS),
        ),
        (
            &charter_domain,
            domain_message(CHARTER_DOMAIN, &super::fixtures::golden_charter().encode()),
        ),
    ];

    for (domain, message) in &signed_messages {
        assert!(message.starts_with(domain), "{domain:?}");
    }
    for (left, (left_domain, _)) in signed_messages.iter().enumerate() {
        for (right, (right_domain, _)) in signed_messages.iter().enumerate() {
            if left != right {
                assert!(
                    !right_domain.starts_with(left_domain),
                    "{left_domain:?} is a prefix of {right_domain:?}"
                );
            }
        }
    }
}

#[test]
fn oversized_certificates_are_refused_before_parsing() {
    let mut encoded = certificate_for(&writer_key().public_key(), None).encode();
    encoded.resize(MAX_CERTIFICATE_BYTES + 1, 0);
    assert_eq!(
        SignedCertificate::decode(&encoded),
        Err(CertificateError::Codec(CodecError::Bounds(
            "writer certificate"
        )))
    );
}

#[test]
fn the_certificate_bound_fits_the_largest_certificate() {
    let certificate = WriterCertificate {
        writer_key: writer_key().public_key(),
        device_id: "d".repeat(MAX_DEVICE_ID_BYTES),
        device_key: device_key().public_key(),
        created_at_ms: i64::MAX,
        predecessor: Some(other_writer_id()),
    };
    let encoded = sign_certificate(&certificate, &device_key())
        .unwrap()
        .encode();
    assert_eq!(encoded.len(), MAX_CERTIFICATE_BYTES);
    assert_eq!(MAX_CERTIFICATE_BYTES, 315);
    assert!(SignedCertificate::decode(&encoded).is_ok());
}
