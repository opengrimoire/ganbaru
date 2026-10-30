//! Full relay passes against an in-process relay server: devices converge only through the
//! relay, a removed device opens nothing sealed after its removal, an admin repairs a wrong epoch
//! registration, an admin that lags behind a rotation or a demotion leaves the relay alone, a
//! stalled flush registers nothing, asset chunks round trip and a tampered chunk is refused, and
//! an access proof bound to another connection is refused.

use super::handover::{commit_sequenced, sequence_here};
use super::{
    HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE, block_on, device_key, done, hub, hub_keys,
    png,
};
use crate::authority::{self, Enrollment};
use crate::client::{MAX_EXCHANGE_BYTES, PeerIds};
use crate::keys::SpaceKeys;
use crate::outbox::Outbox;
use crate::relay::{
    Connection, RelayError, RelayLimits, RelayPass, RelayProblem, RelaySession, RelayTarget,
    UNMETERED_MAX_ASSET_BYTES, claim_space,
};
use crate::tests::{NOW_MS, TestDirectory, VAULT_ID, acquire, add_note};
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::protocol::relay::{
    RELAY_AUTH_DOMAIN, RELAY_EPOCH_DOMAIN, RELAY_SETUP_TOKEN_LIFETIME, RelayRefusalCode,
    RelayRequest, RelayResponse, RelaySetupToken, encode_relay_bytes,
};
use ganbaru_sync::{AuthorizationView, RelayKeys};
use ganbaru_sync_contracts::bounds::RELAY_ASSET_CHUNK_BYTES;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKind, DeviceRole, EncryptionKeyPair, Entry, RelayAccessKeyPair, SetRole,
    SpaceKey,
};
use ganbaru_sync_relay::identity::RelayIdentity;
use ganbaru_sync_relay::server::{ConnectionLimits, RelayServer, unix_time_ms};
use ganbaru_sync_relay::store::{RelayStore, RetentionPolicy};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

/// A relay server running on a loopback port for one test.
struct TestRelay {
    _directory: TestDirectory,
    database: PathBuf,
    store: RelayStore,
    target: RelayTarget,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start_relay(label: &str) -> TestRelay {
    let directory = TestDirectory::new(label);
    let identity = RelayIdentity::load_or_create(directory.path()).expect("relay identity");
    let database = directory.path().join("relay.sqlite");
    let store = RelayStore::open(&database, RetentionPolicy::default())
        .await
        .expect("relay store");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
    let server = RelayServer::new(
        listener,
        identity.server_config().expect("relay TLS"),
        store.clone(),
        ConnectionLimits::default(),
    );
    let address = server.local_addr().expect("relay address");
    let task = tokio::spawn(server.run());
    TestRelay {
        _directory: directory,
        database,
        store,
        target: RelayTarget {
            endpoints: vec![address.to_string()],
            certificate_fingerprint: identity.fingerprint_bytes(),
        },
        task,
    }
}

/// A replica with its installation's space keys attached.
struct Keyed {
    replica: Replica,
    keys: Arc<SpaceKeys>,
}

fn new_space_keys(device_id: &str) -> Arc<SpaceKeys> {
    Arc::new(SpaceKeys::new(
        device_key(device_id),
        EncryptionKeyPair::generate().expect("encryption key"),
    ))
}

fn ids(device_id: &str) -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: device_id.to_string(),
    }
}

