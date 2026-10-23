use super::{NOW_MS, TestDirectory, VAULT_ID, acquire, add_note, block_on, create_vault, person};
use crate::assets::{
    ASSET_UNAVAILABLE, AssetMemory, AssetSession, AssetStore, AssetTransfer, ContentAddressedStore,
    INVALID_ASSET, MAX_ASSET_PASS_BYTES,
};
use crate::authority::{self, AnchorKeys, Enrollment, Founding, ensure_charter};
use crate::client::{
    ClientSession, Exchange, Exchanged, MAX_EXCHANGE_BYTES, PeerIds, SyncTransport, wait,
};
use crate::hub::{
    BOOTSTRAP_REQUIRED, HubContact, HubContactSource, HubStability, HubVault, OpenHubVault, SyncHub,
};
use crate::recovery::{
    RecoveredRow, RecoveryAction, RecoveryChoice, RecoveryRestorers, RestoreFuture,
};
use crate::wire::{asset_to_wire, bytes_to_wire};
use crate::writer::{ActiveWriter, SuccessorPlan, create_writer};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::protocol::{
    ControlMessage, PROTOCOL_VERSION, SYNC_ASSET_CHUNK_BYTES, SYNC_PAGE_BYTES, SyncAsset,
    SyncRefusalCode,
};
use ganbaru_quick_notes::images;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::local;
use ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS;
use ganbaru_sync::manifest::vault::quick_notes::{NOTES, NOTES_TABLE};
use ganbaru_sync::{Engine, SpaceContext};
use ganbaru_sync::{LocalWriter, SequenceReservation};
use ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_DEVICE;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKeyPair, DeviceKind, ProposalRejection, RecoverySeed, SignedCertificate,
    SpaceId, VersionVector, WriterCertificate, WriterId, WriterKeyPair, sign_certificate,
};
use sha2::{Digest as _, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;

const HUB_DEVICE: &str = "device-hub";
const PHONE_DEVICE: &str = "device-phone";
const TABLET_DEVICE: &str = "device-tablet";

/// Calls the hub directly, validating both frames as the transport would.
struct DirectTransport(SyncHub);

impl SyncTransport for DirectTransport {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        message.validate()?;
        let response = self.0.respond(message).await;
        response.validate()?;
        Ok(response)
    }
}

/// One replica: its vault database and the writer of its installation.
struct Replica {
    directory: TestDirectory,
    registry: DatabasePoolRegistry,
    pool: SqlitePool,
    ctx: SpaceContext,
    writer: ActiveWriter,
    engine: Engine,
    assets: Arc<dyn AssetStore>,
}

/// The device key of a test device, derived from its id.
fn device_key(device_id: &str) -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&Sha256::digest(device_id.as_bytes()).into()).expect("device seed")
}

/// The anchor keys of [`HUB_DEVICE`].
fn hub_keys() -> AnchorKeys {
    AnchorKeys {
        device: device_key(HUB_DEVICE),
        admin: AdminKeyPair::from_seed(&[21; 32]).expect("admin seed"),
        recovery: RecoverySeed::from_bytes([22; 32]),
    }
}

/// A reservation that never needs to persist, for writers that seal only during setup.
struct Unreserved;

impl SequenceReservation for Unreserved {
    fn ensure_reserved(&mut self, _seq: u64) -> std::io::Result<()> {
        Ok(())
    }
}

/// Database image of a vault whose space [`HUB_DEVICE`] founded as its anchor, with the phone
/// and the tablet enrolled by an earlier writer of the hub.
static SHARED_VAULT: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn shared_image() -> std::sync::MutexGuard<'static, Option<Vec<u8>>> {
    SHARED_VAULT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

async fn shared_vault_image() -> Vec<u8> {
    if let Some(image) = shared_image().as_ref() {
        return image.clone();
    }
    let directory = TestDirectory::new("shared-vault");
    let registry = DatabasePoolRegistry::default();
    let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
    let mut conn = acquire(&pool).await;
    let engine = Engine::vault();
    engine
        .init_space(&mut conn, VAULT_ID, NOW_MS)
        .await
        .expect("initialize space");
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read space")
        .expect("space context");
    let anchor = hub_keys();
    let founding = Founding {
        device_id: HUB_DEVICE,
        tls_fingerprint: [23; 32],
        discovery_key: [24; 32],
        person: &person(),
        now_ms: NOW_MS,
    };
    ensure_charter(&engine, &mut conn, &ctx, &anchor, &founding)
        .await
        .expect("found space");
    let key = WriterKeyPair::from_seed(&[25; 32]).expect("writer seed");
    let certificate: SignedCertificate = sign_certificate(
        &WriterCertificate {
            writer_key: key.public_key(),
            device_id: HUB_DEVICE.to_string(),
            device_key: anchor.device.public_key(),
            created_at_ms: i64::try_from(NOW_MS).expect("clock"),
            predecessor: None,
        },
        &anchor.device,
    )
    .expect("certify authority writer");
    let mut reservation = Unreserved;
    for device_id in [PHONE_DEVICE, TABLET_DEVICE] {
        let mut writer = LocalWriter {
            key: &key,
            certificate: &certificate,
            reservation: &mut reservation,
        };
        let enrollment = Enrollment {
            device_id: device_id.to_string(),
            device_key: device_key(device_id).public_key(),
            tls_fingerprint: [26; 32],
            label: device_id.to_string(),
            device_kind: DeviceKind::Mobile,
        };
        authority::enroll(
            &engine,
            &mut conn,
            &ctx,
            &anchor.admin,
            &mut writer,
            enrollment,
            NOW_MS,
        )
        .await
        .expect("enroll device");
    }
    let image = conn
        .serialize(None)
        .await
        .expect("serialize shared vault")
        .to_vec();
    drop(conn);
    drop(pool);
    registry.close_all().await.expect("close shared vault");
    shared_image().get_or_insert(image).clone()
}

impl Replica {
    async fn new(label: &str, device_id: &str) -> Self {
        let directory = TestDirectory::new(label);
        let registry = DatabasePoolRegistry::default();
        let path = directory.path().join("vault.sqlite");
        std::fs::write(&path, shared_vault_image().await).expect("write shared vault");
        let pool = registry.connect_path(&path).await.expect("open vault");
        Self::open(directory, registry, pool, device_id).await
    }

    async fn open(
        directory: TestDirectory,
        registry: DatabasePoolRegistry,
        pool: SqlitePool,
        device_id: &str,
    ) -> Self {
        let ctx = local::space_context(&mut *acquire(&pool).await, VAULT_ID)
            .await
            .expect("read space")
            .expect("space context");
        let writer = create_writer(
            &directory.files(),
            &directory.keys(),
            SuccessorPlan::default(),
            &device_key(device_id),
            device_id,
            NOW_MS,
        )
        .expect("create writer");
        let assets: Arc<dyn AssetStore> = Arc::new(
            ContentAddressedStore::new(directory.path().to_path_buf())
                .with_check(&IMAGES_CLASS, images::check_synced_image),
        );
        Self {
            engine: Engine::vault(),
            assets,
            directory,
            registry,
            pool,
            ctx,
            writer,
        }
    }

    /// Removes `device_id` through this replica's writer, signed with the hub's admin key.
    async fn remove(&mut self, device_id: &str) -> Result<u64, String> {
        let mut conn = acquire(&self.pool).await;
        let revision = authority::remove(
            &self.engine,
            &mut conn,
            &self.ctx,
            &hub_keys().admin,
            &mut self.writer.local(),
            device_id,
            NOW_MS,
        )
        .await?;
        let seq = self
            .engine
            .stored_vector(&mut conn, &self.ctx)
            .await
            .expect("stored vector")
            .get(&self.writer.id());
        self.writer.committed(seq).expect("record commit");
        Ok(revision)
    }

