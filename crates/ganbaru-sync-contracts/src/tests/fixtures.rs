use crate::signing::Digest32;
use crate::{
    Change, ChangeAction, Content, Envelope, Field, GroupId, GroupValue, Header, Hlc, Operation,
    OperationKind, Revocation, RevokeReason, RowKey, SignedCertificate, SpaceId, TableId, Value,
    WriterCertificate, WriterId, WriterKeyPair, WriterPublicKey, sign_certificate,
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
        person_key: person().public_key(),
        device_id: GOLDEN_DEVICE_ID.to_string(),
        created_at_ms: GOLDEN_CREATED_AT_MS,
        predecessor,
    };
    sign_certificate(&certificate, &person()).unwrap()
}

pub fn header(clock_ms: u64, dependencies: Vec<(WriterId, u64)>) -> Header {
    Header {
        clock: Hlc::new(clock_ms, 0),
        manifest_version: 1,
        authorization_revision: 0,
        key_epoch: 0,
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
            },
            Change {
                table: TableId(1),
                row: row("tag-work-duplicate"),
                action: ChangeAction::Tombstone,
                groups: Vec::new(),
                replaced_by: Some(row("tag-work")),
            },
        ]),
    }
}

pub fn revoke_operation(previous_hash: Digest32) -> Operation {
    Operation {
        space: space(),
        writer: writer_id(),
        seq: 3,
        previous_hash,
        header: header(GOLDEN_CREATED_AT_MS as u64 + 2_000, Vec::new()),
        content: Content::Revoke(Revocation {
            writer: other_writer_id(),
            cutoff: 41,
            reason: RevokeReason::Forked,
        }),
    }
}

/// The three golden operations as a chain: genesis, changes, revoke.
pub fn golden_chain() -> Vec<(Operation, Vec<u8>)> {
    let key = writer_key();
    let genesis = genesis_operation();
    let genesis_sealed = genesis.seal(&key).unwrap();
    let changes = changes_operation(genesis_sealed.hash());
    let changes_sealed = changes.seal(&key).unwrap();
    let revoke = revoke_operation(changes_sealed.hash());
    let revoke_sealed = revoke.seal(&key).unwrap();
    vec![
        (genesis, genesis_sealed.into_bytes()),
        (changes, changes_sealed.into_bytes()),
        (revoke, revoke_sealed.into_bytes()),
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