/// The hub opens key epoch 1, publishes the device key of every member, and links each member
/// over the LAN once so it holds its envelope; everything after moves through the relay.
async fn keyed_space(label: &str, members: &[&str]) -> (Keyed, Vec<Keyed>) {
    let mut hub_replica = Replica::new(&format!("relay-hub-{label}"), HUB_DEVICE).await;
    let hub_space_keys = new_space_keys(HUB_DEVICE);
    hub_replica.writer.attach_keys(hub_space_keys.clone());
    {
        let mut conn = acquire(&hub_replica.pool).await;
        let mut outbox =
            Outbox::load(&hub_replica.directory.files().outbox_path()).expect("outbox");
        authority::publish_device_key(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &mut hub_replica.writer.local(),
            hub_space_keys.as_ref(),
            &mut outbox,
            NOW_MS,
        )
        .await
        .expect("publish the hub's device key");
        authority::start_key_epochs(
            &hub_replica.engine,
            &mut conn,
            &hub_replica.ctx,
            &hub_keys().admin,
            &mut hub_replica.writer.local(),
            &hub_keys().recovery,
            NOW_MS,
        )
        .await
        .expect("start the key epochs")
        .expect("the founding kit starts them")
        .expect("a key epoch was sealed");
    }
    commit_sequenced(&mut hub_replica).await;
    let charter = hub_replica.charter().await;
    let mut keyed = Vec::new();
    for device_id in members {
        let mut replica = Replica::new(&format!("relay-{device_id}-{label}"), device_id).await;
        let keys = new_space_keys(device_id);
        let published = authority::device_key_proposal(
            hub_replica.ctx.space,
            charter,
            device_id,
            keys.as_ref(),
            NOW_MS,
        )
        .expect("sign the member's device key");
        sequence_here(&mut hub_replica, &published).await;
        replica.writer.attach_keys(keys.clone());
        keyed.push(Keyed { replica, keys });
    }
    let (transport, _changes) = hub(&hub_replica);
    for member in &mut keyed {
        done(
            member
                .replica
                .exchange(&transport, &ids(&member.replica.device_id))
                .await
                .expect("link over the LAN"),
        );
        member.replica.apply().await;
    }
    drop(transport);
    (
        Keyed {
            replica: hub_replica,
            keys: hub_space_keys,
        },
        keyed,
    )
}

impl Keyed {
    async fn relay_keys(&self) -> RelayKeys {
        let mut conn = acquire(&self.replica.pool).await;
        let stored = self
            .replica
            .engine
            .space_keys(&mut conn, &self.replica.ctx)
            .await
            .expect("space keys");
        RelayKeys::open(self.replica.ctx.space, &stored, self.keys.as_ref())
    }

    async fn access(&self, epoch: u32) -> RelayAccessKeyPair {
        self.relay_keys()
            .await
            .key(epoch)
            .expect("held epoch key")
            .relay_access_key(&self.replica.ctx.space)
            .expect("access key")
    }

    async fn view(&self) -> AuthorizationView {
        let mut conn = acquire(&self.replica.pool).await;
        self.replica
            .engine
            .authorization(&mut conn, &self.replica.ctx)
            .await
            .expect("authorization")
            .expect("charter")
    }

    /// Runs one relay pass, as an admin when this is the hub.
    async fn pass(&self, relay: &TestRelay) -> RelayPass {
        self.try_pass(relay, UNMETERED_MAX_ASSET_BYTES)
            .await
            .expect("relay pass")
    }

    async fn try_pass(
        &self,
        relay: &TestRelay,
        max_asset_bytes: u32,
    ) -> Result<RelayPass, RelayError> {
        let admin = self.replica.device_id == HUB_DEVICE;
        self.pass_as(relay, admin, MAX_EXCHANGE_BYTES, max_asset_bytes)
            .await
    }

    /// Runs one relay pass as a second admin device of the space, holding the hub's admin key,
    /// that stops pulling once it moved `max_bytes`.
    async fn admin_pass(&self, relay: &TestRelay, max_bytes: usize) -> RelayPass {
        self.pass_as(relay, true, max_bytes, UNMETERED_MAX_ASSET_BYTES)
            .await
            .expect("admin relay pass")
    }

    async fn pass_as(
        &self,
        relay: &TestRelay,
        admin: bool,
        max_bytes: usize,
        max_asset_bytes: u32,
    ) -> Result<RelayPass, RelayError> {
        let admin_key = hub_keys().admin;
        let own_writers = self.replica.writer.own_writers();
        let quarantined = BTreeSet::new();
        RelaySession {
            engine: &self.replica.engine,
            pool: &self.replica.pool,
            ctx: &self.replica.ctx,
            holder: self.keys.as_ref(),
            admin: admin.then_some(&admin_key),
            own_writers: &own_writers,
            quarantined: &quarantined,
            assets: Some(&self.replica.assets),
            limits: RelayLimits {
                max_bytes,
                max_asset_bytes,
                loopback: LoopbackPolicy::Allow,
            },
        }
        .pass(&relay.target)
        .await
    }

    /// Claims the space on `relay` with the hub's admin key.
    async fn claim(&self, relay: &TestRelay) {
        let token = RelaySetupToken::generate().expect("setup token");
        let lifetime = i64::try_from(RELAY_SETUP_TOKEN_LIFETIME.as_millis()).expect("lifetime");
        relay
            .store
            .add_setup_token(&token, unix_time_ms() + lifetime)
            .await
            .expect("issue setup token");
        let keys = self.relay_keys().await;
        let first: &SpaceKey = keys.key(1).expect("epoch 1 key");
        let admin = hub_keys().admin;
        claim_space(
            &relay.target,
            self.replica.ctx.space,
            LoopbackPolicy::Allow,
            &token,
            first,
            &admin,
            &[admin.public_key()],
        )
        .await
        .expect("claim the space");
    }