    async fn seal_note(&mut self, id: &str) -> u64 {
        add_note(&mut *acquire(&self.pool).await, id).await;
        self.seal().await
    }

    /// Runs a local edit and seals it.
    async fn edit(&mut self, sql: &'static str, id: &str) {
        sqlx::query(sql)
            .bind(id)
            .execute(&self.pool)
            .await
            .expect("edit");
        self.seal().await;
    }

    async fn seal(&mut self) -> u64 {
        let mut conn = acquire(&self.pool).await;
        let seq = Engine::vault()
            .seal(&mut conn, &self.ctx, &mut self.writer.local(), NOW_MS)
            .await
            .expect("seal")
            .last_seq
            .expect("sealed operations");
        self.writer.committed(seq).expect("record commit");
        seq
    }

    async fn apply(&mut self) {
        let mut conn = acquire(&self.pool).await;
        let report = Engine::vault()
            .apply(&mut conn, &self.ctx, Some(&mut self.writer.local()), NOW_MS)
            .await
            .expect("apply");
        assert!(!report.more, "test logs apply in one round");
    }

    async fn stored(&self) -> VersionVector {
        Engine::vault()
            .stored_vector(&mut *acquire(&self.pool).await, &self.ctx)
            .await
            .expect("stored vector")
    }

    /// Compacts every applied operation below each writer's head, and returns how many bodies
    /// it dropped.
    async fn compact(&self) -> usize {
        let limit = self.stored().await;
        let mut conn = acquire(&self.pool).await;
        Engine::vault()
            .compact(&mut conn, &self.ctx, &limit)
            .await
            .expect("compact")
            .compacted
    }

    async fn has_note(&self, id: &str) -> bool {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM quick_notes WHERE id = ?")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .expect("count notes")
            > 0
    }

    async fn exchange(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
    ) -> Result<Exchange, String> {
        let own_writers = self.writer.own_writers();
        self.exchange_with(
            transport,
            ids,
            self.writer.id(),
            &own_writers,
            MAX_EXCHANGE_BYTES,
        )
        .await
    }

    async fn exchange_with(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        active: WriterId,
        own_writers: &BTreeSet<WriterId>,
        max_bytes: usize,
    ) -> Result<Exchange, String> {
        let engine = Engine::vault();
        ClientSession {
            transport,
            ids,
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            writer: Some(active),
            own_writers,
            max_bytes,
        }
        .exchange()
        .await
    }

    async fn close(self) {
        drop(self.pool);
        self.registry.close_all().await.expect("close pools");
    }

    /// Stores an image file and adds it to a note, without sealing.
    async fn add_image(&self, note_id: &str, sort_order: i64, bytes: &[u8]) -> AssetRef {
        images::import_image(self.directory.path(), bytes).expect("import image");
        let image = reference(bytes);
        sqlx::query(
            "INSERT INTO quick_note_images
                (note_id, sort_order, sha256, content_type, byte_size, width, height)
             VALUES (?, ?, ?, ?, ?, 40, 30)",
        )
        .bind(note_id)
        .bind(sort_order)
        .bind(image.sha256())
        .bind(image.content_type())
        .bind(image.byte_size())
        .execute(&self.pool)
        .await
        .expect("add image");
        image
    }

    fn asset_bytes(&self, asset: &AssetRef) -> Option<Vec<u8>> {
        std::fs::read(asset.path(self.directory.path())).ok()
    }

    /// Names of every entry in the directory of a class, temporary files included.
    fn class_entries(&self) -> Vec<String> {
        match std::fs::read_dir(IMAGES_CLASS.directory(self.directory.path())) {
            Ok(entries) => entries
                .map(|entry| {
                    entry
                        .expect("asset entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("list assets: {error}"),
        }
    }

    async fn upload(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        memory: &mut AssetMemory,
        max_bytes: usize,
    ) -> AssetTransfer {
        let mut transfer = AssetTransfer::default();
        self.assets(transport, ids, max_bytes)
            .upload(memory, &mut transfer)
            .await
            .expect("upload images");
        transfer
    }

    async fn download(
        &self,
        transport: &DirectTransport,
        ids: &PeerIds,
        memory: &mut AssetMemory,
    ) -> AssetTransfer {
        let mut transfer = AssetTransfer::default();
        self.assets(transport, ids, MAX_ASSET_PASS_BYTES)
            .download(memory, &mut transfer)
            .await
            .expect("download images");
        transfer
    }

    fn assets<'a>(
        &'a self,
        transport: &'a DirectTransport,
        ids: &'a PeerIds,
        max_bytes: usize,
    ) -> AssetSession<'a, DirectTransport> {
        AssetSession {
            transport,
            ids,
            engine: &self.engine,
            pool: &self.pool,
            store: &self.assets,
            max_bytes,
        }
    }
}

fn phone_ids() -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: PHONE_DEVICE.to_string(),
    }
}

/// Opens the replica's vault as the vault a hub serves.
fn open_hub(replica: &Replica) -> OpenHubVault {
    let pool = replica.pool.clone();
    let assets = Arc::clone(&replica.assets);
    Arc::new(move || {
        let pool = pool.clone();
        let assets = Arc::clone(&assets);
        Box::pin(async move {
            Ok(Some(HubVault {
                pool,
                vault_id: VAULT_ID.to_string(),
                assets,
            }))
        })
    })
}

/// A hub serving the replica's vault, with the change signal its service would bump.
fn hub(replica: &Replica) -> (DirectTransport, watch::Sender<u64>) {
    let changes = watch::Sender::new(0);
    let hub = SyncHub::new(open_hub(replica), changes.clone())
        .with_contact(contact_source(test_contact()));
    (DirectTransport(hub), changes)
}

/// The contact a test hub reports.
fn test_contact() -> HubContact {
    HubContact {
        endpoints: vec![
            "192.168.1.20:43821".to_string(),
            "[fd00::20]:43821".to_string(),
        ],
        discovery_key: bytes_to_wire(&[7; ganbaru_handoff::protocol::DISCOVERY_KEY_BYTES]),
    }
}

fn contact_source(contact: HubContact) -> HubContactSource {
    Arc::new(move |vault_id| {
        if vault_id == VAULT_ID {
            Ok(contact.clone())
        } else {
            Err("no linked device uses this vault".to_string())
        }
    })
}

fn done(exchange: Exchange) -> Exchanged {
    match exchange {
        Exchange::Done(done) => done,
        Exchange::Fork(fork) => panic!("unexpected fork: {fork:?}"),
    }
}

fn copy_database(from: &Path, to: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let source = Path::new(&format!("{}{suffix}", from.display())).to_path_buf();
        if source.exists() {
            std::fs::copy(&source, format!("{}{suffix}", to.display())).expect("copy database");
        }
    }
}

