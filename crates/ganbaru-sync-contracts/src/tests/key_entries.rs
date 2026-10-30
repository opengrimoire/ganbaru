use super::fixtures::{
    GOLDEN_CREATED_AT_MS, GOLDEN_DEVICE_ID, admin_key, device_key, encryption_key, fixed_envelope,
    golden_charter, golden_key_entries, golden_key_epoch, proposal, space,
};
use super::golden::assert_vector;
use crate::bounds::{MAX_KEY_RECIPIENTS, MAX_SERVER_ENDPOINTS};
use crate::codec::CodecError;
use crate::ids::hex;
use crate::signing::{DEVICE_PROPOSAL_DOMAIN, PROPOSAL_DOMAIN};
use crate::{
    AdminPublicKey, AuthorizationError, AuthorizationRecord, DeviceKey, DeviceKeyPair, Entry,
    KeyEpoch, Proposal, Sequencing, Server, ServerConfig, ServerMode, SignedProposal,
    sign_device_proposal, sign_proposal,
};

fn key_record(entry: Entry, sequencing: Sequencing) -> AuthorizationRecord {
    AuthorizationRecord {
        epoch: 0,
        revision: 3,
        previous_entry_hash: [0x0c; 32],
        anchor_device_id: GOLDEN_DEVICE_ID.to_string(),
        proposal: proposal(entry, 0),
        sequencing,
    }
}

fn unsigned_proposal(entry: Entry, author: AdminPublicKey) -> Proposal {
    Proposal {
        space: space(),
        charter: golden_charter().hash(),
        author,
        created_at_ms: GOLDEN_CREATED_AT_MS,
        entry,
    }
}

fn device_author() -> AdminPublicKey {
    AdminPublicKey::from_bytes(*device_key().public_key().as_bytes())
}

fn key_epoch_error(key_epoch: KeyEpoch) -> AuthorizationError {
    let proposal = unsigned_proposal(Entry::KeyEpoch(key_epoch), device_author());
    sign_device_proposal(&proposal, &device_key()).unwrap_err()
}

#[test]
fn key_entries_encode_to_frozen_bytes_and_round_trip() {
    for (name, entry, sequencing) in golden_key_entries() {
        let record = key_record(entry, sequencing);
        let bytes = record.encode();
        assert_vector(name, &hex(&bytes));
        let decoded = AuthorizationRecord::decode(&bytes).unwrap();
        assert_eq!(decoded, record, "{name}");
        assert_eq!(decoded.encode(), bytes, "{name}");
    }
}

#[test]
fn device_key_and_key_epoch_entries_are_signed_by_a_device_key_only() {
    for (name, entry, _) in golden_key_entries() {
        let signed = proposal(entry.clone(), 0);
        assert_eq!(
            signed.proposal.device_author().is_some(),
            entry.device_signed(),
            "{name}"
        );
        assert_eq!(
            signed.proposal.admin_author().is_some(),
            !entry.device_signed(),
            "{name}"
        );
        if entry.device_signed() {
            let as_admin = unsigned_proposal(entry, admin_key().public_key());
            assert_eq!(
                sign_proposal(&as_admin, &admin_key()),
                Err(AuthorizationError::Signer),
                "{name}"
            );
        } else {
            let as_device = unsigned_proposal(entry, device_author());
            assert_eq!(
                sign_device_proposal(&as_device, &device_key()),
                Err(AuthorizationError::Signer),
                "{name}"
            );
        }
    }

    let entry = golden_key_entries().remove(0).1;
    let stranger = DeviceKeyPair::from_seed(&[0x58; 32]).unwrap();
    assert_eq!(
        sign_device_proposal(&unsigned_proposal(entry, device_author()), &stranger),
        Err(AuthorizationError::WrongAuthor)
    );
}

#[test]
fn device_signatures_do_not_verify_under_the_admin_domain_or_the_reverse() {
    let device_signed = proposal(golden_key_entries().remove(0).1, 0);
    let encoded = device_signed.encode();
    let unsigned = &encoded[..encoded.len() - 64];

    let mut admin_domain = device_signed.clone();
    admin_domain.signature = device_key().sign(PROPOSAL_DOMAIN, unsigned);
    assert_eq!(
        SignedProposal::decode(&admin_domain.encode()),
        Err(AuthorizationError::Signature)
    );

    let admin_signed = proposal(golden_key_entries().remove(2).1, 0);
    let encoded = admin_signed.encode();
    let unsigned = &encoded[..encoded.len() - 64];
    let mut device_domain = admin_signed.clone();
    device_domain.signature = admin_key().sign(DEVICE_PROPOSAL_DOMAIN, unsigned);
    assert_eq!(
        SignedProposal::decode(&device_domain.encode()),
        Err(AuthorizationError::Signature)
    );
}

#[test]
fn key_epochs_carry_a_chain_link_exactly_after_the_first() {
    let mut first = golden_key_epoch();
    first.key_epoch = 1;
    assert_eq!(
        key_epoch_error(first.clone()),
        AuthorizationError::Codec(CodecError::Malformed("key epoch"))
    );
    first.wrapped_previous = None;
    let proposal = unsigned_proposal(Entry::KeyEpoch(first.clone()), device_author());
    assert!(sign_device_proposal(&proposal, &device_key()).is_ok());

    let mut unchained = golden_key_epoch();
    unchained.wrapped_previous = None;
    assert_eq!(
        key_epoch_error(unchained),
        AuthorizationError::Codec(CodecError::Malformed("key epoch"))
    );

    let mut zero = first;
    zero.key_epoch = 0;
    assert_eq!(
        key_epoch_error(zero),
        AuthorizationError::Codec(CodecError::Malformed("key epoch"))
    );
}

