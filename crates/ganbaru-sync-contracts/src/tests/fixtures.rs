use crate::signing::Digest32;
use crate::{
    AdminKeyPair, AuthorizationRecord, Cause, Change, ChangeAction, Charter, Content,
    DeviceKeyPair, DeviceKind, DeviceRole, Enroll, Entry, Envelope, Field, GroupId, GroupValue,
    Handover, Header, Hlc, LateResolution, LateRow, Operation, OperationKind, Proposal,
    RecoverySeed, Remove, ResolveLate, RowKey, Sequencing, SetRole, SignedCertificate,
    SignedCharter, SignedProposal, SpaceId, TableId, Value, WriterCertificate, WriterId,
    WriterKeyPair, WriterPublicKey, sign_certificate, sign_charter, sign_proposal,
};
use ganbaru_contacts::PersonKeyPair;

pub const PERSON_SEED: [u8; 32] = [0x11; 32];
pub const WRITER_SEED: [u8; 32] = [0x22; 32];
pub const OTHER_WRITER_SEED: [u8; 32] = [0x33; 32];
pub const GOLDEN_VAULT_ID: &str = "vault-golden";
pub const GOLDEN_DEVICE_ID: &str = "device-a.desktop";
pub const GOLDEN_CREATED_AT_MS: i64 = 1_790_000_000_000;

pub fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&PERSON_SEED).unwrap()
}

pub fn writer_key() -> WriterKeyPair {
    WriterKeyPair::from_seed(&WRITER_SEED).unwrap()
}

pub fn other_writer_key() -> WriterKeyPair {
    WriterKeyPair::from_seed(&OTHER_WRITER_SEED).unwrap()
}

pub fn writer_id() -> WriterId {
    WriterId::for_public_key(&writer_key().public_key())
}

pub fn other_writer_id() -> WriterId {
    WriterId::for_public_key(&other_writer_key().public_key())
}

pub fn space() -> SpaceId {
    SpaceId::personal(GOLDEN_VAULT_ID)
}

pub fn certificate_for(key: &WriterPublicKey, predecessor: Option<WriterId>) -> SignedCertificate {
    let certificate = WriterCertificate {
        writer_key: *key,
        device_id: GOLDEN_DEVICE_ID.to_string(),
        device_key: device_key().public_key(),
        created_at_ms: GOLDEN_CREATED_AT_MS,
        predecessor,
    };
    sign_certificate(&certificate, &device_key()).unwrap()
}

pub fn header(clock_ms: u64, dependencies: Vec<(WriterId, u64)>) -> Header {
    Header {
        clock: Hlc::new(clock_ms, 0),
        manifest_version: 1,
        authorization_revision: 0,
        key_epoch: 0,
        group_continues: false,
        dependencies,
    }
}

pub fn genesis_operation() -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 1,
        previous_hash: [0u8; 32],
        header: header(GOLDEN_CREATED_AT_MS as u64, Vec::new()),
        content: Content::Genesis(certificate_for(
            &writer_key().public_key(),
            Some(other_writer_id()),
        )),
    }
}

pub fn group(id: u8) -> GroupId {
    GroupId::new(id).unwrap()
}

pub fn row(key: &str) -> RowKey {
    RowKey::new(key).unwrap()
}

pub fn value(fields: Vec<Field>) -> Value {
    Value::new(fields).unwrap()
}

pub fn group_value(table: u16, id: u8, fields: Vec<Field>) -> GroupValue {
    GroupValue::new(TableId(table), group(id), value(fields))
}

/// A create of a tag, a write of a note, and a tombstone with a redirect.
pub fn changes_operation(previous_hash: Digest32) -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 2,
        previous_hash,
        header: header(
            GOLDEN_CREATED_AT_MS as u64 + 1_000,
            vec![(other_writer_id(), 7)],
        ),
        content: Content::Changes(vec![
            Change {
                table: TableId(1),
                row: row("tag-work"),
                action: ChangeAction::Create,
                groups: vec![
                    group_value(1, 0, vec![Field::Text("2026-10-08T12:00:00.000Z".into())]),
                    group_value(1, 1, vec![Field::Text("Work".into())]),
                    group_value(1, 2, vec![Field::Text("a0".into())]),
                    group_value(1, 3, vec![Field::Text("2026-10-08T12:00:00.000Z".into())]),
                ],
                replaced_by: None,
                cause: None,
            },
            Change {
                table: TableId(2),
                row: row("note-1"),
                action: ChangeAction::Write,
                groups: vec![
                    group_value(2, 2, vec![Field::Blob(vec![0, 1, 2, 3, 255])]),
                    group_value(
                        2,
                        5,
                        vec![Field::Integer(0), Field::Integer(1), Field::Null],
                    ),
                ],
                replaced_by: None,
                cause: None,
            },
            Change {
                table: TableId(1),
                row: row("tag-work-duplicate"),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: Some(row("tag-work")),
                cause: None,
            },
        ]),
    }
}

