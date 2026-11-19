use crate::server::unix_time_ms;
use crate::store::{AdminChange, ClaimRequest, RelayStore, RetentionPolicy, StoreError};
use ganbaru_handoff::protocol::relay::{
    RelayAssetOutcome, RelayPushOutcome, RelayRefusalCode, RelaySetupToken,
};
use ganbaru_sync_contracts::relay::RELAY_NONCE_BYTES;
use ganbaru_sync_contracts::{
    AdminKeyPair, RelayAccessKeyPair, RelayAssetHeader, RelayAssetId, RelayRecordHeader, SpaceId,
    SpaceKey, WriterId,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = ganbaru_atomic_files::create_private_temporary_directory(
            &std::env::temp_dir(),
            "ganbaru-relay-test-",
        )
        .unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn open_store(dir: &TempDir, retention: RetentionPolicy) -> RelayStore {
    RelayStore::open(&dir.path().join("relay.sqlite"), retention)
        .await
        .unwrap()
}

fn space_id(byte: u8) -> SpaceId {
    SpaceId::from_bytes([byte; 16])
}

fn writer_id(byte: u8) -> WriterId {
    WriterId::from_bytes([byte; 16])
}

fn admin() -> AdminKeyPair {
    AdminKeyPair::generate().unwrap().1
}

fn access(space: &SpaceId, epoch: u32) -> RelayAccessKeyPair {
    SpaceKey::generate(epoch)
        .unwrap()
        .relay_access_key(space)
        .unwrap()
}

/// A well-formed record encoding; the relay never opens the ciphertext.
fn record(
    space: SpaceId,
    writer: WriterId,
    seq: u64,
    epoch: u32,
    fill: u8,
    size: usize,
) -> Vec<u8> {
    let header = RelayRecordHeader {
        space,
        writer,
        seq,
        epoch,
    };
    let mut bytes = header.encode().to_vec();
    bytes.extend_from_slice(&[7u8; RELAY_NONCE_BYTES]);
    bytes.extend(std::iter::repeat_n(fill, size));
    bytes
}

fn header_of(bytes: &[u8]) -> RelayRecordHeader {
    *ganbaru_sync_contracts::RelayRecord::decode(bytes)
        .unwrap()
        .header()
}

fn chunk(space: SpaceId, asset: [u8; 32], epoch: u32, index: u32, count: u32, fill: u8) -> Vec<u8> {
    let header = RelayAssetHeader {
        space,
        asset: RelayAssetId::from_bytes(asset),
        epoch,
        index,
        count,
    };
    let mut bytes = header.encode().to_vec();
    bytes.extend_from_slice(&[9u8; RELAY_NONCE_BYTES]);
    bytes.extend(std::iter::repeat_n(fill, 64));
    bytes
}

fn chunk_header(bytes: &[u8]) -> RelayAssetHeader {
    *ganbaru_sync_contracts::RelayAssetChunk::decode(bytes)
        .unwrap()
        .header()
}

fn code_of<T: std::fmt::Debug>(result: Result<T, StoreError>) -> RelayRefusalCode {
    match result {
        Err(StoreError::Refused(code, _)) => code,
        Err(StoreError::StaleAdminCounter(_)) => RelayRefusalCode::AdminCounter,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

async fn claimed_store(
    dir: &TempDir,
    retention: RetentionPolicy,
    space: SpaceId,
    owner: &AdminKeyPair,
    first: &RelayAccessKeyPair,
) -> RelayStore {
    let store = open_store(dir, retention).await;
    let token = RelaySetupToken::generate().unwrap();
    store.add_setup_token(&token, DAY_MS).await.unwrap();
    let admins = [owner.public_key()];
    store
        .claim(
            &ClaimRequest {
                space,
                token: &token,
                epoch: 1,
                access_key: first.public_key(),
                admins: &admins,
                counter: 0,
            },
            1,
        )
        .await
        .unwrap();
    store
}

async fn claim_with(
    store: &RelayStore,
    space: SpaceId,
    token: &RelaySetupToken,
    first: &RelayAccessKeyPair,
    owner: &AdminKeyPair,
) -> Result<(), StoreError> {
    let admins = [owner.public_key()];
    store
        .claim(
            &ClaimRequest {
                space,
                token,
                epoch: 1,
                access_key: first.public_key(),
                admins: &admins,
                counter: 0,
            },
            unix_time_ms(),
        )
        .await
}

fn have(entries: &[(WriterId, u64)]) -> HashMap<WriterId, u64> {
    entries.iter().copied().collect()
}

#[tokio::test]
async fn setup_tokens_claim_once_and_expire() {
    let dir = TempDir::new();
    let store = open_store(&dir, RetentionPolicy::default()).await;
    let owner = admin();
    let admins = [owner.public_key()];
    let key = access(&space_id(1), 1);
    let token = RelaySetupToken::generate().unwrap();
    store.add_setup_token(&token, 1_000).await.unwrap();
    let claim = |space, token| ClaimRequest {
        space,
        token,
        epoch: 1,
        access_key: key.public_key(),
        admins: &admins,
        counter: 0,
    };
    store.claim(&claim(space_id(1), &token), 500).await.unwrap();
    // A used token claims nothing, not even another space.
    assert_eq!(
        code_of(store.claim(&claim(space_id(2), &token), 600).await),
        RelayRefusalCode::Token
    );
    let expiring = RelaySetupToken::generate().unwrap();
    store.add_setup_token(&expiring, 1_000).await.unwrap();
    assert_eq!(
        code_of(store.claim(&claim(space_id(2), &expiring), 1_000).await),
        RelayRefusalCode::Token
    );
    let unknown = RelaySetupToken::from_bytes([3; 32]);
    assert_eq!(
        code_of(store.claim(&claim(space_id(2), &unknown), 10).await),
        RelayRefusalCode::Token
    );
    // A claim of an existing space does not consume the token.
    let fresh = RelaySetupToken::generate().unwrap();
    store.add_setup_token(&fresh, 5_000).await.unwrap();
    assert_eq!(
        code_of(store.claim(&claim(space_id(1), &fresh), 10).await),
        RelayRefusalCode::SpaceExists
    );
    store.claim(&claim(space_id(2), &fresh), 10).await.unwrap();
}

#[tokio::test]
async fn admin_counters_refuse_replays_and_the_admin_set_changes() {
    let dir = TempDir::new();
    let space = space_id(1);
    let owner = admin();
    let second = admin();
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &owner,
        &access(&space, 1),
    )
    .await;
    store
        .apply_admin(
            &space,
            &owner.public_key(),
            1,
            AdminChange::AddKey(second.public_key()),
            2,
        )
        .await
        .unwrap();
    assert_eq!(
        code_of(
            store
                .apply_admin(&space, &owner.public_key(), 1, AdminChange::Wipe, 2)
                .await
        ),
        RelayRefusalCode::AdminCounter
    );
    store
        .apply_admin(
            &space,
            &second.public_key(),
            5,
            AdminChange::RemoveKey(owner.public_key()),
            2,
        )
        .await
        .unwrap();
    assert_eq!(
        store.admins(&space).await.unwrap(),
        vec![second.public_key()]
    );
    assert_eq!(
        code_of(
            store
                .apply_admin(&space, &owner.public_key(), 6, AdminChange::Wipe, 2)
                .await
        ),
        RelayRefusalCode::AdminSignature
    );
    assert_eq!(
        code_of(
            store
                .apply_admin(
                    &space,
                    &second.public_key(),
                    6,
                    AdminChange::RemoveKey(second.public_key()),
                    2,
                )
                .await
        ),
        RelayRefusalCode::AdminSet
    );
    store
        .apply_admin(&space, &second.public_key(), 7, AdminChange::RemoveSpace, 2)
        .await
        .unwrap();
    assert_eq!(
        code_of(store.state(&space).await),
        RelayRefusalCode::UnknownSpace
    );
}

#[tokio::test]
async fn pushes_are_contiguous_per_writer_and_report_duplicates_and_taken_slots() {
    let dir = TempDir::new();
    let space = space_id(1);
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &admin(),
        &access(&space, 1),
    )
    .await;
    let writer = writer_id(4);
    let page: Vec<_> = [3u64, 4, 6]
        .iter()
        .map(|seq| {
            let bytes = record(space, writer, *seq, 1, 1, 32);
            (header_of(&bytes), bytes)
        })
        .collect();
    let outcomes = store.push(&space, 1, &page, 10).await.unwrap();
    // The first record of a new writer may start anywhere; after it, sequences are contiguous.
    assert_eq!(
        outcomes,
        vec![
            RelayPushOutcome::Stored,
            RelayPushOutcome::Stored,
            RelayPushOutcome::Gap
        ]
    );
    let again = record(space, writer, 4, 1, 1, 32);
    let other = record(space, writer, 4, 1, 2, 32);
    let outcomes = store
        .push(
            &space,
            1,
            &[
                (header_of(&again), again.clone()),
                (header_of(&other), other),
            ],
            11,
        )
        .await
        .unwrap();
    assert_eq!(
        outcomes,
        vec![RelayPushOutcome::Duplicate, RelayPushOutcome::SlotTaken]
    );
    let state = store.state(&space).await.unwrap();
    assert_eq!((state.writers[0].floor, state.writers[0].next), (3, 5));
    // A device missing the sequences before the first stored one cannot apply anything here.
    let page = store.pull(&space, 1, &have(&[])).await.unwrap();
    assert!(page.records.is_empty());
    assert_eq!(page.behind_floor, vec![(writer, 3)]);
    let page = store.pull(&space, 1, &have(&[(writer, 2)])).await.unwrap();
    assert_eq!(page.records.len(), 2);
    assert!(page.behind_floor.is_empty());
    let page = store.pull(&space, 1, &have(&[(writer, 3)])).await.unwrap();
    assert_eq!(page.records, vec![again]);
}

#[tokio::test]
async fn epochs_register_only_as_the_next_epoch_from_the_current_one() {
    let dir = TempDir::new();
    let space = space_id(1);
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &admin(),
        &access(&space, 1),
    )
    .await;
    let second = access(&space, 2).public_key();
    assert_eq!(
        code_of(
            store
                .register_epoch(&space, 1, 3, &second, b"entry", 5)
                .await
        ),
        RelayRefusalCode::UnknownEpoch
    );
    store
        .register_epoch(&space, 1, 2, &second, b"entry two", 5)
        .await
        .unwrap();
    // A retry of the same registration from the old epoch succeeds without a change.
    store
        .register_epoch(&space, 1, 2, &second, b"entry two", 6)
        .await
        .unwrap();
    let third = access(&space, 3).public_key();
    assert_eq!(
        code_of(
            store
                .register_epoch(&space, 1, 3, &third, b"entry", 7)
                .await
        ),
        RelayRefusalCode::StaleEpoch
    );
    store
        .register_epoch(&space, 2, 3, &third, b"entry three", 8)
        .await
        .unwrap();
    let page = store.epochs(&space, 1).await.unwrap();
    assert_eq!(page.current_epoch, 3);
    assert_eq!(
        page.entries,
        vec![(2, b"entry two".to_vec()), (3, b"entry three".to_vec())]
    );
    assert_eq!(
        code_of(store.credential(&space, 4).await),
        RelayRefusalCode::UnknownEpoch
    );
}