#[test]
fn exchange_pushes_and_pulls_until_both_replicas_hold_every_operation() {
    block_on(async {
        let mut hub_replica = Replica::new("hub", HUB_DEVICE).await;
        let mut phone = Replica::new("phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-hub").await;
        phone.seal_note("note-phone").await;

        let exchanged = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(exchanged.pushed > 0);
        assert!(exchanged.pulled > 0);
        assert_eq!(exchanged.problem, None);
        assert_eq!(exchanged.known, hub_replica.stored().await);
        assert_eq!(phone.stored().await, hub_replica.stored().await);

        hub_replica.apply().await;
        phone.apply().await;
        for replica in [&hub_replica, &phone] {
            assert!(replica.has_note("note-hub").await);
            assert!(replica.has_note("note-phone").await);
        }

        let again = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!((again.pushed, again.pulled), (0, 0));
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn an_exchange_stops_at_its_byte_bound_and_the_next_one_continues() {
    block_on(async {
        let mut hub_replica = Replica::new("hub", HUB_DEVICE).await;
        let mut phone = Replica::new("phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        for index in 0..3 {
            hub_replica.seal_note(&format!("note-{index}")).await;
        }
        let own_writers = phone.writer.own_writers();

        let first = done(
            phone
                .exchange_with(&transport, &phone_ids(), phone.writer.id(), &own_writers, 1)
                .await
                .expect("bounded exchange"),
        );
        assert!(first.more);
        assert_eq!(first.problem, None);
        assert_eq!(first.pulled, 1);
        assert_ne!(phone.stored().await, hub_replica.stored().await);

        let mut exchanges = 1;
        loop {
            let next = done(
                phone
                    .exchange_with(&transport, &phone_ids(), phone.writer.id(), &own_writers, 1)
                    .await
                    .expect("bounded exchange"),
            );
            exchanges += 1;
            assert_eq!(next.problem, None);
            if !next.more {
                break;
            }
            assert!(exchanges < 64, "bounded exchanges did not finish");
        }
        assert_eq!(phone.stored().await, hub_replica.stored().await);
        phone.apply().await;
        for index in 0..3 {
            assert!(phone.has_note(&format!("note-{index}")).await);
        }

        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn wait_returns_once_the_hub_stores_new_operations() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-wait", HUB_DEVICE).await;
        let phone = Replica::new("phone-wait", PHONE_DEVICE).await;
        let (transport, changes) = hub(&hub_replica);
        hub_replica.seal_note("note-first").await;
        let known = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        )
        .known;

        let ids = phone_ids();
        let (answer, ()) = tokio::join!(wait(&transport, &ids, &known), async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            hub_replica.seal_note("note-second").await;
            changes.send_modify(|generation| *generation += 1);
        });
        let answer = answer.expect("wait");
        assert!(!known.dominates(&answer));
        assert_eq!(answer, hub_replica.stored().await);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn sync_states_carry_the_coordinator_contact_from_its_source() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-contact", HUB_DEVICE).await;
        hub_replica.seal_note("note-hub").await;
        let (transport, _changes) = hub(&hub_replica);
        let hello = ControlMessage::SyncHello {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            manifest_version: Engine::vault().manifest().version,
            stored: Vec::new(),
            probe: None,
        };
        let wait = ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            known: Vec::new(),
        };
        let expected = test_contact();
        for request in [hello.clone(), wait] {
            match transport.request(request).await.expect("sync state") {
                ControlMessage::SyncState {
                    coordinator_endpoints,
                    discovery_key,
                    ..
                } => {
                    assert_eq!(coordinator_endpoints, expected.endpoints);
                    assert_eq!(discovery_key, expected.discovery_key);
                }
                other => panic!("expected a sync state, got {other:?}"),
            }
        }

        let silent = SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0));
        assert_eq!(
            error_code(silent.respond(hello.clone()).await),
            "sync_unavailable",
            "a hub without a contact source answers no state"
        );
        let public = SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0)).with_contact(
            contact_source(HubContact {
                endpoints: vec!["203.0.113.5:43821".to_string()],
                ..test_contact()
            }),
        );
        assert_eq!(
            error_code(public.respond(hello).await),
            "sync_failed",
            "a contact outside the wire bounds fails the request, not the frame"
        );
        hub_replica.close().await;
    });
}

