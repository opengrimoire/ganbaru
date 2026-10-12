use super::fixtures::{
    GOLDEN_CREATED_AT_MS, GOLDEN_DEVICE_ID, certificate_for, other_writer_id, person, writer_key,
};
use crate::signing::{CERTIFICATE_DOMAIN, domain_message};
use crate::{
    CertificateError, SignedCertificate, WriterCertificate, sign_certificate, validate_device_id,
};
use ganbaru_contacts::{PersonKeyPair, verify};

#[test]
fn certificates_roundtrip_with_and_without_predecessor() {
    for predecessor in [None, Some(other_writer_id())] {
        let signed = certificate_for(&writer_key().public_key(), predecessor);
        let decoded = SignedCertificate::decode(&signed.encode()).unwrap();
        assert_eq!(decoded, signed);
        assert_eq!(
            decoded.certificate.writer_id(),
            super::fixtures::writer_id()
        );
    }
}

#[test]
fn every_byte_of_a_certificate_is_authenticated() {
    let encoded = certificate_for(&writer_key().public_key(), Some(other_writer_id())).encode();
    for index in 0..encoded.len() {
        let mut tampered = encoded.clone();
        tampered[index] ^= 0x01;
        assert!(
            SignedCertificate::decode(&tampered).is_err(),
            "tampering byte {index} still decoded"
        );
    }
    let mut extended = encoded.clone();
    extended.push(0);
    assert!(SignedCertificate::decode(&extended).is_err());
    assert!(SignedCertificate::decode(&encoded[..encoded.len() - 1]).is_err());
}

#[test]
fn signing_requires_the_certified_person_key() {
    let (_, stranger) = PersonKeyPair::generate().unwrap();
    let certificate = WriterCertificate {
        writer_key: writer_key().public_key(),
        person_key: person().public_key(),
        device_id: GOLDEN_DEVICE_ID.to_string(),
        created_at_ms: GOLDEN_CREATED_AT_MS,
        predecessor: None,
    };
    assert_eq!(
        sign_certificate(&certificate, &stranger),
        Err(CertificateError::WrongPerson)
    );
}

#[test]
fn certificate_signatures_are_domain_separated() {
    let signed = certificate_for(&writer_key().public_key(), None);
    let encoded = signed.encode();
    let unsigned = &encoded[..encoded.len() - 64];
    let public_key = person().public_key();
    assert!(verify(&public_key, unsigned, &signed.signature).is_err());
    assert!(
        verify(
            &public_key,
            &domain_message(CERTIFICATE_DOMAIN, unsigned),
            &signed.signature
        )
        .is_ok()
    );
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
    let certificate = WriterCertificate {
        writer_key: writer_key().public_key(),
        person_key: person().public_key(),
        device_id: "bad id".to_string(),
        created_at_ms: GOLDEN_CREATED_AT_MS,
        predecessor: None,
    };
    assert_eq!(
        sign_certificate(&certificate, &person()),
        Err(CertificateError::DeviceId)
    );
}

#[test]
fn unknown_versions_and_predecessor_flags_are_rejected() {
    let encoded = certificate_for(&writer_key().public_key(), None).encode();
    let mut version = encoded.clone();
    version[0] = 2;
    assert_eq!(
        SignedCertificate::decode(&version),
        Err(CertificateError::Version)
    );
    let flag_index = 1 + 32 + 32 + 1 + GOLDEN_DEVICE_ID.len() + 8;
    let mut flag = encoded.clone();
    flag[flag_index] = 2;
    assert!(matches!(
        SignedCertificate::decode(&flag),
        Err(CertificateError::Codec(_))
    ));
}