/// A tombstone naming its cause beside one without a successor, the version 2 vector.
pub fn cause_operation(previous_hash: Digest32) -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 2,
        previous_hash,
        header: header(GOLDEN_CREATED_AT_MS as u64 + 1_000, Vec::new()),
        content: Content::Changes(vec![
            Change {
                table: TableId(60_000),
                row: row("parent-1"),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: None,
                cause: None,
            },
            Change {
                table: TableId(60_001),
                row: row("child-1"),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: None,
                cause: Some(Cause {
                    table: TableId(60_000),
                    row: row("parent-1"),
                }),
            },
        ]),
    }
}

/// The version 2 golden operation, sealed after the golden genesis.
pub fn golden_cause() -> (Operation, Vec<u8>) {
    let key = writer_key();
    let genesis = genesis_operation().seal(&key).unwrap();
    let operation = cause_operation(genesis.hash());
    let sealed = operation.seal(&key).unwrap();
    (operation, sealed.into_bytes())
}

/// The cause operation marked as continuing its apply group, which seals at version 4.
pub fn golden_group() -> (Operation, Vec<u8>) {
    let key = writer_key();
    let genesis = genesis_operation().seal(&key).unwrap();
    let mut operation = cause_operation(genesis.hash());
    operation.header.group_continues = true;
    let sealed = operation.seal(&key).unwrap();
    (operation, sealed.into_bytes())
}

pub fn acknowledgement_operation(previous_hash: Digest32) -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 3,
        previous_hash,
        header: header(
            GOLDEN_CREATED_AT_MS as u64 + 3_000,
            vec![(other_writer_id(), 42)],
        ),
        content: Content::Acknowledgement,
    }
}

/// The three golden operations as a chain: genesis, changes, acknowledgement.
pub fn golden_chain() -> Vec<(Operation, Vec<u8>)> {
    let key = writer_key();
    let genesis = genesis_operation();
    let genesis_sealed = genesis.seal(&key).unwrap();
    let changes = changes_operation(genesis_sealed.hash());
    let changes_sealed = changes.seal(&key).unwrap();
    let acknowledgement = acknowledgement_operation(changes_sealed.hash());
    let acknowledgement_sealed = acknowledgement.seal(&key).unwrap();
    vec![
        (genesis, genesis_sealed.into_bytes()),
        (changes, changes_sealed.into_bytes()),
        (acknowledgement, acknowledgement_sealed.into_bytes()),
    ]
}

/// Full verification as a replica performs it: envelope, signature, and payload.
pub fn fully_verifies(bytes: &[u8], key: &WriterPublicKey) -> bool {
    let Ok(envelope) = Envelope::decode(bytes) else {
        return false;
    };
    if envelope.kind == OperationKind::Genesis.as_u8() {
        return envelope.genesis().is_ok();
    }
    envelope.verify(key) && envelope.operation().is_ok()
}

pub const DEVICE_SEED: [u8; 32] = [0x44; 32];
pub const ADMIN_SEED: [u8; 32] = [0x55; 32];
pub const RECOVERY_SEED: [u8; 32] = [0x66; 32];
pub const ENROLLED_DEVICE_ID: &str = "device-b.phone";
pub const NEXT_ANCHOR_DEVICE_ID: &str = "device-c.desktop";

pub fn device_key() -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&DEVICE_SEED).unwrap()
}

pub fn admin_key() -> AdminKeyPair {
    AdminKeyPair::from_seed(&ADMIN_SEED).unwrap()
}

pub fn recovery_seed() -> RecoverySeed {
    RecoverySeed::from_bytes(RECOVERY_SEED)
}

pub fn golden_charter() -> SignedCharter {
    let charter = Charter {
        space: space(),
        anchor_device_id: GOLDEN_DEVICE_ID.to_string(),
        anchor_device_key: device_key().public_key(),
        anchor_tls_fingerprint: [0xa1; 32],
        anchor_admin_key: admin_key().public_key(),
        recovery_admin_key: recovery_seed().recovery_admin_key().unwrap().public_key(),
        discovery_key: [0xd1; 32],
        created_at_ms: GOLDEN_CREATED_AT_MS,
    };
    sign_charter(&charter, &person()).unwrap()
}

pub fn proposal(entry: Entry, created_at_offset_ms: i64) -> SignedProposal {
    let proposal = Proposal {
        space: space(),
        charter: golden_charter().hash(),
        author: admin_key().public_key(),
        created_at_ms: GOLDEN_CREATED_AT_MS + created_at_offset_ms,
        entry,
    };
    sign_proposal(&proposal, &admin_key()).unwrap()
}

pub fn enroll_entry() -> Entry {
    Entry::Enroll(Enroll {
        device_id: ENROLLED_DEVICE_ID.to_string(),
        device_key: DeviceKeyPair::from_seed(&[0x45; 32]).unwrap().public_key(),
        tls_fingerprint: [0xb2; 32],
        label: "Ana's phone".to_string(),
        device_kind: DeviceKind::Mobile,
        role: DeviceRole::Member,
    })
}