fn ranges(state: &crate::store::SpaceState) -> Vec<(WriterId, u64, u64)> {
    state
        .writers
        .iter()
        .map(|range| (range.writer, range.floor, range.next))
        .collect()
}

async fn push_one(store: &RelayStore, bytes: &[u8], now_ms: i64) -> RelayPushOutcome {
    let header = header_of(bytes);
    store
        .push(
            &header.space,
            header.epoch,
            &[(header, bytes.to_vec())],
            now_ms,
        )
        .await
        .unwrap()[0]
}

#[tokio::test]
async fn an_epoch_repair_lowers_the_epoch_and_deletes_everything_above_it() {
    let dir = TempDir::new();
    let space = space_id(1);
    let owner = admin();
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &owner,
        &access(&space, 1),
    )
    .await;
    let honest = writer_id(1);
    let stray = writer_id(2);
    let second = access(&space, 2).public_key();
    let first_record = record(space, honest, 1, 1, 1, 32);
    assert_eq!(
        push_one(&store, &first_record, 1).await,
        RelayPushOutcome::Stored
    );
    store
        .register_epoch(&space, 1, 2, &second, b"entry two", 2)
        .await
        .unwrap();
    let second_record = record(space, honest, 2, 2, 1, 32);
    assert_eq!(
        push_one(&store, &second_record, 3).await,
        RelayPushOutcome::Stored
    );
    // A holder of epoch 2 climbs through keys of its own and fills slots above it.
    store
        .register_epoch(&space, 2, 3, &access(&space, 3).public_key(), b"x", 4)
        .await
        .unwrap();
    store
        .register_epoch(&space, 3, 4, &access(&space, 4).public_key(), b"y", 5)
        .await
        .unwrap();
    assert_eq!(
        push_one(&store, &record(space, honest, 3, 4, 9, 32), 6).await,
        RelayPushOutcome::Stored
    );
    assert_eq!(
        push_one(&store, &record(space, stray, 7, 4, 9, 32), 6).await,
        RelayPushOutcome::Stored
    );
    let stray_chunk = chunk(space, [5; 32], 4, 0, 1, 3);
    store
        .put_asset_chunk(&space, 4, &chunk_header(&stray_chunk), &stray_chunk, 7)
        .await
        .unwrap();

    let repair = AdminChange::RelayEpochRepair {
        epoch: 2,
        access_key: second,
    };
    assert_eq!(
        code_of(
            store
                .apply_admin(
                    &space,
                    &owner.public_key(),
                    1,
                    AdminChange::RelayEpochRepair {
                        epoch: 0,
                        access_key: second,
                    },
                    8,
                )
                .await
        ),
        RelayRefusalCode::BadRequest
    );
    store
        .apply_admin(&space, &owner.public_key(), 1, repair, 8)
        .await
        .unwrap();
    // A replay of the same signed repair is refused by the counter, whatever the epoch.
    assert!(matches!(
        store
            .apply_admin(&space, &owner.public_key(), 1, repair, 9)
            .await,
        Err(StoreError::StaleAdminCounter(1))
    ));

    let state = store.state(&space).await.unwrap();
    assert_eq!(state.current_epoch, 2);
    assert_eq!(state.admin_counter, 1);
    // The honest writer keeps its records up to epoch 2, and the stray writer is gone.
    assert_eq!(ranges(&state), vec![(honest, 1, 3)]);
    assert_eq!(
        state.stored_bytes,
        u64::try_from(first_record.len() + second_record.len()).unwrap()
    );
    assert_eq!(
        store.pull(&space, 2, &have(&[])).await.unwrap().records,
        vec![first_record, second_record]
    );
    assert!(
        store
            .complete_assets(&space, 2, &[[5; 32]])
            .await
            .unwrap()
            .is_empty()
    );
    // The repaired epoch kept its key, so it keeps its entry; nothing above it is known.
    let page = store.epochs(&space, 0).await.unwrap();
    assert_eq!(page.current_epoch, 2);
    assert_eq!(page.entries, vec![(2, b"entry two".to_vec())]);
    assert_eq!(
        code_of(store.credential(&space, 3).await),
        RelayRefusalCode::UnknownEpoch
    );
    assert_eq!(
        store.credential(&space, 2).await.unwrap().access_key,
        second
    );
    // The freed slot takes the honest writer's next record.
    assert_eq!(
        push_one(&store, &record(space, honest, 3, 2, 4, 32), 10).await,
        RelayPushOutcome::Stored
    );
}