    async fn close(self) {
        self.replica.close().await;
    }
}

/// Removes `device_id` on the hub and seals the key epoch the removal waits for.
async fn remove_and_rotate(hub: &mut Keyed, device_id: &str) {
    hub.replica.remove(device_id).await.expect("remove");
    {
        let mut conn = acquire(&hub.replica.pool).await;
        authority::complete_rotation(
            &hub.replica.engine,
            &mut conn,
            &hub.replica.ctx,
            &mut hub.replica.writer.local(),
            NOW_MS,
        )
        .await
        .expect("seal the key epoch")
        .expect("the anchor seals it");
    }
    commit_sequenced(&mut hub.replica).await;
}

#[test]
fn two_devices_converge_only_through_the_relay() {
    block_on(async {
        let relay = start_relay("relay-converge").await;
        let (mut hub_device, members) = keyed_space("converge", &[PHONE_DEVICE]).await;
        let mut phone = members.into_iter().next().expect("phone");
        hub_device.claim(&relay).await;

        hub_device.replica.seal_note("note-from-hub").await;
        let pushed = hub_device.pass(&relay).await;
        assert_eq!(pushed.problem, None);
        assert_eq!(pushed.epoch, 1);
        assert!(pushed.pushed > 0);
        assert!(pushed.stored_bytes > 0);
        assert_eq!(pushed.undelivered, Some(0), "{pushed:?}");

        let pulled = phone.pass(&relay).await;
        assert_eq!(pulled.problem, None);
        assert!(pulled.pulled > 0);
        phone.replica.apply().await;
        assert!(phone.replica.has_note("note-from-hub").await);

        phone.replica.seal_note("note-from-phone").await;
        let pushed = phone.pass(&relay).await;
        assert_eq!(pushed.problem, None);
        assert!(pushed.pushed > 0);
        let pulled = hub_device.pass(&relay).await;
        assert_eq!(pulled.problem, None);
        hub_device.replica.apply().await;
        assert!(hub_device.replica.has_note("note-from-phone").await);
        assert_eq!(
            hub_device.replica.stored().await,
            phone.replica.stored().await
        );

        // A settled pass moves nothing.
        let settled = hub_device.pass(&relay).await;
        assert_eq!((settled.pushed, settled.pulled), (0, 0));
        hub_device.close().await;
        phone.close().await;
    });
}

