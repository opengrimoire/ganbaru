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
fn contact_codes_carry_80_bits_while_live_codes_keep_60() {
    let key = PersonKeyPair::from_seed(&[5u8; 32]).expect("key");
    let contact_code = key.public_key().verification_code();
    let groups: Vec<&str> = contact_code.split('-').collect();

    assert_eq!(groups.len(), 4);
    assert!(groups.iter().all(|group| group.len() == 4));
    assert_eq!(
        contact_code.len() - 3,
        16,
        "16 base32 characters are 80 bits"
    );
    let digest: [u8; 32] =
        <sha2::Sha256 as sha2::Digest>::digest(key.public_key().as_bytes()).into();
    assert!(contact_code.starts_with(&verification_code(&digest)));
    assert_eq!(verification_code(&digest).len(), 14);
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

#[test]
fn cards_with_bidi_or_invisible_name_characters_are_refused_even_when_signed() {
    let (_, key) = PersonKeyPair::generate().unwrap();
    let spoofed = ContactCard {
        display_name: "Ana\u{202E}".to_string(),
        ..sample_card(&key)
    };
    assert_eq!(sign_card(&spoofed, &key), Err(CardError::Field));

    let template = ContactCard {
        display_name: "Anaaaa".to_string(),
        ..sample_card(&key)
    };
    let signed = sign_card(&template, &key).unwrap();
    let name_start = 4 + 1 + 32 + 1;
    for replacement in [
        "Ana\u{202E}",
        "Ana\u{2066}",
        "Ana\u{200B}",
        "Ana\u{FEFF}",
        "Anaa\u{0085}",
    ] {
        let mut bytes = signed.bytes[..signed.bytes.len() - 64].to_vec();
        assert_eq!(replacement.len(), template.display_name.len());
        bytes[name_start..name_start + replacement.len()].copy_from_slice(replacement.as_bytes());
        let signature = key.sign(&bytes);
        bytes.extend_from_slice(&signature);
        assert_eq!(
            decode_card(&bytes),
            Err(CardError::Field),
            "{replacement:?}"
        );
    }

    for name in ["👩\u{200D}💻", "می\u{200C}خواهم"] {
        let card = ContactCard {
            display_name: name.to_string(),
            ..sample_card(&key)
        };
        let signed = sign_card(&card, &key).unwrap();
        assert_eq!(decode_card(&signed.bytes).unwrap().card.display_name, name);
    }
}

/// Every encoding of a point whose order divides 8: the seven y coordinates with either sign bit.
fn small_order_encodings() -> Vec<[u8; 32]> {
    let mut prime = [0xffu8; 32];
    prime[0] = 0xed;
    prime[31] = 0x7f;
    let order_eight = [
        "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
        "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    ]
    .map(|hex| {
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex");
        }
        bytes
    });
    let mut y_values = vec![[0u8; 32], {
        let mut one = [0u8; 32];
        one[0] = 1;
        one
    }];
    y_values.extend(order_eight);
    for low_byte in [0xec, 0xed, 0xee] {
        let mut value = prime;
        value[0] = low_byte;
        y_values.push(value);
    }
    y_values
        .into_iter()
        .flat_map(|y| {
            let mut negative = y;
            negative[31] |= 0x80;
            [y, negative]
        })
        .collect()
}

/// A message among the first 256 for which ring alone accepts the all-identity forgery under `key`.
fn message_ring_alone_accepts(key: &[u8; 32]) -> Option<[u8; 4]> {
    use ring::signature::{ED25519, UnparsedPublicKey};
    let mut forgery = [0u8; 64];
    forgery[0] = 1;
    (0u32..256).map(u32::to_le_bytes).find(|message| {
        UnparsedPublicKey::new(&ED25519, key)
            .verify(message, &forgery)
            .is_ok()
    })
}

#[test]
fn small_order_keys_are_refused_although_ring_accepts_forgeries_under_them() {
    let mut forgery = [0u8; 64];
    forgery[0] = 1;
    for encoding in small_order_encodings() {
        let message = message_ring_alone_accepts(&encoding)
            .unwrap_or_else(|| panic!("{encoding:02x?} should admit a forgery"));
        assert!(crate::is_small_order_public_key(&encoding));
        assert_eq!(
            verify(&PersonPublicKey::from_bytes(encoding), &message, &forgery),
            Err(SignatureError::InvalidKey)
        );
        assert_eq!(
            PersonPublicKey::from_slice(&encoding),
            Err(SignatureError::InvalidKey)
        );
        let text = PersonPublicKey::from_bytes(encoding).to_text();
        assert_eq!(
            PersonPublicKey::from_text(&text),
            Err(SignatureError::InvalidKey)
        );
    }

    let genuine = PersonKeyPair::from_seed(&[4u8; 32]).expect("genuine key");
    let public_key = genuine.public_key();
    assert!(!crate::is_small_order_public_key(public_key.as_bytes()));
    assert_eq!(
        PersonPublicKey::from_text(&public_key.to_text()),
        Ok(public_key)
    );
    assert_eq!(
        verify(&public_key, b"payload", &genuine.sign(b"payload")),
        Ok(())
    );
    assert!(message_ring_alone_accepts(public_key.as_bytes()).is_none());
}