#[tokio::test]
async fn an_epoch_repair_with_another_key_drops_what_the_wrong_key_wrote() {
    let dir = TempDir::new();
    let space = space_id(1);
    let owner = admin();
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &owner,
        &access(&space, 1),
    )
    .await;
    let writer = writer_id(1);
    let kept = record(space, writer, 1, 1, 1, 32);
    push_one(&store, &kept, 1).await;
    store
        .register_epoch(&space, 1, 2, &access(&space, 2).public_key(), b"fake", 2)
        .await
        .unwrap();
    push_one(&store, &record(space, writer, 2, 2, 9, 32), 3).await;
    let real = access(&space, 2).public_key();
    store
        .apply_admin(
            &space,
            &owner.public_key(),
            4,
            AdminChange::RelayEpochRepair {
                epoch: 2,
                access_key: real,
            },
            4,
        )
        .await
        .unwrap();
    let state = store.state(&space).await.unwrap();
    assert_eq!(ranges(&state), vec![(writer, 1, 2)]);
    assert_eq!(state.stored_bytes, u64::try_from(kept.len()).unwrap());
    assert_eq!(store.credential(&space, 2).await.unwrap().access_key, real);
    // The wrong entry is gone; the holder of the real key fills it in by registering again.
    assert!(store.epochs(&space, 0).await.unwrap().entries.is_empty());
    store
        .register_epoch(&space, 1, 2, &real, b"real entry", 5)
        .await
        .unwrap();
    assert_eq!(
        store.epochs(&space, 0).await.unwrap().entries,
        vec![(2, b"real entry".to_vec())]
    );
    store
        .register_epoch(&space, 1, 2, &real, b"other entry", 6)
        .await
        .unwrap();
    assert_eq!(
        store.epochs(&space, 0).await.unwrap().entries,
        vec![(2, b"real entry".to_vec())]
    );
}

