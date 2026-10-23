use super::fixtures::{
    GOLDEN_CREATED_AT_MS, GOLDEN_DEVICE_ID, admin_key, authorization_operation, device_key,
    enroll_entry, golden_authorization_log, golden_charter, other_writer_id, person, proposal,
    space, writer_id, writer_key,
};
use super::golden::assert_vector;
use crate::authorization::entry_hash;
use crate::codec::{CodecError, Writer};
use crate::ids::hex;
use crate::op::{AUTHORIZATION_FORMAT_VERSION, FORMAT_VERSION, OPERATION_MAGIC};
use crate::signing::{CHARTER_HASH_DOMAIN, PROPOSAL_DOMAIN, domain_digest};
use crate::{
    AdminKeyPair, AuthorizationError, AuthorizationRecord, CharterError, Content, DeviceRole,
    Enroll, Entry, Envelope, Handover, LateResolution, LateRow, OperationError, OperationKind,
    PayloadError, Proposal, ProposalRejection, Remove, ResolveLate, Sequencing, SignedCharter,
    SignedProposal, TableId, sign_proposal,
};
use ganbaru_contacts::PersonKeyPair;
use std::collections::BTreeSet;

fn record(entry: Entry, sequencing: Sequencing, revision: u64) -> AuthorizationRecord {
    AuthorizationRecord {
        epoch: 0,
        revision,
        previous_entry_hash: [0x0c; 32],
        anchor_device_id: GOLDEN_DEVICE_ID.to_string(),
        proposal: proposal(entry, 0),
        sequencing,
    }
}

fn remove_record(cutoffs: Vec<(crate::WriterId, u64)>) -> AuthorizationRecord {
    record(
        Entry::Remove(Remove {
            device_id: "device-b.phone".to_string(),
        }),
        Sequencing::Removal {
            cutoffs,
            discovery_key: [0xd3; 32],
        },
        2,
    )
}

fn codec_error(result: Result<impl Sized, OperationError>) -> CodecError {
    match result {
        Err(OperationError::Codec(error))
        | Err(OperationError::Authorization(AuthorizationError::Codec(error))) => error,
        Err(error) => panic!("unexpected error {error}"),
        Ok(_) => panic!("an invalid authorization operation sealed"),
    }
}

#[test]
fn charter_certificate_and_proposal_encode_to_frozen_bytes() {
    let charter = golden_charter();
    assert_vector("charter", &hex(&charter.encode()));
    let certificate =
        super::fixtures::certificate_v2_for(&writer_key().public_key(), Some(other_writer_id()));
    assert_vector("certificate-v2", &hex(&certificate.encode()));
    assert_vector(
        "proposal-enroll",
        &hex(&proposal(enroll_entry(), 1).encode()),
    );
}

#[test]
fn authorization_operations_encode_to_frozen_bytes() {
    for (name, _, bytes, _) in golden_authorization_log() {
        assert_vector(name, &hex(&bytes));
    }
}

#[test]
fn authorization_operations_decode_reencode_and_chain_from_the_charter() {
    let mut previous_entry = golden_charter().hash();
    let mut epoch = 0;
    for (name, operation, bytes, key) in golden_authorization_log() {
        let envelope = Envelope::decode(&bytes).unwrap();
        assert_eq!(
            envelope.format_version, AUTHORIZATION_FORMAT_VERSION,
            "{name}"
        );
        assert_eq!(envelope.kind, OperationKind::Authorization.as_u8());
        assert!(envelope.verify(&key), "{name}");
        let decoded = envelope.operation().unwrap();
        assert_eq!(decoded, operation, "{name}");
        let Content::Authorization(record) = &decoded.content else {
            panic!("{name} is not an authorization");
        };
        assert_eq!(record.previous_entry_hash, previous_entry, "{name}");
        assert_eq!(
            decoded.header.authorization_revision + 1,
            record.revision,
            "{name}"
        );
        assert!(record.epoch >= epoch);
        assert!(envelope.header.ends_with(&record.hash()), "{name}");
        assert_eq!(envelope.body, record.encode().as_slice());
        assert_eq!(entry_hash(envelope.body), record.hash());
        assert_eq!(
            AuthorizationRecord::decode(envelope.body).unwrap(),
            **record
        );
        previous_entry = record.hash();
        epoch = record.epoch;
        let resealed = if key == writer_key().public_key() {
            decoded.seal(&writer_key()).unwrap()
        } else {
            decoded.seal(&super::fixtures::other_writer_key()).unwrap()
        };
        assert_eq!(resealed.bytes(), bytes.as_slice(), "{name}");
    }
    assert_eq!(epoch, 1);
}