/// One golden entry of every known kind, with its sequencing, in log order.
pub fn golden_entries() -> Vec<(&'static str, Entry, Sequencing)> {
    vec![
        ("authorization-enroll", enroll_entry(), Sequencing::Empty),
        (
            "authorization-set-role",
            Entry::SetRole(SetRole::Device {
                device_id: GOLDEN_DEVICE_ID.to_string(),
                role: DeviceRole::Admin(admin_key().public_key()),
            }),
            Sequencing::Empty,
        ),
        (
            "authorization-set-recovery",
            Entry::SetRole(SetRole::Recovery {
                recovery_admin_key: AdminKeyPair::from_seed(&[0x56; 32]).unwrap().public_key(),
            }),
            Sequencing::Empty,
        ),
        (
            "authorization-remove",
            Entry::Remove(Remove {
                device_id: ENROLLED_DEVICE_ID.to_string(),
            }),
            Sequencing::Removal {
                cutoffs: vec![(writer_id_for_index(1), 12), (writer_id_for_index(2), 3)],
                discovery_key: [0xd2; 32],
            },
        ),
        (
            "authorization-resolve-late",
            Entry::ResolveLate(ResolveLate {
                removal_revision: 4,
                rows: vec![
                    LateRow {
                        table: TableId(1),
                        row: row("tag-late"),
                        resolution: LateResolution::Restore,
                    },
                    LateRow {
                        table: TableId(2),
                        row: row("note-late"),
                        resolution: LateResolution::Discard,
                    },
                ],
            }),
            Sequencing::Empty,
        ),
    ]
}

pub fn writer_id_for_index(index: u16) -> WriterId {
    let mut bytes = [0u8; 16];
    bytes[14..].copy_from_slice(&index.to_be_bytes());
    WriterId::from_bytes(bytes)
}

pub fn authorization_operation(
    writer: WriterId,
    seq: u64,
    previous_hash: Digest32,
    record: AuthorizationRecord,
) -> Operation {
    let mut operation_header = header(
        GOLDEN_CREATED_AT_MS as u64 + 10_000 + record.revision * 1_000,
        Vec::new(),
    );
    operation_header.authorization_revision = record.revision - 1;
    Operation {
        space: space(),
        writer,
        seq,
        previous_hash,
        header: operation_header,
        content: Content::Authorization(Box::new(record)),
    }
}

/// The golden authorization log after the golden charter: one operation per known entry kind
/// sealed by the anchor's writer after its genesis, then a handover sealed by the next anchor's
/// writer after its own genesis. Each item is the vector name, the operation, its bytes, and the
/// sealing writer key.
pub fn golden_authorization_log() -> Vec<(&'static str, Operation, Vec<u8>, WriterPublicKey)> {
    let anchor = writer_key();
    let mut previous_operation = genesis_operation().seal(&anchor).unwrap().hash();
    let mut previous_entry = golden_charter().hash();
    let mut log = Vec::new();
    for (index, (name, entry, sequencing)) in golden_entries().into_iter().enumerate() {
        let revision = index as u64 + 1;
        let record = AuthorizationRecord {
            epoch: 0,
            revision,
            previous_entry_hash: previous_entry,
            anchor_device_id: GOLDEN_DEVICE_ID.to_string(),
            proposal: proposal(entry, revision as i64),
            sequencing,
        };
        previous_entry = record.hash();
        let operation =
            authorization_operation(writer_id(), revision + 1, previous_operation, record);
        let sealed = operation.seal(&anchor).unwrap();
        previous_operation = sealed.hash();
        log.push((name, operation, sealed.into_bytes(), anchor.public_key()));
    }

    let next = other_writer_key();
    let next_genesis = Operation {
        space: space(),
        writer: other_writer_id(),
        seq: 1,
        previous_hash: [0u8; 32],
        header: header(GOLDEN_CREATED_AT_MS as u64, Vec::new()),
        content: Content::Genesis(certificate_for(&next.public_key(), None)),
    };
    let revision = log.len() as u64 + 1;
    let record = AuthorizationRecord {
        epoch: 1,
        revision,
        previous_entry_hash: previous_entry,
        anchor_device_id: NEXT_ANCHOR_DEVICE_ID.to_string(),
        proposal: proposal(
            Entry::Handover(Handover {
                new_anchor_device_id: NEXT_ANCHOR_DEVICE_ID.to_string(),
                epoch: 1,
                continues_from_revision: revision - 1,
                continues_from_hash: previous_entry,
            }),
            revision as i64,
        ),
        sequencing: Sequencing::Handover {
            old_anchor_vector: Some(
                [(writer_id(), revision), (other_writer_id(), 1)]
                    .into_iter()
                    .collect(),
            ),
        },
    };
    let operation = authorization_operation(
        other_writer_id(),
        2,
        next_genesis.seal(&next).unwrap().hash(),
        record,
    );
    let sealed = operation.seal(&next).unwrap();
    log.push((
        "authorization-handover",
        operation,
        sealed.into_bytes(),
        next.public_key(),
    ));
    log
}

/// Deterministic xorshift generator for mutation and fuzz tests.
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }

    pub fn byte(&mut self) -> u8 {
        self.next_u64() as u8
    }
}