#[test]
fn key_epoch_envelopes_are_nonempty_strictly_ascending_and_bounded() {
    let mut empty = golden_key_epoch();
    empty.envelopes.clear();
    assert_eq!(
        key_epoch_error(empty),
        AuthorizationError::Codec(CodecError::Bounds("key envelopes"))
    );

    let mut reversed = golden_key_epoch();
    reversed.envelopes.reverse();
    assert_eq!(
        key_epoch_error(reversed),
        AuthorizationError::Codec(CodecError::Malformed("key envelope order"))
    );

    let mut duplicated = golden_key_epoch();
    let first = duplicated.envelopes[0].clone();
    duplicated.envelopes.insert(0, first);
    assert_eq!(
        key_epoch_error(duplicated),
        AuthorizationError::Codec(CodecError::Malformed("key envelope order"))
    );

    let mut oversized = golden_key_epoch();
    let template = oversized.envelopes[0].clone();
    oversized.envelopes = (0..=MAX_KEY_RECIPIENTS)
        .map(|index| {
            let mut envelope = template.clone();
            let mut id = [0u8; 16];
            id[8..].copy_from_slice(&(index as u64).to_be_bytes());
            envelope.recipient = crate::KeyId::from_bytes(id);
            envelope
        })
        .collect();
    assert_eq!(
        key_epoch_error(oversized),
        AuthorizationError::Codec(CodecError::Bounds("key envelopes"))
    );

    let key_epoch = golden_key_epoch();
    for envelope in &key_epoch.envelopes {
        let found = [0x71, 0x72, 0x73]
            .iter()
            .map(|seed| encryption_key(*seed))
            .find(|key| key.key_id() == envelope.recipient)
            .unwrap();
        assert_eq!(key_epoch.envelope_for(&found), Some(envelope));
    }
    assert_eq!(key_epoch.envelope_for(&encryption_key(0x79)), None);
}

#[test]
fn a_device_key_envelope_is_addressed_to_the_published_key() {
    let published = encryption_key(0x71);
    let entry = Entry::DeviceKey(DeviceKey {
        device_id: GOLDEN_DEVICE_ID.to_string(),
        encryption_key: published,
    });
    assert!(
        key_record(entry.clone(), Sequencing::DeviceKey { envelope: None })
            .validate()
            .is_ok()
    );
    let misaddressed = Sequencing::DeviceKey {
        envelope: Some(fixed_envelope(&encryption_key(0x72), 0x77)),
    };
    assert_eq!(
        key_record(entry.clone(), misaddressed).validate(),
        Err(AuthorizationError::Codec(CodecError::Malformed(
            "device key envelope"
        )))
    );
    assert_eq!(
        key_record(entry, Sequencing::Empty).validate(),
        Err(AuthorizationError::Codec(CodecError::Malformed(
            "sequencing"
        )))
    );
}

#[test]
fn small_order_encryption_keys_never_decode_in_an_entry() {
    let (_, entry, sequencing) = golden_key_entries().remove(2);
    let record = key_record(entry, sequencing);
    let mut bytes = record.encode();
    let key = *encryption_key(0x73).as_bytes();
    let at = bytes.windows(32).position(|window| window == key).unwrap();
    bytes[at..at + 32].copy_from_slice(&[0u8; 32]);
    assert!(AuthorizationRecord::decode(&bytes).is_err());
}

#[test]
fn server_endpoints_are_bounded_host_and_port_pairs() {
    let server = |endpoints: Vec<&str>| {
        Entry::ServerConfig(ServerConfig {
            server: Some(Server {
                mode: ServerMode::Anchor,
                endpoints: endpoints.into_iter().map(str::to_string).collect(),
                certificate_fingerprint: [0x78; 32],
            }),
        })
    };
    let sign = |entry: Entry| {
        sign_proposal(
            &unsigned_proposal(entry, admin_key().public_key()),
            &admin_key(),
        )
    };
    for valid in [
        vec!["relay.example.org:443"],
        vec!["192.0.2.1:65535", "[::1]:1", "host:7443", "h:2"],
    ] {
        assert!(sign(server(valid.clone())).is_ok(), "{valid:?}");
    }
    for invalid in [
        "relay.example.org",
        ":443",
        "host:",
        "host:0",
        "host:065",
        "host:65536",
        "host:+80",
        "a b:80",
        "host/path:80",
        "user@host:80",
        "::1:80",
        "[::1:80",
        "[not-ipv6]:80",
        "[::1]x:80",
    ] {
        assert_eq!(
            sign(server(vec![invalid])),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "server endpoint"
            ))),
            "{invalid}"
        );
    }
    let long = format!("{}:80", "h".repeat(253));
    assert!(sign(server(vec![&long])).is_err());
    assert_eq!(
        sign(server(Vec::new())),
        Err(AuthorizationError::Codec(CodecError::Bounds(
            "server endpoints"
        )))
    );
    let many = vec!["host:80"; MAX_SERVER_ENDPOINTS + 1];
    assert_eq!(
        sign(server(many)),
        Err(AuthorizationError::Codec(CodecError::Bounds(
            "server endpoints"
        )))
    );
}