#[test]
fn the_golden_log_covers_every_known_entry_kind() {
    let kinds: BTreeSet<u8> = golden_authorization_log()
        .iter()
        .map(|(_, operation, _, _)| match &operation.content {
            Content::Authorization(record) => record.proposal.proposal.entry.kind(),
            _ => panic!("not an authorization"),
        })
        .collect();
    assert_eq!(kinds, (0..=4).collect());
}

#[test]
fn a_body_that_does_not_hash_to_the_signed_entry_hash_is_body_damage() {
    let log = golden_authorization_log();
    let (_, _, bytes, key) = &log[0];
    let (_, _, other, _) = &log[1];
    let envelope = Envelope::decode(bytes).unwrap();
    let swapped_body = Envelope::decode(other).unwrap().body;
    let mut forged = envelope.signed.to_vec();
    forged.extend_from_slice(&(swapped_body.len() as u32).to_be_bytes());
    forged.extend_from_slice(swapped_body);
    forged.extend_from_slice(envelope.signature);
    let forged = Envelope::decode(&forged).unwrap();
    assert!(forged.verify(key));
    assert_eq!(
        forged.operation(),
        Err(PayloadError::Body(CodecError::Malformed(
            "authorization entry hash"
        )))
    );
}

#[test]
fn authorization_is_newer_than_every_older_reader() {
    let log = golden_authorization_log();
    let envelope = Envelope::decode(&log[0].2).unwrap();
    let mut writer = Writer::default();
    writer.raw(OPERATION_MAGIC);
    writer.u8(FORMAT_VERSION + 1);
    writer.u8(envelope.kind);
    writer.raw(envelope.space.as_bytes());
    writer.raw(envelope.writer.as_bytes());
    writer.u64(envelope.seq);
    writer.raw(&envelope.previous_hash);
    writer.bytes_u32(envelope.header);
    let signature = writer_key().sign_operation(writer.as_bytes());
    writer.bytes_u32(envelope.body);
    writer.raw(&signature);
    let newer = writer.into_bytes();
    let newer = Envelope::decode(&newer).unwrap();
    assert!(newer.verify(&writer_key().public_key()));
    assert_eq!(newer.operation(), Err(PayloadError::NewerFormat));
}

#[test]
fn the_header_revision_is_the_revision_before_the_record() {
    let log = golden_authorization_log();
    let mut operation = log[0].1.clone();
    assert_eq!(operation.header.authorization_revision, 0);
    for revision in [1, 2] {
        operation.header.authorization_revision = revision;
        assert_eq!(
            codec_error(operation.seal(&writer_key())),
            CodecError::Malformed("authorization revision")
        );
    }
    let mut foreign = operation.clone();
    foreign.header.authorization_revision = 0;
    if let Content::Authorization(record) = &mut foreign.content {
        let mut entry = record.proposal.proposal.clone();
        entry.space = crate::SpaceId::personal("another-vault");
        record.proposal = sign_proposal(&entry, &admin_key()).unwrap();
    }
    assert_eq!(
        codec_error(foreign.seal(&writer_key())),
        CodecError::Malformed("authorization space")
    );
}

#[test]
fn revision_zero_belongs_to_the_charter() {
    let mut zero = record(enroll_entry(), Sequencing::Empty, 1);
    zero.revision = 0;
    assert_eq!(
        zero.validate(),
        Err(AuthorizationError::Codec(CodecError::Malformed(
            "authorization revision"
        )))
    );
}

#[test]
fn a_handover_continues_the_log_it_names() {
    let continues = [0x0c; 32];
    let handover = |epoch, device: &str, from_revision, from_hash| {
        Entry::Handover(Handover {
            new_anchor_device_id: device.to_string(),
            epoch,
            continues_from_revision: from_revision,
            continues_from_hash: from_hash,
        })
    };
    let sequencing = Sequencing::Handover {
        old_anchor_vector: None,
    };
    let valid = |entry| {
        let mut record = record(entry, sequencing.clone(), 5);
        record.epoch = 1;
        record
    };
    assert!(
        valid(handover(1, GOLDEN_DEVICE_ID, 4, continues))
            .validate()
            .is_ok()
    );
    for entry in [
        handover(2, GOLDEN_DEVICE_ID, 4, continues),
        handover(1, "device-z.desktop", 4, continues),
        handover(1, GOLDEN_DEVICE_ID, 3, continues),
        handover(1, GOLDEN_DEVICE_ID, 4, [0x0d; 32]),
    ] {
        assert_eq!(
            valid(entry).validate(),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "handover continuity"
            )))
        );
    }
    assert!(matches!(
        sign_proposal(
            &Proposal {
                space: space(),
                author: admin_key().public_key(),
                created_at_ms: GOLDEN_CREATED_AT_MS,
                entry: handover(0, GOLDEN_DEVICE_ID, 4, continues),
            },
            &admin_key()
        ),
        Err(AuthorizationError::Codec(_))
    ));
}