#[tokio::test]
async fn an_old_epoch_credential_cannot_push_or_read_records_sealed_after_it() {
    let dir = TempDir::new();
    let space = space_id(1);
    let store = claimed_store(
        &dir,
        RetentionPolicy::default(),
        space,
        &admin(),
        &access(&space, 1),
    )
    .await;
    let writer = writer_id(4);
    let before = record(space, writer, 1, 1, 1, 32);
    store
        .push(&space, 1, &[(header_of(&before), before.clone())], 1)
        .await
        .unwrap();
    store
        .register_epoch(&space, 1, 2, &access(&space, 2).public_key(), b"entry", 2)
        .await
        .unwrap();
    let late = record(space, writer, 2, 1, 1, 32);
    assert_eq!(
        code_of(store.push(&space, 1, &[(header_of(&late), late)], 3).await),
        RelayRefusalCode::StaleEpoch
    );
    let after = record(space, writer, 2, 2, 1, 32);
    store
        .push(&space, 2, &[(header_of(&after), after.clone())], 3)
        .await
        .unwrap();
    let old = store.pull(&space, 1, &have(&[])).await.unwrap();
    assert_eq!(old.records, vec![before.clone()]);
    assert!(!old.more);
    let current = store.pull(&space, 2, &have(&[])).await.unwrap();
    assert_eq!(current.records, vec![before, after]);
}