#[test]
fn sync_states_report_the_published_stability_within_what_the_hub_stores() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-stable", HUB_DEVICE).await;
        let phone = Replica::new("phone-stable", PHONE_DEVICE).await;
        let sealed = hub_replica.seal_note("note-hub").await;
        let stability = HubStability::default();
        let transport = DirectTransport(
            SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0))
                .with_contact(contact_source(test_contact()))
                .with_stability(stability.clone()),
        );

        let unpublished = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(unpublished.stable.is_empty());
        assert!(!unpublished.bootstrap_required);

        let hub_writer = hub_replica.writer.id();
        let unknown = WriterId::from_bytes([9; ganbaru_sync_contracts::ids::ID_BYTES]);
        stability.publish(
            VAULT_ID,
            [(hub_writer, sealed + 5), (unknown, 3)]
                .into_iter()
                .collect(),
        );
        let published = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        let expected: VersionVector = [(hub_writer, sealed)].into_iter().collect();
        assert_eq!(
            published.stable, expected,
            "stability never exceeds what the hub stores"
        );

        stability.publish("other-vault", expected.clone());
        assert!(stability.stable(VAULT_ID).is_empty());
        stability.publish(VAULT_ID, expected);
        stability.clear();
        assert!(stability.stable(VAULT_ID).is_empty());
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_matching_probe_confirms_the_head_of_the_active_writer() {
    block_on(async {
        let hub_replica = Replica::new("hub-confirm", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-confirm", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let sealed = phone.seal_note("note-phone").await;
        let first = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(first.confirmed, None, "the hub held nothing at the probe");
        let second = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(second.confirmed, Some((phone.writer.id(), sealed)));
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_device_behind_compacted_operations_bootstraps_and_keeps_pushing() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-compacted", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-compacted", PHONE_DEVICE).await;
        let mut tablet = Replica::new("tablet-compacted", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-1").await;
        hub_replica.seal_note("note-2").await;
        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        hub_replica.seal_note("note-3").await;
        assert!(hub_replica.compact().await > 0);

        // The tablet holds every compacted operation, so it pulls the rest as before.
        let caught_up = done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert!(!caught_up.bootstrap_required);
        assert_eq!(caught_up.problem, None);
        assert_eq!(tablet.stored().await, hub_replica.stored().await);

        // The phone lacks them: it still pushes its own work but pulls nothing.
        phone.seal_note("note-phone").await;
        let behind = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(behind.bootstrap_required);
        assert!(behind.pushed > 0);
        assert_eq!(behind.pulled, 0);
        hub_replica.apply().await;
        assert!(hub_replica.has_note("note-phone").await);
        tablet.apply().await;

        // A pull that ignores the signal is refused with the bootstrap code.
        let pull = ControlMessage::SyncPull {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            known: crate::wire::vector_to_wire(&phone.stored().await),
            max_bytes: u32::try_from(SYNC_PAGE_BYTES).expect("page size fits"),
        };
        match transport.request(pull).await.expect("pull answer") {
            ControlMessage::Error {
                code, retryable, ..
            } => {
                assert_eq!(code, BOOTSTRAP_REQUIRED);
                assert!(!retryable);
            }
            other => panic!("expected a bootstrap refusal, got {other:?}"),
        }

        // The hub's wait answers carry the same signal for the phone only.
        for (ids, known, expected) in [
            (phone_ids(), phone.stored().await, true),
            (tablet_ids(), tablet.stored().await, false),
        ] {
            let request = ControlMessage::SyncWait {
                protocol_version: PROTOCOL_VERSION,
                vault_id: VAULT_ID.to_string(),
                device_id: ids.device_id.clone(),
                known: crate::wire::vector_to_wire(&known),
            };
            match transport.request(request).await.expect("wait answer") {
                ControlMessage::SyncState {
                    bootstrap_required, ..
                } => assert_eq!(bootstrap_required, expected, "{}", ids.device_id),
                other => panic!("expected a sync state, got {other:?}"),
            }
        }
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn a_fork_inside_compacted_operations_is_refused_as_a_bootstrap() {
    let writer = WriterId::from_bytes([4; ganbaru_sync_contracts::ids::ID_BYTES]);
    let compacted: VersionVector = [(writer, 5)].into_iter().collect();
    let refusal = |seq| {
        crate::hub::refusal_to_wire(
            ganbaru_sync::StoreRefusal::Fork { seq },
            writer,
            seq,
            &compacted,
        )
        .code
    };
    assert_eq!(refusal(5), SyncRefusalCode::BootstrapRequired);
    assert_eq!(refusal(6), SyncRefusalCode::Fork);
}

#[test]
fn hub_refuses_foreign_geneses_unlinked_writers_and_other_vaults() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-refuse", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-refuse", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-phone").await;

        let other_vault = PeerIds {
            vault_id: "vault-other".to_string(),
            device_id: PHONE_DEVICE.to_string(),
        };
        let error = phone
            .exchange(&transport, &other_vault)
            .await
            .expect_err("another vault is refused");
        assert!(error.contains("vault_mismatch"), "{error}");

        let impostor = PeerIds {
            vault_id: VAULT_ID.to_string(),
            device_id: "device-other".to_string(),
        };
        let refused = done(
            phone
                .exchange(&transport, &impostor)
                .await
                .expect("exchange"),
        );
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("UnknownWriter")),
            "{refused:?}"
        );
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), 0);

        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        // A removal keeps every operation the anchor stored before it.
        hub_replica.apply().await;
        let cutoff = hub_replica.stored().await.get(&phone.writer.id());
        assert!(cutoff > 0);
        hub_replica
            .remove(PHONE_DEVICE)
            .await
            .expect("remove the phone");
        phone.seal_note("note-after-unlink").await;
        let refused = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("Revoked")),
            "{refused:?}"
        );
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), cutoff);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn only_the_anchor_sequences_authorization_entries() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-anchor-only", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-anchor-only", PHONE_DEVICE).await;
        let before = phone.stored().await;
        // A modified build on the phone signs a removal with an admin key it does not hold.
        let error = phone
            .remove(HUB_DEVICE)
            .await
            .expect_err("a member cannot sequence");
        assert!(
            error.contains(&format!("{:?}", ProposalRejection::NotAnchor)),
            "{error}"
        );
        assert_eq!(phone.stored().await, before);
        hub_replica
            .remove(PHONE_DEVICE)
            .await
            .expect("the anchor removes");
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn hub_refuses_a_device_past_its_writer_limit() {
    block_on(async {
        let hub_replica = Replica::new("hub-writer-limit", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-writer-limit", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        // A modified build mints a fresh writer for every operation.
        for index in 0..MAX_WRITERS_PER_DEVICE {
            if index > 0 {
                phone.writer = create_writer(
                    &phone.directory.files(),
                    &phone.directory.keys(),
                    SuccessorPlan::default(),
                    &device_key(PHONE_DEVICE),
                    PHONE_DEVICE,
                    NOW_MS,
                )
                .expect("create writer");
            }
            phone.seal_note(&format!("note-{index}")).await;
        }
        let within = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(within.problem, None, "{within:?}");

        phone.writer = create_writer(
            &phone.directory.files(),
            &phone.directory.keys(),
            SuccessorPlan::default(),
            &device_key(PHONE_DEVICE),
            PHONE_DEVICE,
            NOW_MS,
        )
        .expect("create writer");
        phone.seal_note("note-past-limit").await;
        let refused = done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(refused.refusal, Some(SyncRefusalCode::WriterLimit));
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), 0);
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_cloned_installation_is_found_and_resealed_by_a_successor() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-fork", HUB_DEVICE).await;
        let mut original = Replica::new("phone-fork", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let shared = original.seal_note("note-shared").await;
        done(
            original
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );

        // A disk clone: the same writer continues in two copies of the database.
        let original_path = original.directory.path().join("vault.sqlite");
        drop(original.pool);
        original.registry.close_all().await.expect("close original");
        let clone_directory = TestDirectory::new("phone-clone");
        let clone_path = clone_directory.path().join("vault.sqlite");
        copy_database(&original_path, &clone_path);
        original.pool = original
            .registry
            .connect_path(&original_path)
            .await
            .expect("reopen original");
        let clone_registry = DatabasePoolRegistry::default();
        let clone_pool = clone_registry
            .connect_path(&clone_path)
            .await
            .expect("open clone");

        let forked_seq = original.seal_note("note-original").await;
        let mut conn = acquire(&clone_pool).await;
        add_note(&mut conn, "note-clone").await;
        let clone_seq = Engine::vault()
            .seal(
                &mut conn,
                &original.ctx,
                &mut original.writer.local(),
                NOW_MS,
            )
            .await
            .expect("seal clone")
            .last_seq
            .expect("clone operations");
        drop(conn);
        assert_eq!(clone_seq, forked_seq);
        done(
            original
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );

        let forked_writer = original.writer.id();
        let mut clone =
            Replica::open(clone_directory, clone_registry, clone_pool, PHONE_DEVICE).await;
        // The clone still seals with the shared writer, so its probe reveals the fork.
        let clone_writers = BTreeSet::from([forked_writer]);
        let fork = match clone
            .exchange_with(
                &transport,
                &phone_ids(),
                forked_writer,
                &clone_writers,
                MAX_EXCHANGE_BYTES,
            )
            .await
            .expect("exchange")
        {
            Exchange::Fork(fork) => fork,
            Exchange::Done(done) => panic!("the fork went unnoticed: {done:?}"),
        };
        assert_eq!(fork.writer, forked_writer);
        assert_eq!(fork.keep_through, shared);

        {
            let mut conn = acquire(&clone.pool).await;
            let report = Engine::vault()
                .reseal_fork(
                    &mut conn,
                    &clone.ctx,
                    fork.writer,
                    fork.keep_through,
                    &mut clone.writer.local(),
                    NOW_MS,
                )
                .await
                .expect("re-seal fork");
            assert!(report.replaced_ops > 0);
            let seq = report.seal.last_seq.expect("re-sealed operations");
            clone.writer.committed(seq).expect("record commit");
        }
        let clone_writers = BTreeSet::from([forked_writer, clone.writer.id()]);
        let exchanged = done(
            clone
                .exchange_with(
                    &transport,
                    &phone_ids(),
                    clone.writer.id(),
                    &clone_writers,
                    MAX_EXCHANGE_BYTES,
                )
                .await
                .expect("exchange after re-seal"),
        );
        assert_eq!(exchanged.problem, None);
        assert_eq!(clone.stored().await, hub_replica.stored().await);

        hub_replica.apply().await;
        assert!(hub_replica.has_note("note-original").await);
        assert!(hub_replica.has_note("note-clone").await);
        hub_replica.close().await;
        original.close().await;
        clone.close().await;
    });
}

/// Exchanges once and applies on both replicas.
async fn converge(hub: &mut Replica, phone: &mut Replica, transport: &DirectTransport) {
    done(
        phone
            .exchange(transport, &phone_ids())
            .await
            .expect("exchange"),
    );
    hub.apply().await;
    phone.apply().await;
}

async fn note_json(replica: &Replica, id: &str) -> serde_json::Value {
    let note = ganbaru_quick_notes::load_note_from_pool(&replica.pool, id)
        .await
        .expect("load note");
    serde_json::to_value(note).expect("serialize note")
}

async fn recovery_titles(replica: &Replica) -> Vec<(String, String)> {
    Engine::vault()
        .recovery_entries(&mut *acquire(&replica.pool).await, &replica.ctx)
        .await
        .expect("recovery entries")
        .into_iter()
        .map(|entry| (entry.row_key, entry.presentation.title))
        .collect()
}