#[test]
fn sequencing_matches_the_entry_kind() {
    let mismatched = [
        record(enroll_entry(), remove_record(Vec::new()).sequencing, 1),
        record(
            Entry::Remove(Remove {
                device_id: "device-b.phone".to_string(),
            }),
            Sequencing::Empty,
            1,
        ),
        record(
            enroll_entry(),
            Sequencing::Handover {
                old_anchor_vector: None,
            },
            1,
        ),
        record(enroll_entry(), Sequencing::Opaque(vec![1]), 1),
    ];
    for record in mismatched {
        assert_eq!(
            record.validate(),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "sequencing"
            )))
        );
    }
}

#[test]
fn removal_cutoffs_are_ascending_nonzero_and_bounded() {
    let low = crate::WriterId::from_bytes([1; 16]);
    let high = crate::WriterId::from_bytes([2; 16]);
    assert!(remove_record(vec![(low, 1), (high, 9)]).validate().is_ok());
    assert!(remove_record(Vec::new()).validate().is_ok());
    for cutoffs in [
        vec![(high, 1), (low, 1)],
        vec![(low, 1), (low, 2)],
        vec![(low, 0)],
    ] {
        assert_eq!(
            remove_record(cutoffs).validate(),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "removal cutoffs"
            )))
        );
    }
}

#[test]
fn unknown_entry_kinds_round_trip_byte_for_byte() {
    let entry = Entry::Unknown {
        kind: 0xc8,
        bytes: b"a future entry".to_vec(),
    };
    let signed = proposal(entry.clone(), 0);
    let decoded = SignedProposal::decode(&signed.encode()).unwrap();
    assert_eq!(decoded, signed);
    assert_eq!(decoded.encode(), signed.encode());

    let mut future = record(entry, Sequencing::Opaque(b"future sequencing".to_vec()), 1);
    future.previous_entry_hash = golden_charter().hash();
    let operation = authorization_operation(writer_id(), 2, [0x0e; 32], future.clone());
    let sealed = operation.seal(&writer_key()).unwrap();
    let envelope = Envelope::decode(sealed.bytes()).unwrap();
    let decoded = envelope.operation().unwrap();
    assert_eq!(decoded, operation);
    assert_eq!(decoded.seal(&writer_key()).unwrap().bytes(), sealed.bytes());
    assert_eq!(future.hash(), entry_hash(envelope.body));

    for kind in 0..=4 {
        let entry = Entry::Unknown {
            kind,
            bytes: Vec::new(),
        };
        let proposal = Proposal {
            space: space(),
            author: admin_key().public_key(),
            created_at_ms: GOLDEN_CREATED_AT_MS,
            entry,
        };
        assert_eq!(
            sign_proposal(&proposal, &admin_key()),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "entry kind"
            )))
        );
    }
}

#[test]
fn proposals_are_idempotent_by_hash_and_signed_by_their_author() {
    let first = proposal(enroll_entry(), 0);
    let again = proposal(enroll_entry(), 0);
    assert_eq!(first.hash(), again.hash());
    assert_ne!(first.hash(), proposal(enroll_entry(), 1).hash());

    let stranger = AdminKeyPair::from_seed(&[0x57; 32]).unwrap();
    assert_eq!(
        sign_proposal(&first.proposal, &stranger),
        Err(AuthorizationError::WrongAuthor)
    );

    let mut forged = first.clone();
    let unsigned = first.encode();
    forged.signature = stranger.sign(PROPOSAL_DOMAIN, &unsigned[..unsigned.len() - 64]);
    assert_eq!(
        SignedProposal::decode(&forged.encode()),
        Err(AuthorizationError::Signature)
    );
    let mut version = first.encode();
    version[0] = 2;
    assert_eq!(
        SignedProposal::decode(&version),
        Err(AuthorizationError::Version)
    );
}

