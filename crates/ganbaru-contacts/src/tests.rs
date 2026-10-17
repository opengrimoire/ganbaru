use crate::{
    CARD_TEXT_PREFIX, CardError, ContactCard, MAX_DISPLAY_NAME_BYTES, MAX_ENDPOINT_HINT_BYTES,
    PersonKeyPair, PersonPublicKey, SignatureError, TrustKind, decode_card, decode_card_text,
    decode_nonce, encode_nonce, nonces_match, request_signature_payload, sign_card,
    status_signature_payload, verification_code, verify,
};
use chrono::{TimeZone, Utc};

fn sample_card(key: &PersonKeyPair) -> ContactCard {
    ContactCard {
        public_key: key.public_key(),
        display_name: "Ana".to_string(),
        color: 7,
        nonce: [9u8; 16],
        endpoint_hint: "192.168.1.20:43821".to_string(),
        coordinator_fingerprint: Some([3u8; 32]),
        issued_at_ms: 1_760_000_000_000,
    }
}

#[test]
fn key_pair_roundtrips_through_pkcs8_and_signs() {
    let (pkcs8, key) = PersonKeyPair::generate().unwrap();
    let restored = PersonKeyPair::from_pkcs8(&pkcs8).unwrap();
    assert_eq!(key.public_key(), restored.public_key());
    let signature = key.sign(b"payload");
    verify(&restored.public_key(), b"payload", &signature).unwrap();
    assert_eq!(
        verify(&restored.public_key(), b"other", &signature),
        Err(SignatureError::Invalid)
    );
    assert!(PersonKeyPair::from_pkcs8(b"garbage").is_err());
}

#[test]
fn public_key_text_form_and_contact_id_are_stable() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let public_key = key.public_key();
    let text = public_key.to_text();
    assert_eq!(text.len(), 43);
    assert_eq!(PersonPublicKey::from_text(&text).unwrap(), public_key);
    assert!(PersonPublicKey::from_text("short").is_err());
    let id = public_key.contact_id();
    assert!(id.starts_with("person:"));
    assert_eq!(id.len(), "person:".len() + 32);
    assert_eq!(id, public_key.contact_id());
}

#[test]
fn signed_card_roundtrips_through_bytes_and_text() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let card = sample_card(&key);
    let signed = sign_card(&card, &key).unwrap();
    let decoded = decode_card(&signed.bytes).unwrap();
    assert_eq!(decoded, signed);
    let text = signed.text();
    assert!(text.starts_with(CARD_TEXT_PREFIX));
    let from_text = decode_card_text(&format!("  {text}\n")).unwrap();
    assert_eq!(from_text.card, card);
    assert_eq!(from_text.verification_code(), signed.verification_code());
}

#[test]
fn verification_code_follows_the_person_key_across_card_changes() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let card = sample_card(&key);
    let signed = sign_card(&card, &key).unwrap();
    let resigned = sign_card(
        &ContactCard {
            nonce: [7u8; 16],
            endpoint_hint: String::new(),
            coordinator_fingerprint: None,
            issued_at_ms: card.issued_at_ms + 60_000,
            ..card.clone()
        },
        &key,
    )
    .unwrap();
    assert_ne!(resigned.digest, signed.digest);
    assert_eq!(resigned.verification_code(), signed.verification_code());
    assert_eq!(
        signed.verification_code(),
        key.public_key().verification_code()
    );

    let (_, other) = PersonKeyPair::generate().unwrap();
    assert_ne!(
        other.public_key().verification_code(),
        signed.verification_code()
    );
}

#[test]
fn card_without_hint_or_fingerprint_roundtrips() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let card = ContactCard {
        endpoint_hint: String::new(),
        coordinator_fingerprint: None,
        ..sample_card(&key)
    };
    let signed = sign_card(&card, &key).unwrap();
    assert_eq!(decode_card(&signed.bytes).unwrap().card, card);
}

#[test]
fn tampered_card_fails_signature() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let signed = sign_card(&sample_card(&key), &key).unwrap();
    let mut bytes = signed.bytes.clone();
    let name_offset = 4 + 1 + 32 + 1;
    bytes[name_offset] ^= 0x01;
    assert_eq!(decode_card(&bytes), Err(CardError::Signature));
    let mut truncated = signed.bytes.clone();
    truncated.pop();
    assert_eq!(decode_card(&truncated), Err(CardError::Bounds));
    let mut extended = signed.bytes.clone();
    extended.push(0);
    assert_eq!(decode_card(&extended), Err(CardError::Bounds));
}