#[tokio::test]
async fn retention_trims_oldest_records_raises_floors_and_reports_behind_floor() {
    let dir = TempDir::new();
    let space = space_id(1);
    let size = 200;
    let one = record(space, writer_id(1), 1, 1, 0, size).len() as u64;
    let retention = RetentionPolicy {
        quota_bytes: one * 3,
        max_age: Duration::from_secs(10 * 24 * 60 * 60),
    };
    let store = claimed_store(&dir, retention, space, &admin(), &access(&space, 1)).await;
    let writer = writer_id(1);
    for seq in 1..=5u64 {
        let bytes = record(space, writer, seq, 1, 0, size);
        store
            .push(&space, 1, &[(header_of(&bytes), bytes)], seq as i64)
            .await
            .unwrap();
    }
    let state = store.state(&space).await.unwrap();
    assert_eq!((state.writers[0].floor, state.writers[0].next), (3, 6));
    assert_eq!(state.stored_bytes, one * 3);
    let page = store.pull(&space, 1, &have(&[(writer, 1)])).await.unwrap();
    assert!(page.records.is_empty());
    assert_eq!(page.behind_floor, vec![(writer, 3)]);
    let page = store.pull(&space, 1, &have(&[(writer, 2)])).await.unwrap();
    assert_eq!(page.records.len(), 3);
    let trimmed = record(space, writer, 2, 1, 0, size);
    assert_eq!(
        store
            .push(&space, 1, &[(header_of(&trimmed), trimmed)], 6)
            .await
            .unwrap(),
        vec![RelayPushOutcome::Trimmed]
    );
    // Age trims everything stored before the cutoff, and the floor follows.
    store.sweep(10 * DAY_MS + 10).await.unwrap();
    let state = store.state(&space).await.unwrap();
    assert_eq!((state.writers[0].floor, state.writers[0].next), (6, 6));
    assert_eq!(state.stored_bytes, 0);
    let next = record(space, writer, 6, 1, 0, size);
    assert_eq!(
        store
            .push(&space, 1, &[(header_of(&next), next)], 10 * DAY_MS + 20)
            .await
            .unwrap(),
        vec![RelayPushOutcome::Stored]
    );
}

