//! Full relay passes against an in-process relay server: devices converge only through the
//! relay, a removed device opens nothing sealed after its removal, an admin repairs a wrong epoch
//! registration, asset chunks round trip and a tampered chunk is refused, and an access proof
//! bound to another connection is refused.

use super::handover::{commit_sequenced, sequence_here};
use super::{
    HUB_DEVICE, PHONE_DEVICE, Replica, TABLET_DEVICE, block_on, device_key, done, hub, hub_keys,
    png,
};
use crate::authority;
use crate::client::{MAX_EXCHANGE_BYTES, PeerIds};
use crate::keys::SpaceKeys;
use crate::outbox::Outbox;
use crate::relay::{
    Connection, RelayLimits, RelayPass, RelaySession, RelayTarget, UNMETERED_MAX_ASSET_BYTES,
    claim_space,
};
use crate::tests::{NOW_MS, TestDirectory, VAULT_ID, acquire, add_note};
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::protocol::relay::{
    RELAY_AUTH_DOMAIN, RELAY_EPOCH_DOMAIN, RELAY_SETUP_TOKEN_LIFETIME, RelayRefusalCode,
    RelayRequest, RelayResponse, RelaySetupToken, encode_relay_bytes,
};
use ganbaru_sync::{AuthorizationView, RelayKeys};
use ganbaru_sync_contracts::bounds::RELAY_ASSET_CHUNK_BYTES;
use ganbaru_sync_contracts::{EncryptionKeyPair, RelayAccessKeyPair, SpaceKey};
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

    async fn try_pass(&self, relay: &TestRelay, max_asset_bytes: u32) -> Result<RelayPass, String> {
        let admin = hub_keys().admin;
        let own_writers = self.replica.writer.own_writers();
        let quarantined = BTreeSet::new();
        RelaySession {
            engine: &self.replica.engine,
            pool: &self.replica.pool,
            ctx: &self.replica.ctx,
            holder: self.keys.as_ref(),
            admin: (self.replica.device_id == HUB_DEVICE).then_some(&admin),
            own_writers: &own_writers,
            quarantined: &quarantined,
            assets: Some(&self.replica.assets),
            limits: RelayLimits {
                max_bytes: MAX_EXCHANGE_BYTES,
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
fn an_admin_pass_repairs_a_hijacked_epoch_registration_and_delivery_resumes() {
    block_on(async {
        let relay = start_relay("relay-hijack").await;
        let (mut hub_device, members) = keyed_space("hijack", &[PHONE_DEVICE]).await;
        let mut phone = members.into_iter().next().expect("phone");
        hub_device.claim(&relay).await;
        assert_eq!(hub_device.pass(&relay).await.problem, None);
        assert_eq!(phone.pass(&relay).await.problem, None);

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
                entry: encode_relay_bytes(&[7; 64]),
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
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("refused")),
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
        assert!(error.contains("not allowed"), "{error}");
    });
}