#[test]
fn card_signed_by_another_key_is_rejected() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let (_, other) = PersonKeyPair::generate().unwrap();
    let card = sample_card(&key);
    assert_eq!(sign_card(&card, &other), Err(CardError::Signature));
}

#[test]
fn card_rejects_unknown_magic_and_bad_text() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let signed = sign_card(&sample_card(&key), &key).unwrap();
    let mut bytes = signed.bytes.clone();
    bytes[0] = b'X';
    assert_eq!(decode_card(&bytes), Err(CardError::Format));
    assert_eq!(decode_card_text("GANB"), Err(CardError::Text));
    assert_eq!(decode_card_text("GANB-***"), Err(CardError::Text));
    assert_eq!(decode_card_text(""), Err(CardError::Text));
}

#[test]
fn oversized_fields_are_rejected_before_signing() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let long_name = ContactCard {
        display_name: "a".repeat(MAX_DISPLAY_NAME_BYTES + 1),
        ..sample_card(&key)
    };
    assert_eq!(sign_card(&long_name, &key), Err(CardError::Bounds));
    let long_hint = ContactCard {
        endpoint_hint: "h".repeat(MAX_ENDPOINT_HINT_BYTES + 1),
        ..sample_card(&key)
    };
    assert_eq!(sign_card(&long_hint, &key), Err(CardError::Bounds));
    let blank_name = ContactCard {
        display_name: "   ".to_string(),
        ..sample_card(&key)
    };
    assert_eq!(sign_card(&blank_name, &key), Err(CardError::Field));
    let bad_color = ContactCard {
        color: 32,
        ..sample_card(&key)
    };
    assert_eq!(sign_card(&bad_color, &key), Err(CardError::Bounds));
}

#[test]
fn verification_code_is_crockford_groups_and_stable() {
    let code = verification_code(&[0xffu8; 32]);
    assert_eq!(code, "ZZZZ-ZZZZ-ZZZZ");
    let other = verification_code(&[0u8; 32]);
    assert_eq!(other, "0000-0000-0000");
    let mixed = verification_code(&[0x01u8; 32]);
    assert_eq!(mixed.len(), 14);
    assert!(
        mixed
            .chars()
            .all(|c| c == '-' || "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c))
    );
    assert_eq!(mixed, verification_code(&[0x01u8; 32]));
}

#[test]
fn request_and_status_payloads_bind_their_inputs() {
    let nonce = [1u8; 16];
    let first = request_signature_payload(&nonce, "request-a");
    let second = request_signature_payload(&[2u8; 16], "request-a");
    let third = request_signature_payload(&nonce, "request-b");
    assert_ne!(first, second);
    assert_ne!(first, third);
    assert_ne!(
        status_signature_payload("request-a", 1),
        status_signature_payload("request-a", 2)
    );
    assert_ne!(first, status_signature_payload("request-a", 0));
}

#[test]
fn nonce_text_roundtrips_and_compares_in_constant_time() {
    let nonce = [5u8; 16];
    let text = encode_nonce(&nonce);
    assert_eq!(text.len(), 22);
    assert_eq!(decode_nonce(&text), Some(nonce));
    assert_eq!(decode_nonce("short"), None);
    assert!(nonces_match(&nonce, &nonce));
    assert!(!nonces_match(&nonce, &[6u8; 16]));
    assert!(!nonces_match(&nonce, &nonce[..8]));
}

#[test]
fn trust_kind_names_expiry_and_activity() {
    for kind in [
        TrustKind::NotAllowed,
        TrustKind::Once,
        TrustKind::SevenDays,
        TrustKind::ThirtyDays,
        TrustKind::UntilRevoked,
    ] {
        assert_eq!(TrustKind::parse(kind.as_str()), Some(kind));
        assert_eq!(
            serde_json::to_value(kind).unwrap(),
            serde_json::Value::String(kind.as_str().to_string())
        );
    }
    assert_eq!(TrustKind::parse("forever"), None);
    let granted = Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap();
    let week = TrustKind::SevenDays.expires_at(granted).unwrap();
    assert_eq!(week, Utc.with_ymd_and_hms(2026, 10, 14, 12, 0, 0).unwrap());
    assert_eq!(TrustKind::UntilRevoked.expires_at(granted), None);
    assert!(TrustKind::SevenDays.is_active(Some(week), granted));
    assert!(!TrustKind::SevenDays.is_active(Some(week), week));
    assert!(!TrustKind::SevenDays.is_active(None, granted));
    assert!(!TrustKind::NotAllowed.is_active(None, granted));
    assert!(TrustKind::Once.is_active(None, granted));
}