#[test]
fn enrollment_fields_are_validated() {
    let Entry::Enroll(valid) = enroll_entry() else {
        panic!("enroll entry");
    };
    let invalid = [
        (
            Enroll {
                label: "evil\u{202E}label".to_string(),
                ..valid.clone()
            },
            AuthorizationError::Label,
        ),
        (
            Enroll {
                label: " ".to_string(),
                ..valid.clone()
            },
            AuthorizationError::Label,
        ),
        (
            Enroll {
                label: "l".repeat(129),
                ..valid.clone()
            },
            AuthorizationError::Label,
        ),
        (
            Enroll {
                device_id: "bad id".to_string(),
                ..valid.clone()
            },
            AuthorizationError::DeviceId,
        ),
    ];
    for (enroll, error) in invalid {
        let proposal = Proposal {
            space: space(),
            author: admin_key().public_key(),
            created_at_ms: GOLDEN_CREATED_AT_MS,
            entry: Entry::Enroll(enroll),
        };
        assert_eq!(sign_proposal(&proposal, &admin_key()), Err(error));
    }
    let admin = Enroll {
        role: DeviceRole::Admin(admin_key().public_key()),
        ..valid
    };
    let signed = proposal(Entry::Enroll(admin), 0);
    assert_eq!(SignedProposal::decode(&signed.encode()).unwrap(), signed);
}

#[test]
fn late_resolutions_are_ascending_unique_and_nonempty() {
    let row = |table, key: &str, resolution| LateRow {
        table: TableId(table),
        row: super::fixtures::row(key),
        resolution,
    };
    let resolve = |rows| {
        Entry::ResolveLate(ResolveLate {
            removal_revision: 3,
            rows,
        })
    };
    let attempt = |entry| {
        sign_proposal(
            &Proposal {
                space: space(),
                author: admin_key().public_key(),
                created_at_ms: GOLDEN_CREATED_AT_MS,
                entry,
            },
            &admin_key(),
        )
    };
    assert!(
        attempt(resolve(vec![
            row(1, "b", LateResolution::Restore),
            row(2, "a", LateResolution::Discard),
        ]))
        .is_ok()
    );
    for rows in [
        vec![
            row(2, "a", LateResolution::Restore),
            row(1, "b", LateResolution::Discard),
        ],
        vec![
            row(1, "a", LateResolution::Restore),
            row(1, "a", LateResolution::Discard),
        ],
    ] {
        assert_eq!(
            attempt(resolve(rows)),
            Err(AuthorizationError::Codec(CodecError::Malformed(
                "late resolution order"
            )))
        );
    }
    assert_eq!(
        attempt(resolve(Vec::new())),
        Err(AuthorizationError::Codec(CodecError::Bounds(
            "late resolutions"
        )))
    );
}

#[test]
fn rejection_reasons_are_a_closed_stable_set() {
    let mut names = BTreeSet::new();
    for (index, reason) in ProposalRejection::ALL.iter().enumerate() {
        assert_eq!(usize::from(reason.as_u8()), index);
        assert_eq!(ProposalRejection::from_u8(reason.as_u8()), Some(*reason));
        assert!(names.insert(reason.as_str()));
    }
    assert_eq!(
        ProposalRejection::from_u8(ProposalRejection::ALL.len() as u8),
        None
    );
}

#[test]
fn charters_decode_before_the_person_key_and_verify_against_it() {
    let charter = golden_charter();
    let decoded = SignedCharter::decode(&charter.encode()).unwrap();
    assert_eq!(decoded, charter);
    assert_eq!(decoded.verify(&person().public_key()), Ok(()));
    let (_, stranger) = PersonKeyPair::generate().unwrap();
    assert_eq!(
        decoded.verify(&stranger.public_key()),
        Err(CharterError::Signature)
    );
    assert_eq!(
        charter.hash(),
        domain_digest(CHARTER_HASH_DOMAIN, &[&charter.encode()])
    );
    assert_eq!(charter.charter.anchor_device_key, device_key().public_key());
    let mut version = charter.encode();
    version[0] = 2;
    assert_eq!(SignedCharter::decode(&version), Err(CharterError::Version));
    let mut trailing = charter.encode();
    trailing.push(0);
    assert!(SignedCharter::decode(&trailing).is_err());
}