#[test]
fn a_resolved_title_conflict_closes_on_both_replicas() {
    block_on(async {
        let mut hub_replica = Replica::new("conflict-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("conflict-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-1").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Hub title' WHERE id = ?",
                "note-1",
            )
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Phone title' WHERE id = ?",
                "note-1",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let hub_note = note_json(&hub_replica, "note-1").await;
        let phone_note = note_json(&phone, "note-1").await;
        assert_eq!(hub_note["hasConflict"], true);
        assert_eq!(phone_note["hasConflict"], true);
        assert_eq!(hub_note["title"], phone_note["title"]);

        let conflict = ganbaru_quick_notes::conflict_from_pool(
            &phone.pool,
            VAULT_ID,
            &ganbaru_sync::DeviceNames::new(PHONE_DEVICE, HashMap::new()),
            "note-1",
        )
        .await
        .expect("read conflict");
        let conflict = serde_json::to_value(conflict).expect("serialize conflict");
        let groups = conflict["groups"].as_array().expect("groups");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["field"], "title");
        let versions = groups[0]["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let displayed: Vec<_> = versions
            .iter()
            .filter(|version| version["displayed"] == true)
            .collect();
        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0]["title"], phone_note["title"]);
        let own = versions
            .iter()
            .find(|version| version["device"]["ownDevice"] == true)
            .expect("own version");
        assert_eq!(own["title"], "Phone title");
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");

        let resolution = serde_json::from_value(serde_json::json!({
            "id": "note-1",
            "field": "title",
            "choice": { "kind": "version", "version": other["version"] },
        }))
        .expect("resolution");
        let resolved =
            ganbaru_quick_notes::resolve_from_pool(&phone.pool, VAULT_ID, resolution, NOW_MS + 1)
                .await
                .expect("resolve conflict");
        let resolved = serde_json::to_value(resolved).expect("serialize resolution");
        assert_eq!(resolved["note"]["title"], other["title"]);
        assert_eq!(resolved["note"]["hasConflict"], false);
        assert_eq!(resolved["copy"], serde_json::Value::Null);

        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let note = note_json(replica, "note-1").await;
            assert_eq!(note["title"], other["title"]);
            assert_eq!(note["hasConflict"], false);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

/// A user theme write whose live content uses `blend_canvas` and whose seed never varies.
fn theme_write(id: &str, blend_canvas: &str) -> ganbaru_themes::UserThemeWrite {
    let palette: Vec<_> = (0..32)
        .map(|slot| serde_json::json!({ "slot": slot, "value": "#aaaaaa" }))
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": id,
        "displayName": "Forest",
        "iconLabel": "dark",
        "seedIconLabel": "dark",
        "blendCanvas": blend_canvas,
        "seedBlendCanvas": "#101010",
        "derivationEngineVersion": 2,
        "calendarDefaultMode": "app-canvas",
        "calendarDefaultCustom": "#202020",
        "seedCalendarDefaultMode": "app-canvas",
        "seedCalendarDefaultCustom": "#202020",
        "tokens": [{ "kind": "source", "key": "canvas", "value": blend_canvas, "isolated": false }],
        "palette": palette,
        "seedTokens": [{ "kind": "source", "key": "canvas", "value": "#101010", "isolated": false }],
        "seedPalette": palette,
    }))
    .expect("theme write")
}

async fn theme_json(replica: &Replica, id: &str) -> serde_json::Value {
    let theme = ganbaru_themes::load_one(&replica.pool, id)
        .await
        .expect("load theme");
    serde_json::to_value(theme).expect("serialize theme")
}