#[test]
fn a_removed_device_pulls_nothing_sealed_after_its_removal() {
    block_on(async {
        let relay = start_relay("relay-removal").await;
        let (mut hub_device, members) =
            keyed_space("removal", &[PHONE_DEVICE, TABLET_DEVICE]).await;
        let mut members = members.into_iter();
        let mut phone = members.next().expect("phone");
        let mut tablet = members.next().expect("tablet");
        hub_device.claim(&relay).await;
        for keyed in [&hub_device, &phone, &tablet] {
            assert_eq!(keyed.pass(&relay).await.problem, None);
        }

        remove_and_rotate(&mut hub_device, TABLET_DEVICE).await;
        assert_eq!(hub_device.view().await.key_epoch, 2);
        hub_device.replica.seal_note("note-after-removal").await;
        let rotated = hub_device.pass(&relay).await;
        assert_eq!(rotated.problem, None);
        assert_eq!(rotated.registered, vec![2]);
        assert_eq!(rotated.epoch, 2);

        // The phone opens epoch 2 from the records the hub delivered under epoch 1.
        let caught_up = phone.pass(&relay).await;
        assert_eq!(caught_up.problem, None);
        assert_eq!(caught_up.epoch, 2);
        phone.replica.apply().await;
        assert!(phone.replica.has_note("note-after-removal").await);
        phone.replica.seal_note("note-from-phone-at-two").await;
        assert_eq!(phone.pass(&relay).await.problem, None);

        // The tablet learns its removal and opens nothing sealed after it.
        let removed = tablet.pass(&relay).await;
        assert!(removed.problem.is_some());
        assert_eq!(removed.epoch, 1);
        tablet.replica.apply().await;
        assert!(!tablet.replica.has_note("note-after-removal").await);
        assert!(!tablet.replica.has_note("note-from-phone-at-two").await);
        let view = tablet.view().await;
        assert!(
            view.devices.iter().any(
                |device| device.device_id == TABLET_DEVICE && device.removed_revision.is_some()
            )
        );
        let writer = phone.replica.writer.id();
        assert!(
            tablet.replica.stored().await.get(&writer) < phone.replica.stored().await.get(&writer)
        );
        hub_device.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn a_pass_stopped_by_a_newer_epoch_reports_delivery_as_unknown() {
    block_on(async {
        let relay = start_relay("relay-epoch-ahead").await;
        let (mut hub_device, members) =
            keyed_space("epoch-ahead", &[PHONE_DEVICE, TABLET_DEVICE]).await;
        let mut members = members.into_iter();
        let phone = members.next().expect("phone");
        let mut tablet = members.next().expect("tablet");
        hub_device.claim(&relay).await;
        for keyed in [&hub_device, &phone, &tablet] {
            assert_eq!(keyed.pass(&relay).await.problem, None);
        }

        remove_and_rotate(&mut hub_device, TABLET_DEVICE).await;
        assert_eq!(hub_device.pass(&relay).await.epoch, 2);

        tablet.replica.seal_note("note-the-relay-never-gets").await;
        let stopped = tablet.pass(&relay).await;
        assert_eq!(stopped.problem, Some(RelayProblem::EpochAhead));
        assert_eq!(stopped.undelivered, None, "{stopped:?}");
        hub_device.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn an_admin_pass_repairs_a_hijacked_epoch_registration_and_delivery_resumes() {
    block_on(hijacked_epoch_is_repaired("hijack", Hijack::Garbage));
}

#[test]
fn an_admin_pass_repairs_an_epoch_registered_with_a_replayed_key_epoch_record() {
    block_on(hijacked_epoch_is_repaired(
        "hijack-replay",
        Hijack::ReplayedEpochOne,
    ));
}

/// The record a hijacker registers its epoch 2 with.
enum Hijack {
    Garbage,
    /// The genuine, anchor-signed key epoch record of epoch 1, which any holder of the epoch 1
    /// key has.
    ReplayedEpochOne,
}

async fn hijacked_epoch_is_repaired(label: &str, hijack: Hijack) {
    let relay = start_relay(&format!("relay-{label}")).await;
    let (mut hub_device, members) = keyed_space(label, &[PHONE_DEVICE]).await;
    let mut phone = members.into_iter().next().expect("phone");
    hub_device.claim(&relay).await;
    assert_eq!(hub_device.pass(&relay).await.problem, None);
    assert_eq!(phone.pass(&relay).await.problem, None);
    let entry = match hijack {
        Hijack::Garbage => vec![7; 64],
        Hijack::ReplayedEpochOne => {
            let mut conn = acquire(&phone.replica.pool).await;
            phone
                .replica
                .engine
                .key_epoch_entry(&mut conn, &phone.replica.ctx, 1)
                .await
                .expect("read the key epoch record")
                .expect("the record of epoch 1")
        }
    };

    // A holder of the epoch 1 key registers an epoch 2 the chain never sealed.
    let space = hub_device.replica.ctx.space;
    let access = phone.access(1).await;
    let mut hijacker = Connection::open(&relay.target, space, LoopbackPolicy::Allow)
        .await
        .expect("connect");
    hijacker.hello().await.expect("hello");
    let signature = access.sign(RELAY_AUTH_DOMAIN, &hijacker.binding().auth_payload(1));
    let response = hijacker
        .request(&RelayRequest::Auth {
            epoch: 1,
            signature: encode_relay_bytes(&signature),
        })
        .await
        .expect("auth");
    assert!(matches!(response, RelayResponse::Authenticated { .. }));
    let forged = SpaceKey::generate(2)
        .expect("forged key")
        .relay_access_key(&space)
        .expect("forged access");
    let signature = forged.sign(RELAY_EPOCH_DOMAIN, &hijacker.binding().epoch_payload(2));
    let response = hijacker
        .request(&RelayRequest::RegisterEpoch {
            epoch: 2,
            access_key: encode_relay_bytes(forged.public_key().as_bytes()),
            entry: encode_relay_bytes(&entry),
            signature: encode_relay_bytes(&signature),
        })
        .await
        .expect("register");
    assert_eq!(response, RelayResponse::EpochRegistered { epoch: 2 });
    drop(hijacker);

    // A member cannot push into the forged epoch and waits for an admin.
    phone.replica.seal_note("note-held-back").await;
    let stalled = phone.pass(&relay).await;
    assert!(stalled.problem.is_some());
    assert_eq!(stalled.pushed, 0);

    let repaired = hub_device.pass(&relay).await;
    assert_eq!(repaired.repaired, Some(1));
    assert_eq!(repaired.epoch, 1);
    hub_device.replica.seal_note("note-after-repair").await;
    assert_eq!(hub_device.pass(&relay).await.problem, None);

    let resumed = phone.pass(&relay).await;
    assert_eq!(resumed.problem, None);
    assert!(resumed.pushed > 0);
    phone.replica.apply().await;
    assert!(phone.replica.has_note("note-after-repair").await);
    assert_eq!(hub_device.pass(&relay).await.problem, None);
    hub_device.replica.apply().await;
    assert!(hub_device.replica.has_note("note-held-back").await);
    hub_device.close().await;
    phone.close().await;
}

/// Characters of each large note body, below the Quick notes body limit.
const LARGE_NOTE_CHARS: usize = 60_000;
/// Large notes that take more than one relay pull page.
const LARGE_NOTES: u64 = 12;
/// An admin device that only ever appears in the authorization chain.
const SECOND_ADMIN_DEVICE: &str = "device-second-admin";

/// Opens the relay's database beside its running server.
async fn relay_database(relay: &TestRelay) -> sqlx::SqlitePool {
    sqlx::SqlitePool::connect(&format!("sqlite://{}", relay.database.display()))
        .await
        .expect("open relay database")
}

async fn relay_epoch(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT current_epoch FROM relay_spaces")
        .fetch_one(pool)
        .await
        .expect("relay epoch")
}

async fn relay_holds_admin(pool: &sqlx::SqlitePool, admin: &AdminKeyPair) -> bool {
    let keys: Vec<u8> = sqlx::query_scalar("SELECT admin_keys FROM relay_spaces")
        .fetch_one(pool)
        .await
        .expect("relay admin keys");
    keys.chunks(32)
        .any(|key| key == admin.public_key().as_bytes().as_slice())
}

/// Text of `chars` letters that does not repeat, so sealing cannot shrink it.
fn noisy_text(chars: usize, seed: u64) -> String {
    let mut state = seed;
    (0..chars)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let letter = u8::try_from((state >> 33) % 26).expect("a letter offset");
            char::from(b'a' + letter)
        })
        .collect()
}

async fn seal_large_note(replica: &mut Replica, id: &str, seed: u64) {
    add_note(&mut *acquire(&replica.pool).await, id).await;
    sqlx::query("INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES (?, 0, ?)")
        .bind(id)
        .bind(noisy_text(LARGE_NOTE_CHARS, seed))
        .execute(&replica.pool)
        .await
        .expect("insert note body");
    replica.seal().await;
}

/// Why an admin device that lags behind a rotation cannot know what the relay still holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lag {
    /// Its pull stops at its byte bound before the rotation.
    ByteBound,
    /// It refuses a record the relay altered.
    RefusedRecord,
    /// The relay trimmed operations it lacks.
    BehindFloor,
}

#[test]
fn a_lagging_admin_stopped_by_its_byte_bound_keeps_the_rotation_and_catches_up() {
    block_on(lagging_admin_keeps_the_rotation(
        "lag-bytes",
        Lag::ByteBound,
    ));
}

#[test]
fn a_lagging_admin_that_refuses_a_record_keeps_the_rotation() {
    block_on(lagging_admin_keeps_the_rotation(
        "lag-refused",
        Lag::RefusedRecord,
    ));
}

#[test]
fn a_lagging_admin_behind_the_relay_floor_keeps_the_rotation() {
    block_on(lagging_admin_keeps_the_rotation(
        "lag-floor",
        Lag::BehindFloor,
    ));
}

async fn lagging_admin_keeps_the_rotation(label: &str, lag: Lag) {
    let relay = start_relay(&format!("relay-{label}")).await;
    let (mut hub_device, members) = keyed_space(label, &[PHONE_DEVICE, TABLET_DEVICE]).await;
    let mut members = members.into_iter();
    let phone = members.next().expect("phone");
    let tablet = members.next().expect("tablet");
    hub_device.claim(&relay).await;
    for keyed in [&hub_device, &phone, &tablet] {
        assert_eq!(keyed.pass(&relay).await.problem, None);
    }
    if lag == Lag::ByteBound {
        for index in 0..LARGE_NOTES {
            seal_large_note(
                &mut hub_device.replica,
                &format!("large-note-{index}"),
                index,
            )
            .await;
        }
    }
    remove_and_rotate(&mut hub_device, TABLET_DEVICE).await;
    let rotated = hub_device.pass(&relay).await;
    assert_eq!(rotated.problem, None);
    assert_eq!(rotated.registered, vec![2]);

    let hub_writer = hub_device.replica.writer.id();
    let pool = relay_database(&relay).await;
    match lag {
        Lag::ByteBound => {}
        Lag::RefusedRecord => {
            let seq = i64::try_from(phone.replica.stored().await.get(&hub_writer) + 1)
                .expect("a small sequence");
            let record: Vec<u8> = sqlx::query_scalar(
                "SELECT record FROM relay_records WHERE writer_id = ? AND seq = ?",
            )
            .bind(hub_writer.as_bytes().as_slice())
            .bind(seq)
            .fetch_one(&pool)
            .await
            .expect("the hub's next record");
            let mut tampered = record;
            let last = tampered.len() - 1;
            tampered[last] ^= 0x01;
            sqlx::query("UPDATE relay_records SET record = ? WHERE writer_id = ? AND seq = ?")
                .bind(tampered)
                .bind(hub_writer.as_bytes().as_slice())
                .bind(seq)
                .execute(&pool)
                .await
                .expect("tamper");
        }
        Lag::BehindFloor => {
            sqlx::query("UPDATE relay_floors SET floor = next_seq WHERE writer_id = ?")
                .bind(hub_writer.as_bytes().as_slice())
                .execute(&pool)
                .await
                .expect("trim");
        }
    }

    // The phone holds an admin key but has not reached the rotation: it repairs nothing.
    let max_bytes = if lag == Lag::ByteBound {
        1
    } else {
        MAX_EXCHANGE_BYTES
    };
    let lagging = phone.admin_pass(&relay, max_bytes).await;
    assert_eq!(lagging.repaired, None, "{lagging:?}");
    assert_eq!(lagging.epoch, 1, "{lagging:?}");
    assert!(lagging.problem.is_some(), "{lagging:?}");
    if lag == Lag::ByteBound {
        assert!(lagging.pulled > 0 && lagging.more, "{lagging:?}");
    }
    assert_eq!(relay_epoch(&pool).await, 2);
    let kept = hub_device.pass(&relay).await;
    assert_eq!((kept.epoch, kept.repaired), (2, None), "{kept:?}");
    assert!(kept.registered.is_empty(), "{kept:?}");

    if lag == Lag::ByteBound {
        // Once it drains, the phone opens epoch 2, and neither side repairs or registers again.
        let mut passes = 0;
        loop {
            let caught_up = phone.admin_pass(&relay, MAX_EXCHANGE_BYTES).await;
            assert_eq!(caught_up.repaired, None, "{caught_up:?}");
            if caught_up.epoch == 2 {
                break;
            }
            passes += 1;
            assert!(passes < 4, "{caught_up:?}");
        }
        for _ in 0..2 {
            for settled in [
                hub_device.pass(&relay).await,
                phone.admin_pass(&relay, MAX_EXCHANGE_BYTES).await,
            ] {
                assert_eq!(settled.problem, None, "{settled:?}");
                assert_eq!((settled.epoch, settled.repaired), (2, None), "{settled:?}");
                assert!(settled.registered.is_empty(), "{settled:?}");
            }
        }
        assert_eq!(relay_epoch(&pool).await, 2);
    }
    pool.close().await;
    hub_device.close().await;
    phone.close().await;
    tablet.close().await;
}

#[test]
fn a_device_whose_flush_stalls_does_not_register_the_next_epoch() {
    block_on(async {
        let relay = start_relay("relay-stall").await;
        let (mut hub_device, members) = keyed_space("stall", &[PHONE_DEVICE, TABLET_DEVICE]).await;
        let mut members = members.into_iter();
        let phone = members.next().expect("phone");
        let tablet = members.next().expect("tablet");
        hub_device.claim(&relay).await;
        for keyed in [&hub_device, &phone, &tablet] {
            assert_eq!(keyed.pass(&relay).await.problem, None);
        }

        // The hub compacts an operation the relay never received, so its flush meets a gap
        // before the record that opens epoch 2.
        hub_device.replica.seal_note("note-before-removal").await;
        remove_and_rotate(&mut hub_device, TABLET_DEVICE).await;
        hub_device.replica.seal_note("note-after-removal").await;
        assert!(hub_device.replica.compact().await > 0);
        let stalled = hub_device.pass(&relay).await;
        assert!(stalled.registered.is_empty(), "{stalled:?}");
        assert_eq!(stalled.epoch, 1, "{stalled:?}");
        assert_eq!(
            stalled.problem,
            Some(RelayProblem::RelayBehind),
            "{stalled:?}"
        );
        assert_eq!(stalled.undelivered, None, "{stalled:?}");
        let pool = relay_database(&relay).await;
        assert_eq!(relay_epoch(&pool).await, 1);
        pool.close().await;
        assert_eq!(phone.pass(&relay).await.epoch, 1);
        hub_device.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn a_lagging_admin_does_not_restore_a_demoted_admin_key() {
    block_on(async {
        let relay = start_relay("relay-demotion").await;
        let (mut hub_device, members) = keyed_space("demotion", &[PHONE_DEVICE]).await;
        let phone = members.into_iter().next().expect("phone");
        hub_device.claim(&relay).await;
        assert_eq!(hub_device.pass(&relay).await.problem, None);
        assert_eq!(phone.pass(&relay).await.problem, None);

        let second_admin = AdminKeyPair::from_seed(&[61; 32]).expect("admin seed");
        let space = hub_device.replica.ctx.space;
        let charter = hub_device.replica.charter().await;
        for entry in [
            authority::enroll_entry(Enrollment {
                device_id: SECOND_ADMIN_DEVICE.to_string(),
                device_key: device_key(SECOND_ADMIN_DEVICE).public_key(),
                tls_fingerprint: [61; 32],
                label: SECOND_ADMIN_DEVICE.to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            authority::promote_entry(SECOND_ADMIN_DEVICE, second_admin.public_key()),
        ] {
            let proposal = authority::propose(space, charter, &hub_keys().admin, entry, NOW_MS)
                .expect("sign proposal");
            sequence_here(&mut hub_device.replica, &proposal).await;
        }
        assert_eq!(hub_device.pass(&relay).await.problem, None);
        let pool = relay_database(&relay).await;
        assert!(relay_holds_admin(&pool, &second_admin).await);
        // The phone learns the promotion, then lags behind the demotion.
        assert_eq!(phone.pass(&relay).await.problem, None);
        let demotion = authority::propose(
            space,
            charter,
            &hub_keys().admin,
            Entry::SetRole(SetRole::Device {
                device_id: SECOND_ADMIN_DEVICE.to_string(),
                role: DeviceRole::Member,
            }),
            NOW_MS,
        )
        .expect("sign demotion");
        sequence_here(&mut hub_device.replica, &demotion).await;
        assert_eq!(hub_device.pass(&relay).await.problem, None);
        assert!(!relay_holds_admin(&pool, &second_admin).await);

        // A pull that meets a trimmed range leaves the admin set alone.
        let floors: Vec<(Vec<u8>, i64)> =
            sqlx::query_as("SELECT writer_id, floor FROM relay_floors")
                .fetch_all(&pool)
                .await
                .expect("relay floors");
        sqlx::query("UPDATE relay_floors SET floor = next_seq")
            .execute(&pool)
            .await
            .expect("trim");
        let lagging = phone.admin_pass(&relay, MAX_EXCHANGE_BYTES).await;
        assert!(lagging.problem.is_some(), "{lagging:?}");
        assert!(!relay_holds_admin(&pool, &second_admin).await);
        assert!(relay_holds_admin(&pool, &hub_keys().admin).await);

        // A drained pull reaches the demotion first, so the set still leaves the key out.
        for (writer, floor) in floors {
            sqlx::query("UPDATE relay_floors SET floor = ? WHERE writer_id = ?")
                .bind(floor)
                .bind(writer)
                .execute(&pool)
                .await
                .expect("restore floor");
        }
        let drained = phone.admin_pass(&relay, MAX_EXCHANGE_BYTES).await;
        assert_eq!(drained.problem, None, "{drained:?}");
        assert!(!relay_holds_admin(&pool, &second_admin).await);
        assert!(relay_holds_admin(&pool, &hub_keys().admin).await);
        pool.close().await;
        hub_device.close().await;
        phone.close().await;
    });
}

#[test]
fn asset_chunks_round_trip_and_a_tampered_chunk_is_refused() {
    block_on(async {
        let relay = start_relay("relay-assets").await;
        let (mut hub_device, members) = keyed_space("assets", &[PHONE_DEVICE, TABLET_DEVICE]).await;
        let mut members = members.into_iter();
        let mut phone = members.next().expect("phone");
        let mut tablet = members.next().expect("tablet");
        hub_device.claim(&relay).await;

        add_note(&mut *acquire(&hub_device.replica.pool).await, "note-images").await;
        let large = png(RELAY_ASSET_CHUNK_BYTES + 17, 3);
        let image = hub_device.replica.add_image("note-images", 0, &large).await;
        hub_device.replica.seal().await;
        let uploaded = hub_device.pass(&relay).await;
        assert_eq!(uploaded.problem, None);
        assert_eq!(uploaded.uploaded, 1);

        // Pulled records ask for a follow-up pass, since their assets are
        // referenced only once they apply.
        let pulled = phone.pass(&relay).await;
        assert_eq!(pulled.problem, None);
        assert!(pulled.pulled > 0 && pulled.more, "{pulled:?}");
        phone.replica.apply().await;

        // A metered pass leaves the large file for an unmetered one.
        let metered = phone
            .try_pass(&relay, crate::relay::METERED_MAX_ASSET_BYTES.min(1024))
            .await
            .expect("metered pass");
        assert_eq!((metered.downloaded, metered.deferred_assets), (0, 1));
        assert_eq!(phone.replica.asset_bytes(&image), None);
        let downloaded = phone.pass(&relay).await;
        assert_eq!(downloaded.problem, None);
        assert_eq!(downloaded.downloaded, 1);
        assert_eq!(phone.replica.asset_bytes(&image), Some(large.clone()));

        // A relay that flips one byte of a stored chunk gets the file refused.
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", relay.database.display()))
            .await
            .expect("open relay database");
        let chunk: Vec<u8> =
            sqlx::query_scalar("SELECT chunk FROM relay_assets WHERE chunk_index = 1")
                .fetch_one(&pool)
                .await
                .expect("stored chunk");
        let mut tampered = chunk.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        sqlx::query("UPDATE relay_assets SET chunk = ? WHERE chunk_index = 1")
            .bind(tampered)
            .execute(&pool)
            .await
            .expect("tamper");
        pool.close().await;
        assert_eq!(tablet.pass(&relay).await.problem, None);
        tablet.replica.apply().await;
        let refused = tablet.pass(&relay).await;
        assert_eq!(refused.downloaded, 0);
        assert_eq!(
            refused.problem,
            Some(RelayProblem::AssetRefused),
            "{refused:?}"
        );
        assert_eq!(tablet.replica.asset_bytes(&image), None);
        hub_device.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn an_access_proof_bound_to_another_connection_is_refused() {
    block_on(async {
        let relay = start_relay("relay-exporter").await;
        let (hub_device, members) = keyed_space("exporter", &[]).await;
        assert!(members.is_empty());
        hub_device.claim(&relay).await;
        let space = hub_device.replica.ctx.space;
        let access = hub_device.access(1).await;
        let mut first = Connection::open(&relay.target, space, LoopbackPolicy::Allow)
            .await
            .expect("connect");
        let mut second = Connection::open(&relay.target, space, LoopbackPolicy::Allow)
            .await
            .expect("connect");
        first.hello().await.expect("hello");
        second.hello().await.expect("hello");
        assert_ne!(first.binding(), second.binding());
        let signature =
            encode_relay_bytes(&access.sign(RELAY_AUTH_DOMAIN, &first.binding().auth_payload(1)));
        let replayed = second
            .request(&RelayRequest::Auth {
                epoch: 1,
                signature: signature.clone(),
            })
            .await
            .expect("auth");
        assert!(
            matches!(
                replayed,
                RelayResponse::Refused {
                    code: RelayRefusalCode::Authentication,
                    ..
                }
            ),
            "{replayed:?}"
        );
        let own = first
            .request(&RelayRequest::Auth {
                epoch: 1,
                signature,
            })
            .await
            .expect("auth");
        assert!(
            matches!(own, RelayResponse::Authenticated { .. }),
            "{own:?}"
        );
        hub_device.close().await;
    });
}

#[test]
fn a_loopback_relay_is_refused_when_loopback_dialing_is_off() {
    block_on(async {
        let relay = start_relay("relay-loopback").await;
        let error = Connection::open(
            &relay.target,
            ganbaru_sync_contracts::SpaceId::from_bytes([3; 16]),
            LoopbackPolicy::Reject,
        )
        .await
        .expect_err("loopback is refused");
        assert_eq!(error.problem, RelayProblem::Unreachable, "{error}");
        assert!(error.detail.contains("not allowed"), "{error}");
    });
}