#[tokio::test]
async fn assets_store_read_and_expire_in_order_of_last_read() {
    let dir = TempDir::new();
    let space = space_id(1);
    let one = chunk(space, [1; 32], 1, 0, 1, 0).len() as u64;
    let retention = RetentionPolicy {
        quota_bytes: one * 2,
        max_age: Duration::from_secs(24 * 60 * 60),
    };
    let store = claimed_store(&dir, retention, space, &admin(), &access(&space, 1)).await;
    let first = chunk(space, [1; 32], 1, 0, 2, 1);
    assert_eq!(
        store
            .put_asset_chunk(&space, 1, &chunk_header(&first), &first, 10)
            .await
            .unwrap(),
        RelayAssetOutcome::Stored
    );
    assert!(
        store
            .complete_assets(&space, 1, &[[1; 32]])
            .await
            .unwrap()
            .is_empty()
    );
    let second = chunk(space, [1; 32], 1, 1, 2, 1);
    store
        .put_asset_chunk(&space, 1, &chunk_header(&second), &second, 11)
        .await
        .unwrap();
    assert_eq!(
        store
            .complete_assets(&space, 1, &[[1; 32], [2; 32]])
            .await
            .unwrap(),
        vec![[1; 32]]
    );
    assert_eq!(
        store
            .put_asset_chunk(&space, 1, &chunk_header(&second), &second, 12)
            .await
            .unwrap(),
        RelayAssetOutcome::Duplicate
    );
    let other = chunk(space, [1; 32], 1, 1, 2, 2);
    assert_eq!(
        store
            .put_asset_chunk(&space, 1, &chunk_header(&other), &other, 12)
            .await
            .unwrap(),
        RelayAssetOutcome::SlotTaken
    );
    let mismatch = chunk(space, [1; 32], 1, 2, 3, 2);
    assert_eq!(
        code_of(
            store
                .put_asset_chunk(&space, 1, &chunk_header(&mismatch), &mismatch, 12)
                .await
        ),
        RelayRefusalCode::AssetMismatch
    );
    assert_eq!(
        store.asset_chunk(&space, 1, &[1; 32], 1, 20).await.unwrap(),
        second
    );
    assert_eq!(
        code_of(store.asset_chunk(&space, 1, &[1; 32], 5, 20).await),
        RelayRefusalCode::NotFound
    );
    // A second asset over the quota evicts the asset read least recently, whole.
    let fresh = chunk(space, [2; 32], 1, 0, 1, 3);
    store
        .put_asset_chunk(&space, 1, &chunk_header(&fresh), &fresh, 30)
        .await
        .unwrap();
    assert_eq!(
        code_of(store.asset_chunk(&space, 1, &[1; 32], 0, 31).await),
        RelayRefusalCode::NotFound
    );
    assert_eq!(
        store.asset_chunk(&space, 1, &[2; 32], 0, 31).await.unwrap(),
        fresh
    );
    // An asset unread for longer than the maximum age expires.
    store.sweep(31 + DAY_MS + 1).await.unwrap();
    assert_eq!(
        code_of(
            store
                .asset_chunk(&space, 1, &[2; 32], 0, 31 + DAY_MS + 2)
                .await
        ),
        RelayRefusalCode::NotFound
    );
    assert_eq!(store.state(&space).await.unwrap().stored_bytes, 0);
}

mod tls;