#[test]
fn concurrent_theme_saves_surface_one_conflict_that_closes_on_both_replicas() {
    block_on(async {
        let mut hub_replica = Replica::new("theme-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("theme-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        ganbaru_themes::insert(&hub_replica.pool, &theme_write("theme-1", "#111111"))
            .await
            .expect("insert theme");
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        ganbaru_themes::replace_content(&hub_replica.pool, &theme_write("theme-1", "#222222"))
            .await
            .expect("hub save");
        ganbaru_themes::record_dismissal(&hub_replica.pool, "theme-1", 5)
            .await
            .expect("hub dismissal");
        hub_replica.seal().await;
        ganbaru_themes::replace_content(&phone.pool, &theme_write("theme-1", "#333333"))
            .await
            .expect("phone save");
        ganbaru_themes::record_dismissal(&phone.pool, "theme-1", 3)
            .await
            .expect("phone dismissal");
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let hub_theme = theme_json(&hub_replica, "theme-1").await;
        let phone_theme = theme_json(&phone, "theme-1").await;
        assert_eq!(hub_theme["theme"]["has_conflict"], true);
        assert_eq!(phone_theme["theme"]["has_conflict"], true);
        assert_eq!(hub_theme["theme"], phone_theme["theme"]);
        assert_eq!(hub_theme["tokens"], phone_theme["tokens"]);
        assert_eq!(phone_theme["theme"]["seed_blend_canvas"], "#101010");
        assert_eq!(phone_theme["seedTokens"][0]["value"], "#101010");
        for replica in [&hub_replica, &phone] {
            let dismissals = ganbaru_themes::load_dismissals(&replica.pool)
                .await
                .expect("load dismissals");
            let dismissals = serde_json::to_value(dismissals).expect("serialize dismissals");
            assert_eq!(dismissals[0]["engine_version"], 5);
        }

        let conflict = ganbaru_themes::conflict_from_pool(
            &phone.pool,
            VAULT_ID,
            &ganbaru_sync::DeviceNames::new(PHONE_DEVICE, HashMap::new()),
            "theme-1",
        )
        .await
        .expect("read conflict");
        let conflict = serde_json::to_value(conflict).expect("serialize conflict");
        let versions = conflict["versions"].as_array().expect("versions");
        assert_eq!(versions.len(), 2);
        let own = versions
            .iter()
            .find(|version| version["device"]["ownDevice"] == true)
            .expect("own version");
        assert_eq!(own["blendCanvas"], "#333333");
        assert_eq!(own["palette"].as_array().map(Vec::len), Some(32));
        let displayed = versions
            .iter()
            .find(|version| version["displayed"] == true)
            .expect("displayed version");
        assert_eq!(
            displayed["blendCanvas"],
            phone_theme["theme"]["blend_canvas"]
        );
        let other = versions
            .iter()
            .find(|version| version["displayed"] == false)
            .expect("hidden version");

        let resolution = serde_json::from_value(serde_json::json!({
            "id": "theme-1",
            "choice": { "kind": "version", "version": other["version"] },
        }))
        .expect("resolution");
        let resolved =
            ganbaru_themes::resolve_from_pool(&phone.pool, VAULT_ID, resolution, NOW_MS + 1)
                .await
                .expect("resolve conflict");
        let resolved = serde_json::to_value(resolved).expect("serialize resolution");
        assert_eq!(resolved["theme"]["blend_canvas"], other["blendCanvas"]);
        assert_eq!(resolved["theme"]["has_conflict"], false);

        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let theme = theme_json(replica, "theme-1").await;
            assert_eq!(theme["theme"]["blend_canvas"], other["blendCanvas"]);
            assert_eq!(theme["tokens"][0]["value"], other["blendCanvas"]);
            assert_eq!(theme["theme"]["has_conflict"], false);
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn restoring_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere() {
    block_on(async {
        let mut hub_replica = Replica::new("recovery-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("recovery-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-2").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-2")
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Kept edit' WHERE id = ?",
                "note-2",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let offer = vec![("note-2".to_string(), "Kept edit".to_string())];
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-2").await);
            assert_eq!(recovery_titles(replica).await, offer);
        }

        let restored = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &phone.ctx,
            &mut phone.writer,
            &restorers(),
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-2".to_string(),
                action: RecoveryAction::Restore,
            },
            NOW_MS + 1,
        )
        .await
        .expect("restore")
        .expect("restored note id");
        assert_eq!(note_json(&phone, &restored).await["title"], "Kept edit");

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(replica.has_note(&restored).await);
            assert!(!replica.has_note("note-2").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_restore_whose_close_fails_leaves_no_restored_row_and_the_offer_open() {
    block_on(async {
        let mut hub_replica = Replica::new("atomic-restore-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("atomic-restore-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-4").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        hub_replica
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-4")
            .await;
        phone
            .edit(
                "UPDATE quick_notes SET title = 'Kept edit' WHERE id = ?",
                "note-4",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        let offer = vec![("note-4".to_string(), "Kept edit".to_string())];
        assert_eq!(recovery_titles(&phone).await, offer);

        let choice = RecoveryChoice {
            table: NOTES_TABLE,
            row_key: "note-4".to_string(),
            action: RecoveryAction::Restore,
        };
        let foreign = SpaceContext {
            space: SpaceId::personal("another-vault"),
            ..phone.ctx.clone()
        };
        let error = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &foreign,
            &mut phone.writer,
            &restorers(),
            &choice,
            NOW_MS + 1,
        )
        .await
        .expect_err("closing in another space fails");
        assert!(error.contains("close recovery offer"), "{error}");
        let notes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quick_notes")
            .fetch_one(&phone.pool)
            .await
            .expect("count notes");
        assert_eq!(notes, 0);
        assert_eq!(recovery_titles(&phone).await, offer);

        let restored = crate::recovery::close(
            &Engine::vault(),
            &phone.pool,
            &phone.ctx,
            &mut phone.writer,
            &restorers(),
            &choice,
            NOW_MS + 2,
        )
        .await
        .expect("restore")
        .expect("restored note id");
        let notes: Vec<String> = sqlx::query_scalar("SELECT id FROM quick_notes")
            .fetch_all(&phone.pool)
            .await
            .expect("list notes");
        assert_eq!(notes, [restored]);
        assert!(recovery_titles(&phone).await.is_empty());
        hub_replica.close().await;
        phone.close().await;
    });
}

/// Runs a local multi-statement edit and seals it.
async fn edit_sql(replica: &mut Replica, sql: String) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&replica.pool)
        .await
        .expect("edit");
    replica.seal().await;
}

/// The derived row key of a Music table for `parts`, in its key input order.
fn music_key(table: &ganbaru_sync::manifest::TableSpec, parts: &[&str]) -> String {
    let prefix = table
        .derived_key
        .as_ref()
        .and_then(|derived| derived.prefix)
        .expect("derived key prefix");
    ganbaru_sync_contracts::derived_key(prefix, parts)
        .expect("derived key")
        .into_string()
}

/// One text column of `sql`, in row order.
async fn music_texts(replica: &Replica, sql: String) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&replica.pool)
        .await
        .expect("read music rows")
}

/// Restores a recovered Music playlist through its domain persistence, as the app does.
fn restore_music_playlist(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move {
        ganbaru_music_library::recovery::restore_recovered_playlist(
            conn,
            &row.values,
            &row.descendants,
            i64::try_from(NOW_MS + 1).expect("restore time"),
        )
        .await
        .map_err(|error| error.to_string())
    })
}

#[test]
fn a_restored_music_playlist_keeps_concurrent_membership_skip_range_and_intended_use_edits() {
    block_on(async {
        use ganbaru_sync::manifest::vault::music::{INTENDED_USES, MEMBERSHIPS, PLAYLISTS};
        let mut hub_replica = Replica::new("music-recovery-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("music-recovery-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let playlist = "music-playlist-recovered";
        let item = "music-item-recovered";
        let membership = music_key(&MEMBERSHIPS, &[playlist, item]);
        let order_key = ganbaru_sync_contracts::OrderKey::rank(0)
            .expect("order key")
            .into_string();
        edit_sql(
            &mut hub_replica,
            format!(
                "INSERT INTO music_library_items
                    (id, identity_key, source_kind, media_kind, original_title, discovered_at_ms,
                     updated_at_ms)
                 VALUES ('{item}', 'local:{item}', 'local-file', 'audio', 'Song', {NOW_MS}, {NOW_MS});
                 INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms, order_key)
                 VALUES ('{playlist}', 'Focus mix', {NOW_MS}, {NOW_MS}, '{order_key}');
                 INSERT INTO music_playlist_memberships
                    (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
                 VALUES ('{membership}', '{playlist}', '{item}', '{order_key}', {NOW_MS}, {NOW_MS});
                 INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES ('{membership}', 0, 1000, 2000);
                 INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
                 VALUES ('{playlist}', 'focus', '{focus}');",
                focus = music_key(&INTENDED_USES, &[playlist, "focus"]),
            ),
        )
        .await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        edit_sql(
            &mut hub_replica,
            format!("DELETE FROM music_playlists WHERE id = '{playlist}'"),
        )
        .await;
        edit_sql(
            &mut phone,
            format!(
                "UPDATE music_playlist_memberships SET weight = 'more-often'
                 WHERE id = '{membership}';
                 INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
                 VALUES ('{membership}', 1, 5000, 6000);
                 INSERT INTO music_playlist_intended_uses (playlist_id, intended_use, sync_key)
                 VALUES ('{playlist}', 'reading', '{reading}');",
                reading = music_key(&INTENDED_USES, &[playlist, "reading"]),
            ),
        )
        .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                recovery_titles(replica).await,
                [(playlist.to_string(), "Focus mix".to_string())]
            );
        }

        let restored = crate::recovery::close(
            &Engine::vault(),
            &hub_replica.pool,
            &hub_replica.ctx,
            &mut hub_replica.writer,
            &RecoveryRestorers::new().with(&PLAYLISTS, restore_music_playlist),
            &RecoveryChoice {
                table: PLAYLISTS.id,
                row_key: playlist.to_string(),
                action: RecoveryAction::Restore,
            },
            NOW_MS + 1,
        )
        .await
        .expect("restore")
        .expect("restored playlist id");
        assert_ne!(restored, playlist);

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT name || '|' || icon FROM music_playlists WHERE id = '{restored}'"
                    )
                )
                .await,
                ["Focus mix|lucide:list-music"]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT item_id || '|' || weight FROM music_playlist_memberships
                         WHERE playlist_id = '{restored}'"
                    )
                )
                .await,
                [format!("{item}|more-often")]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT skip.start_ms || '-' || skip.end_ms
                         FROM music_membership_skip_ranges AS skip
                         JOIN music_playlist_memberships AS membership
                           ON membership.id = skip.membership_id
                         WHERE membership.playlist_id = '{restored}'
                         ORDER BY skip.ordinal"
                    )
                )
                .await,
                ["1000-2000", "5000-6000"]
            );
            assert_eq!(
                music_texts(
                    replica,
                    format!(
                        "SELECT intended_use FROM music_playlist_intended_uses
                         WHERE playlist_id = '{restored}' ORDER BY intended_use"
                    )
                )
                .await,
                ["focus", "reading"]
            );
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn discarding_a_recovery_offer_keeps_the_deletion() {
    block_on(async {
        let mut hub_replica = Replica::new("discard-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("discard-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-3").await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        phone
            .edit("DELETE FROM quick_notes WHERE id = ?", "note-3")
            .await;
        hub_replica
            .edit(
                "UPDATE quick_notes SET title = 'Late edit' WHERE id = ?",
                "note-3",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        assert_eq!(recovery_titles(&hub_replica).await.len(), 1);

        let restored = crate::recovery::close(
            &Engine::vault(),
            &hub_replica.pool,
            &hub_replica.ctx,
            &mut hub_replica.writer,
            &restorers(),
            &RecoveryChoice {
                table: NOTES_TABLE,
                row_key: "note-3".to_string(),
                action: RecoveryAction::Discard,
            },
            NOW_MS + 1,
        )
        .await
        .expect("discard");
        assert_eq!(restored, None);

        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert!(!replica.has_note("note-3").await);
            assert!(recovery_titles(replica).await.is_empty());
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

fn tablet_ids() -> PeerIds {
    PeerIds {
        vault_id: VAULT_ID.to_string(),
        device_id: TABLET_DEVICE.to_string(),
    }
}

/// Header-only PNG bytes padded to `len`, enough for metadata parsing; `seed` varies the digest.
fn png(len: usize, seed: u8) -> Vec<u8> {
    let mut bytes = vec![0; 34];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
    bytes[12..16].copy_from_slice(b"IHDR");
    bytes[16..20].copy_from_slice(&40u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&30u32.to_be_bytes());
    bytes[24..29].copy_from_slice(&[8, 6, 0, 0, 0]);
    bytes[33] = seed;
    bytes.resize(len.max(34), seed);
    bytes
}

/// Restores a recovered Quick note through its domain persistence, as the app does.
fn restore_note(conn: &mut SqliteConnection, row: RecoveredRow) -> RestoreFuture<'_> {
    Box::pin(async move { ganbaru_quick_notes::restore_recovered(conn, &row.values).await })
}

/// The Quick notes restorer, registered as the app registers it.
fn restorers() -> RecoveryRestorers {
    RecoveryRestorers::new().with(&NOTES, restore_note)
}

fn reference(bytes: &[u8]) -> AssetRef {
    let image = serde_json::to_value(images::describe_image(bytes).expect("describe image"))
        .expect("serialize image");
    AssetRef::new(
        &IMAGES_CLASS,
        image["sha256"].as_str().expect("digest").to_string(),
        image["contentType"]
            .as_str()
            .expect("content type")
            .to_string(),
        u32::try_from(bytes.len()).expect("image size"),
    )
    .expect("reference image")
}

fn put(image: &AssetRef, offset: u32, bytes: &[u8]) -> ControlMessage {
    ControlMessage::SyncAssetPut {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT_ID.to_string(),
        device_id: PHONE_DEVICE.to_string(),
        asset: asset_to_wire(image),
        offset,
        data: bytes_to_wire(bytes),
    }
}

fn error_code(response: ControlMessage) -> String {
    match response {
        ControlMessage::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn received(response: ControlMessage) -> u32 {
    match response {
        ControlMessage::SyncAssetReceived { received } => received,
        other => panic!("expected upload progress, got {other:?}"),
    }
}

#[test]
fn assets_travel_through_the_hub_ahead_of_the_rows_that_reference_them() {
    block_on(async {
        let hub_replica = Replica::new("images-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("images-phone", PHONE_DEVICE).await;
        let mut tablet = Replica::new("images-tablet", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&phone.pool).await, "note-images").await;
        let large = png(SYNC_ASSET_CHUNK_BYTES * 2 + 17, 1);
        let small = png(64, 2);
        let large_image = phone.add_image("note-images", 0, &large).await;
        let small_image = phone.add_image("note-images", 1, &small).await;
        phone.seal().await;

        let mut phone_memory = AssetMemory::default();
        let uploaded = phone
            .upload(
                &transport,
                &phone_ids(),
                &mut phone_memory,
                MAX_ASSET_PASS_BYTES,
            )
            .await;
        assert_eq!(
            (uploaded.uploaded, uploaded.more, uploaded.problem),
            (2, false, None)
        );
        assert_eq!(hub_replica.asset_bytes(&large_image), Some(large.clone()));
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        let again = phone
            .upload(
                &transport,
                &phone_ids(),
                &mut phone_memory,
                MAX_ASSET_PASS_BYTES,
            )
            .await;
        assert_eq!(again, AssetTransfer::default());

        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        tablet.apply().await;
        assert_eq!(tablet.asset_bytes(&large_image), None);
        let mut tablet_memory = AssetMemory::default();
        let downloaded = tablet
            .download(&transport, &tablet_ids(), &mut tablet_memory)
            .await;
        assert_eq!((downloaded.downloaded, downloaded.problem), (2, None));
        assert_eq!(tablet.asset_bytes(&large_image), Some(large));
        assert_eq!(tablet.asset_bytes(&small_image), Some(small));
        let again = tablet
            .download(&transport, &tablet_ids(), &mut tablet_memory)
            .await;
        assert_eq!(again, AssetTransfer::default());

        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn assets_stop_transferring_at_the_pass_byte_bound_and_resume() {
    block_on(async {
        let hub_replica = Replica::new("bound-hub", HUB_DEVICE).await;
        let phone = Replica::new("bound-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&phone.pool).await, "note-images").await;
        phone.add_image("note-images", 0, &png(100, 1)).await;
        phone.add_image("note-images", 1, &png(100, 2)).await;

        let mut memory = AssetMemory::default();
        let first = phone.upload(&transport, &phone_ids(), &mut memory, 1).await;
        assert_eq!((first.uploaded, first.more), (1, true));
        let second = phone.upload(&transport, &phone_ids(), &mut memory, 1).await;
        assert_eq!((second.uploaded, second.more), (1, false));
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn the_hub_verifies_asset_uploads_and_reports_missing_assets() {
    block_on(async {
        let hub_replica = Replica::new("verify-hub", HUB_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let bytes = png(SYNC_ASSET_CHUNK_BYTES + 10, 3);
        let image = reference(&bytes);

        let get = ControlMessage::SyncAssetGet {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            asset: asset_to_wire(&image),
            offset: 0,
        };
        let missing = transport.request(get.clone()).await.expect("get");
        assert_eq!(error_code(missing), ASSET_UNAVAILABLE);

        let mut tampered = bytes.clone();
        *tampered.last_mut().expect("bytes") ^= 0xff;
        let (head, tail) = tampered.split_at(SYNC_ASSET_CHUNK_BYTES);
        let progress = transport.request(put(&image, 0, head)).await.expect("put");
        assert_eq!(received(progress), SYNC_ASSET_CHUNK_BYTES as u32);
        let refused = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(error_code(refused), INVALID_ASSET);
        assert_eq!(hub_replica.asset_bytes(&image), None);
        assert_eq!(hub_replica.class_entries(), Vec::<String>::new());

        // A chunk that does not continue the pending upload learns where to restart.
        let (head, tail) = bytes.split_at(SYNC_ASSET_CHUNK_BYTES);
        let restart = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(received(restart), 0);
        transport.request(put(&image, 0, head)).await.expect("put");
        let stored = transport
            .request(put(&image, SYNC_ASSET_CHUNK_BYTES as u32, tail))
            .await
            .expect("put");
        assert_eq!(received(stored), image.byte_size());
        assert_eq!(hub_replica.asset_bytes(&image), Some(bytes.clone()));
        let held = transport.request(put(&image, 0, head)).await.expect("put");
        assert_eq!(received(held), image.byte_size());
        match transport.request(get).await.expect("get") {
            ControlMessage::SyncAssetChunk { data } => {
                assert_eq!(data, bytes_to_wire(head));
            }
            other => panic!("expected a chunk, got {other:?}"),
        }
        hub_replica.close().await;
    });
}

#[test]
fn the_hub_refuses_assets_outside_their_registered_class_before_staging() {
    block_on(async {
        let hub_replica = Replica::new("class-hub", HUB_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        let bytes = png(64, 5);
        let image = reference(&bytes);
        let put_wire = |asset: SyncAsset| ControlMessage::SyncAssetPut {
            protocol_version: PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            asset,
            offset: 0,
            data: bytes_to_wire(&bytes),
        };

        let unregistered = SyncAsset {
            class: "notes/files".to_string(),
            ..asset_to_wire(&image)
        };
        let refused = transport
            .request(put_wire(unregistered))
            .await
            .expect("put");
        assert_eq!(error_code(refused), "invalid_sync_request");

        let oversized = SyncAsset {
            byte_size: IMAGES_CLASS.max_bytes + 1,
            ..asset_to_wire(&image)
        };
        let refused = transport.request(put_wire(oversized)).await.expect("put");
        assert_eq!(error_code(refused), "invalid_sync_request");
        assert_eq!(hub_replica.class_entries(), Vec::<String>::new());
        assert_eq!(hub_replica.asset_bytes(&image), None);
        hub_replica.close().await;
    });
}

#[test]
fn a_replica_refuses_damaged_assets_from_the_hub() {
    block_on(async {
        let mut hub_replica = Replica::new("damaged-hub", HUB_DEVICE).await;
        let mut tablet = Replica::new("damaged-tablet", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        add_note(&mut *acquire(&hub_replica.pool).await, "note-images").await;
        let image = hub_replica.add_image("note-images", 0, &png(80, 4)).await;
        hub_replica.seal().await;
        let path = image.path(hub_replica.directory.path());
        let mut damaged = std::fs::read(&path).expect("read image");
        *damaged.last_mut().expect("bytes") ^= 0xff;
        std::fs::write(&path, damaged).expect("damage image");

        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        tablet.apply().await;
        let mut memory = AssetMemory::default();
        let refused = tablet
            .download(&transport, &tablet_ids(), &mut memory)
            .await;
        assert_eq!(refused.downloaded, 0);
        assert!(
            refused
                .problem
                .as_deref()
                .is_some_and(|problem| problem.contains("do not match")),
            "{refused:?}"
        );
        assert_eq!(tablet.asset_bytes(&image), None);
        assert_eq!(tablet.class_entries(), Vec::<String>::new());
        let skipped = tablet
            .download(&transport, &tablet_ids(), &mut memory)
            .await;
        assert_eq!(skipped, AssetTransfer::default());
        hub_replica.close().await;
        tablet.close().await;
    });
}

/// Public key of the person the contacts tests add.
const PEER_KEY: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE";
const CONTACTS_AT: &str = "2026-10-07T12:00:00.000Z";
const REQUEST_EXPIRES_AT: &str = "2026-10-14T12:00:00.000Z";

/// Runs a contact update that binds the peer key as `?1` and the change time as `?2`.
async fn update_contact(replica: &Replica, sql: &'static str) {
    sqlx::query(sql)
        .bind(PEER_KEY)
        .bind(CONTACTS_AT)
        .execute(&replica.pool)
        .await
        .expect("contacts write");
}

async fn insert_contact(replica: &Replica, state: &str) {
    let blocked_at = (state == "blocked").then_some(CONTACTS_AT);
    sqlx::query(
        "INSERT INTO contacts (id, public_key, display_name, state, blocked_at, created_at, updated_at)
         VALUES ('contact-' || ?1, ?1, 'Ana', ?2, ?3, ?4, ?4)",
    )
    .bind(PEER_KEY)
    .bind(state)
    .bind(blocked_at)
    .bind(CONTACTS_AT)
    .execute(&replica.pool)
    .await
    .expect("insert contact");
}

async fn insert_request(replica: &Replica, id: &str, direction: &str) {
    sqlx::query(
        "INSERT INTO contact_requests
         (id, direction, public_key, display_name, card, card_digest, expires_at, created_at,
          updated_at)
         VALUES (?, ?, ?, 'Ana', 'Y2FyZA', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(direction)
    .bind(PEER_KEY)
    .bind("0".repeat(64))
    .bind(REQUEST_EXPIRES_AT)
    .bind(CONTACTS_AT)
    .bind(CONTACTS_AT)
    .execute(&replica.pool)
    .await
    .expect("insert request");
}

async fn contact_standing(replica: &Replica) -> Option<(String, String)> {
    sqlx::query_as("SELECT state, invite_trust FROM contacts WHERE public_key = ?")
        .bind(PEER_KEY)
        .fetch_optional(&replica.pool)
        .await
        .expect("read contact")
}

async fn request_states(replica: &Replica) -> Vec<(String, String)> {
    sqlx::query_as("SELECT id, state FROM contact_requests ORDER BY id")
        .fetch_all(&replica.pool)
        .await
        .expect("read requests")
}

#[test]
fn a_block_outranks_a_concurrent_accept_and_trust_edit() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-block-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-block-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);

        insert_contact(&hub_replica, "blocked").await;
        hub_replica.seal().await;
        insert_contact(&phone, "active").await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a block made while the other device accepted wins"
            );
        }

        update_contact(
            &hub_replica,
            "UPDATE contacts SET state = 'active', blocked_at = NULL, accepted_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        update_contact(
            &hub_replica,
            "UPDATE contacts SET state = 'blocked', blocked_at = ?2 WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a trust edit never lifts a concurrent block"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

/// Gives the peer the removed standing, as removing or unblocking a contact does.
const REMOVE_CONTACT: &str = "UPDATE contacts
     SET state = 'removed', invite_trust = 'not_allowed', invite_trust_expires_at = NULL,
         message_trust = 'not_allowed', message_trust_expires_at = NULL, accepted_at = NULL,
         blocked_at = NULL, updated_at = ?2
     WHERE public_key = ?1";

#[test]
fn removing_and_adding_one_person_on_two_devices_settles_on_one_row() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-readd-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-readd-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        assert_eq!(
            contact_standing(&phone).await,
            Some(("removed".to_string(), "not_allowed".to_string()))
        );

        update_contact(
            &phone,
            "UPDATE contacts SET state = 'active', accepted_at = ?2, updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
                .fetch_one(&replica.pool)
                .await
                .expect("count contacts");
            assert_eq!(count, 1, "the person comes back as the same row");
            assert_eq!(
                contact_standing(replica).await,
                Some(("active".to_string(), "not_allowed".to_string())),
                "an add made after the removal arrived revives the contact"
            );
        }

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("removed".to_string(), "not_allowed".to_string())),
                "a concurrent trust edit never revives a removed contact"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_block_survives_a_concurrent_removal() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-block-remove-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-block-remove-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET state = 'blocked', blocked_at = ?2, updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a removal on one device never drops a block made on another"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_concurrent_trust_narrowing_beats_a_widening() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-narrow-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-narrow-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        update_contact(
            &hub_replica,
            "UPDATE contacts SET invite_trust = 'seven_days', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(
            &hub_replica,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'not_allowed', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("active".to_string(), "not_allowed".to_string())),
                "a revocation made while another device widened trust stands"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn the_higher_card_revision_wins_a_concurrent_regeneration() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-card-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-card-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        sqlx::query(
            "INSERT INTO contact_card (key, card_nonce, card_revision, updated_at)
             VALUES ('card', 'AAAAAAAAAAAAAAAAAAAAAA', 1, ?)",
        )
        .bind(CONTACTS_AT)
        .execute(&hub_replica.pool)
        .await
        .expect("insert card");
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let rotate = |nonce: &'static str| {
            sqlx::query(
                "UPDATE contact_card SET card_nonce = ?, card_revision = card_revision + 1
                 WHERE key = 'card'",
            )
            .bind(nonce)
        };
        rotate("BBBBBBBBBBBBBBBBBBBBBB")
            .execute(&hub_replica.pool)
            .await
            .expect("hub rotate");
        rotate("CCCCCCCCCCCCCCCCCCCCCC")
            .execute(&hub_replica.pool)
            .await
            .expect("hub rotate again");
        hub_replica.seal().await;
        rotate("DDDDDDDDDDDDDDDDDDDDDD")
            .execute(&phone.pool)
            .await
            .expect("phone rotate");
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let card: (String, i64) =
                sqlx::query_as("SELECT card_nonce, card_revision FROM contact_card")
                    .fetch_one(&replica.pool)
                    .await
                    .expect("read card");
            assert_eq!(card, ("CCCCCCCCCCCCCCCCCCCCCC".to_string(), 3));
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn duplicate_pending_requests_both_survive_and_a_concurrent_delete_beats_an_answer() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-requests-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-requests-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);

        insert_request(&hub_replica, "request-hub", "sent").await;
        hub_replica.seal().await;
        insert_request(&phone, "request-phone", "sent").await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        let pending = vec![
            ("request-hub".to_string(), "pending".to_string()),
            ("request-phone".to_string(), "pending".to_string()),
        ];
        for replica in [&hub_replica, &phone] {
            assert_eq!(request_states(replica).await, pending);
        }

        insert_request(&hub_replica, "request-received", "received").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        hub_replica
            .edit(
                "DELETE FROM contact_requests WHERE id = ?",
                "request-received",
            )
            .await;
        phone
            .edit(
                "UPDATE contact_requests SET state = 'accepted' WHERE id = ?",
                "request-received",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                request_states(replica).await,
                pending,
                "the deleted request stays deleted"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

mod projects_conflicts;
mod projects_recovery;
