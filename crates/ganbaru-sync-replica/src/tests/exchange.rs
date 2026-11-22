use super::{NOW_MS, TestDirectory, VAULT_ID, acquire, add_note, block_on, create_vault, person};
use crate::assets::{
    AssetMemory, AssetSession, AssetStore, AssetTransfer, ContentAddressedStore,
    MAX_ASSET_PASS_BYTES,
};
use crate::authority::{self, AnchorKeys, Enrollment, Founding, ensure_charter};
use crate::client::{
    self, ClientSession, Exchange, Exchanged, MAX_EXCHANGE_BYTES, PeerIds, SyncTransport, wait,
};
use crate::hub::{
    BOOTSTRAP_REQUIRED, HubContact, HubContactSource, HubProposals, HubStability, HubVault,
    OpenHubVault, SyncHub,
};
use crate::outbox::Outbox;
use crate::quarantine::PullSkips;
use crate::recovery::{RecoveredRow, RecoveryRestorers, RestoreFuture};
use crate::writer::{ActiveWriter, SuccessorPlan, create_writer};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::protocol::{
    ControlMessage, MemberEndpoint, PROTOCOL_VERSION, SYNC_PAGE_BYTES, SyncRefusalCode,
};
use ganbaru_quick_notes::images;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::local;
use ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS;
use ganbaru_sync::manifest::vault::quick_notes::NOTES;
use ganbaru_sync::{Engine, EvidenceOutcome, SpaceContext};
use ganbaru_sync::{LocalWriter, SequenceReservation};
use ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_DEVICE;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKeyPair, DeviceKind, ProposalRejection, RecoverySeed, SignedCertificate,
    VersionVector, WriterCertificate, WriterId, WriterKeyPair, sign_certificate,
};
use sha2::{Digest as _, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use std::collections::BTreeSet;
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
    device_id: String,
}

/// The device key of a test device, derived from its id.
fn device_key(device_id: &str) -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&Sha256::digest(device_id.as_bytes()).into()).expect("device seed")
}

/// The recovery seed whose kit founds the space of [`HUB_DEVICE`].
fn hub_recovery_seed() -> RecoverySeed {
    RecoverySeed::from_bytes([22; 32])
}

/// The anchor keys of [`HUB_DEVICE`].
fn hub_keys() -> AnchorKeys {
    AnchorKeys {
        device: device_key(HUB_DEVICE),
        admin: AdminKeyPair::from_seed(&[21; 32]).expect("admin seed"),
        recovery: hub_recovery_seed().public_keys().expect("recovery keys"),
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
    for (device_id, fingerprint) in [(PHONE_DEVICE, 26), (TABLET_DEVICE, 30)] {
        let mut writer = LocalWriter {
            key: &key,
            certificate: &certificate,
            reservation: &mut reservation,
            keys: None,
        };
        let enrollment = Enrollment {
            device_id: device_id.to_string(),
            device_key: device_key(device_id).public_key(),
            tls_fingerprint: [fingerprint; 32],
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
    /// Hash of the charter this replica holds, which its proposals name.
    async fn charter(&self) -> ganbaru_sync_contracts::Digest32 {
        let mut conn = acquire(&self.pool).await;
        authority::charter_hash(&self.engine, &mut conn, &self.ctx)
            .await
            .expect("charter")
    }

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
            device_id: device_id.to_string(),
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

    /// Applies every received operation, over as many transactions as the apply caps take.
    async fn apply(&mut self) {
        let mut conn = acquire(&self.pool).await;
        while Engine::vault()
            .apply(&mut conn, &self.ctx, Some(&mut self.writer.local()), NOW_MS)
            .await
            .expect("apply")
            .more
        {}
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
            quarantined: &BTreeSet::new(),
            pull_skips: None,
            max_bytes,
            listening: &[],
            records: None,
        }
        .exchange()
        .await
    }

    /// Exchanges as an installation that quarantined `quarantined`, remembering what its pulls
    /// turned away in `skips`.
    async fn exchange_quarantining<T: SyncTransport>(
        &self,
        transport: &T,
        ids: &PeerIds,
        quarantined: &BTreeSet<String>,
        skips: &PullSkips,
    ) -> Result<Exchange, String> {
        let engine = Engine::vault();
        let own_writers = self.writer.own_writers();
        ClientSession {
            transport,
            ids,
            engine: &engine,
            pool: &self.pool,
            ctx: &self.ctx,
            writer: Some(self.writer.id()),
            own_writers: &own_writers,
            quarantined,
            pull_skips: Some(skips),
            max_bytes: MAX_EXCHANGE_BYTES,
            listening: &[],
            records: None,
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
    open_hub_quarantining(replica, BTreeSet::new())
}

/// Opens the replica's vault as a hub that quarantined `quarantined`.
fn open_hub_quarantining(replica: &Replica, quarantined: BTreeSet<String>) -> OpenHubVault {
    open_hub_serving(replica, quarantined, Arc::clone(&replica.assets))
}

/// Opens the replica's vault as a hub that quarantined `quarantined` and serves `assets`.
fn open_hub_serving(
    replica: &Replica,
    quarantined: BTreeSet<String>,
    assets: Arc<dyn AssetStore>,
) -> OpenHubVault {
    let pool = replica.pool.clone();
    let device_id = replica.device_id.clone();
    Arc::new(move || {
        let pool = pool.clone();
        let assets = Arc::clone(&assets);
        let quarantined = quarantined.clone();
        let device_id = device_id.clone();
        Box::pin(async move {
            Ok(Some(HubVault {
                pool,
                vault_id: VAULT_ID.to_string(),
                assets,
                quarantined,
                device_id,
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
        members: ["192.168.1.20:43821", "[fd00::20]:43821"]
            .into_iter()
            .map(|endpoint| MemberEndpoint {
                device_id: HUB_DEVICE.to_string(),
                endpoint: endpoint.to_string(),
                observed_ms: 1_700_000_000_000,
            })
            .collect(),
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
        Exchange::Fork(fork) | Exchange::UnprovenFork(fork) => panic!("unexpected fork: {fork:?}"),
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

/// A transport that counts the operations pulls download.
struct CountingTransport<'a> {
    inner: &'a DirectTransport,
    pulled: std::sync::atomic::AtomicUsize,
}

impl SyncTransport for CountingTransport<'_> {
    async fn request(&self, message: ControlMessage) -> Result<ControlMessage, String> {
        let response = self.inner.request(message).await?;
        if let ControlMessage::SyncOps { ops, .. } = &response {
            self.pulled
                .fetch_add(ops.len(), std::sync::atomic::Ordering::Relaxed);
        }
        Ok(response)
    }
}

#[test]
fn a_client_pulls_a_quarantined_backlog_once_until_the_quarantine_changes() {
    block_on(async {
        let hub_replica = Replica::new("hub-pull-skips", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-pull-skips", PHONE_DEVICE).await;
        let tablet = Replica::new("tablet-pull-skips", TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-phone-1").await;
        phone.seal_note("note-phone-2").await;
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("push"),
        );

        let counting = CountingTransport {
            inner: &transport,
            pulled: std::sync::atomic::AtomicUsize::new(0),
        };
        let tablet_ids = PeerIds {
            vault_id: VAULT_ID.to_string(),
            device_id: TABLET_DEVICE.to_string(),
        };
        let quarantined = BTreeSet::from([PHONE_DEVICE.to_string()]);
        let skips = PullSkips::default();
        let first = done(
            tablet
                .exchange_quarantining(&counting, &tablet_ids, &quarantined, &skips)
                .await
                .expect("first exchange"),
        );
        assert_eq!(first.problem, None);
        let downloaded = counting.pulled.load(std::sync::atomic::Ordering::Relaxed);
        assert!(downloaded > 0);
        assert_eq!(tablet.stored().await.get(&phone.writer.id()), 0);

        done(
            tablet
                .exchange_quarantining(&counting, &tablet_ids, &quarantined, &skips)
                .await
                .expect("second exchange"),
        );
        assert_eq!(
            counting.pulled.load(std::sync::atomic::Ordering::Relaxed),
            downloaded,
            "the turned away backlog is not downloaded again"
        );

        // Lifting the quarantine forgets the skips, so the operations arrive.
        done(
            tablet
                .exchange_quarantining(&counting, &tablet_ids, &BTreeSet::new(), &skips)
                .await
                .expect("exchange after lifting"),
        );
        assert_eq!(
            tablet.stored().await.get(&phone.writer.id()),
            phone.stored().await.get(&phone.writer.id())
        );
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn a_hub_stores_nothing_from_a_quarantined_device_until_the_quarantine_lifts() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-quarantine", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-quarantine", PHONE_DEVICE).await;
        let quarantining = DirectTransport(
            SyncHub::new(
                open_hub_quarantining(&hub_replica, BTreeSet::from([PHONE_DEVICE.to_string()])),
                watch::Sender::new(0),
            )
            .with_contact(contact_source(test_contact())),
        );
        hub_replica.seal_note("note-hub").await;
        phone.seal_note("note-phone").await;

        // The exchange still completes: the phone pulls, and the hub skips what it pushes.
        let exchanged = phone.exchange(&quarantining, &phone_ids()).await;
        assert!(exchanged.is_ok(), "{exchanged:?}");
        hub_replica.apply().await;
        phone.apply().await;
        assert!(phone.has_note("note-hub").await);
        assert!(!hub_replica.has_note("note-phone").await);
        assert_eq!(
            hub_replica.stored().await.get(&phone.writer.id()),
            0,
            "no operation of the quarantined device is stored"
        );

        // Lifting the quarantine lets the same operations arrive.
        let (lifted, _changes) = hub(&hub_replica);
        converge(&mut hub_replica, &mut phone, &lifted).await;
        assert!(hub_replica.has_note("note-phone").await);
        assert_eq!(phone.stored().await, hub_replica.stored().await);
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
            listening: vec!["192.168.1.40:43821".to_string()],
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
                ControlMessage::SyncState { members, .. } => {
                    assert_eq!(members, expected.members);
                }
                other => panic!("expected a sync state, got {other:?}"),
            }
        }

        let hints = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = hints.clone();
        let hinted = SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0))
            .with_contact(contact_source(test_contact()))
            .with_peer_hints(Arc::new(
                move |vault: &str, device: &str, listening: &[String]| {
                    recorded.lock().expect("hints").push((
                        vault.to_string(),
                        device.to_string(),
                        listening.to_vec(),
                    ));
                },
            ));
        assert!(matches!(
            hinted.respond(hello.clone()).await,
            ControlMessage::SyncState { .. }
        ));
        assert_eq!(
            *hints.lock().expect("hints"),
            [(
                VAULT_ID.to_string(),
                PHONE_DEVICE.to_string(),
                vec!["192.168.1.40:43821".to_string()]
            )],
            "a hello hands the member's listener endpoints to the peer book"
        );

        let silent = SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0));
        assert_eq!(
            error_code(silent.respond(hello.clone()).await),
            "sync_unavailable",
            "a hub without a contact source answers no state"
        );
        let public = SyncHub::new(open_hub(&hub_replica), watch::Sender::new(0)).with_contact(
            contact_source(HubContact {
                members: vec![MemberEndpoint {
                    device_id: HUB_DEVICE.to_string(),
                    endpoint: "203.0.113.5:43821".to_string(),
                    observed_ms: 1,
                }],
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
        let error = phone
            .exchange(&transport, &impostor)
            .await
            .expect_err("a device the log never enrolled is refused");
        assert!(error.contains("member_unknown"), "{error}");
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
        // A removed device reaches no hub service, so nothing past the cutoff arrives from it.
        phone.seal_note("note-after-unlink").await;
        let error = phone
            .exchange(&transport, &phone_ids())
            .await
            .expect_err("a removed device is refused");
        assert!(error.contains("member_removed"), "{error}");
        assert_eq!(hub_replica.stored().await.get(&phone.writer.id()), cutoff);
        hub_replica.apply().await;
        assert!(!hub_replica.has_note("note-after-unlink").await);
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
            error.contains(&ProposalRejection::NotAnchor.to_string()),
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

/// A hub that answers every proposal with a final rejection, as a compromised coordinator that
/// does not anchor could.
struct RejectingHub;

impl SyncTransport for RejectingHub {
    async fn request(&self, _message: ControlMessage) -> Result<ControlMessage, String> {
        Ok(crate::wire::proposed_to_wire(Err(
            ProposalRejection::DeviceRemoved,
        )))
    }
}

#[test]
fn only_the_anchor_settles_a_final_rejection() {
    block_on(async {
        let hub_replica = Replica::new("hub-final-rejection", HUB_DEVICE).await;
        let phone = Replica::new("phone-final-rejection", PHONE_DEVICE).await;
        let removal = authority::propose(
            hub_replica.ctx.space,
            hub_replica.charter().await,
            &hub_keys().admin,
            authority::remove_entry(TABLET_DEVICE),
            NOW_MS,
        )
        .expect("sign removal");
        let mut outbox = Outbox::load(&phone.directory.files().outbox_path()).expect("outbox");
        outbox.add(removal.clone()).expect("queue removal");
        // The device the proposal removes answers as a hub, but the selected log names another
        // anchor, so only the anchor's answer settles the proposal.
        for answering in [TABLET_DEVICE, HUB_DEVICE] {
            client::prune_and_replay_outbox(
                &phone.engine,
                &phone.pool,
                &phone.ctx,
                &RejectingHub,
                &phone_ids(),
                &mut outbox,
                answering,
            )
            .await
            .expect("replay");
            let rejection = outbox.get(&removal.hash()).expect("kept").rejection();
            let expected = (answering == HUB_DEVICE).then_some(ProposalRejection::DeviceRemoved);
            assert_eq!(rejection, expected, "answered by {answering}");
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn proposals_reach_the_anchor_through_the_hub_until_the_chain_holds_them() {
    block_on(async {
        let hub_replica = Replica::new("hub-proposals", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-proposals", PHONE_DEVICE).await;
        let laptop = Enrollment {
            device_id: "device-laptop".to_string(),
            device_key: device_key("device-laptop").public_key(),
            tls_fingerprint: [27; 32],
            label: "Laptop".to_string(),
            device_kind: DeviceKind::Desktop,
        };
        let enroll = authority::propose(
            hub_replica.ctx.space,
            hub_replica.charter().await,
            &hub_keys().admin,
            authority::enroll_entry(laptop),
            NOW_MS,
        )
        .expect("sign enrollment");

        // A hub without the anchor service answers that it is not the anchor.
        let (plain, _changes) = hub(&hub_replica);
        assert_eq!(
            client::propose(&plain, &phone_ids(), &enroll)
                .await
                .expect("answer"),
            Err(ProposalRejection::NotAnchor)
        );

        // The anchor service sequences what the hub hands it with its own writer.
        let anchor_writer = Arc::new(tokio::sync::Mutex::new(
            create_writer(
                &hub_replica.directory.files(),
                &hub_replica.directory.keys(),
                SuccessorPlan::default(),
                &device_key(HUB_DEVICE),
                HUB_DEVICE,
                NOW_MS,
            )
            .expect("anchor writer"),
        ));
        let (pool, ctx) = (hub_replica.pool.clone(), hub_replica.ctx.clone());
        let sequence: HubProposals = Arc::new(move |proposal| {
            let (pool, ctx, writer) = (pool.clone(), ctx.clone(), Arc::clone(&anchor_writer));
            Box::pin(async move {
                let mut writer = writer.lock().await;
                let mut conn = acquire(&pool).await;
                let engine = Engine::vault();
                let outcome = authority::sequence(
                    &engine,
                    &mut conn,
                    &ctx,
                    &mut writer.local(),
                    &proposal,
                    NOW_MS,
                )
                .await?;
                let seq = engine
                    .stored_vector(&mut conn, &ctx)
                    .await
                    .map_err(|error| error.to_string())?
                    .get(&writer.id());
                writer.committed(seq)?;
                Ok(outcome)
            })
        });
        let changes = watch::Sender::new(0);
        let transport = DirectTransport(
            SyncHub::new(open_hub(&hub_replica), changes.clone())
                .with_contact(contact_source(test_contact()))
                .with_proposals(sequence),
        );

        let path = phone.directory.files().outbox_path();
        let mut outbox = Outbox::load(&path).expect("empty outbox");
        let duplicate = authority::propose(
            hub_replica.ctx.space,
            hub_replica.charter().await,
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: PHONE_DEVICE.to_string(),
                device_key: device_key(PHONE_DEVICE).public_key(),
                tls_fingerprint: [28; 32],
                label: PHONE_DEVICE.to_string(),
                device_kind: DeviceKind::Mobile,
            }),
            NOW_MS,
        )
        .expect("sign duplicate");
        outbox.add(enroll.clone()).expect("queue enrollment");
        outbox.add(duplicate.clone()).expect("queue duplicate");
        let sequenced = client::replay_outbox(&transport, &phone_ids(), &mut outbox, true)
            .await
            .expect("replay");
        assert_eq!(sequenced, 1);
        assert_eq!(*changes.borrow(), 1, "a sequenced proposal wakes waits");
        assert_eq!(
            outbox.entries()[1].rejection(),
            Some(ProposalRejection::AlreadyEnrolled)
        );
        // Replaying again is idempotent: the anchor answers the revision it already holds.
        assert_eq!(
            client::replay_outbox(&transport, &phone_ids(), &mut outbox, true)
                .await
                .expect("replay again"),
            1
        );
        assert_eq!(outbox.pending(), vec![enroll.clone()]);

        // Once the exchange brings the entry and the pass applies it, pruning drops the proposal.
        phone
            .exchange(&transport, &phone_ids())
            .await
            .expect("exchange");
        phone.apply().await;
        let mut conn = acquire(&phone.pool).await;
        outbox
            .prune(&phone.engine, &mut conn, &phone.ctx)
            .await
            .expect("prune");
        drop(conn);
        assert!(outbox.pending().is_empty());
        assert_eq!(
            Outbox::load(&path).expect("reload").entries(),
            outbox.entries()
        );
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
            Exchange::UnprovenFork(fork) => panic!("the fork went unproven: {fork:?}"),
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

        // The clone's offer gave the hub fork evidence, so it excludes both branches.
        hub_replica.apply().await;
        assert!(!hub_replica.has_note("note-original").await);
        assert!(hub_replica.has_note("note-clone").await);
        assert_eq!(clone.stored().await, hub_replica.stored().await);

        // The original learns the fork from the hub's state and re-seals its own branch.
        let fork = match original
            .exchange(&transport, &phone_ids())
            .await
            .expect("exchange")
        {
            Exchange::Fork(fork) => fork,
            Exchange::Done(done) => panic!("the reported fork went unnoticed: {done:?}"),
            Exchange::UnprovenFork(fork) => panic!("the fork went unproven: {fork:?}"),
        };
        assert_eq!(fork.writer, forked_writer);
        assert_eq!(fork.keep_through, shared);
        original.writer = create_writer(
            &original.directory.files(),
            &original.directory.keys(),
            SuccessorPlan::default(),
            &device_key(PHONE_DEVICE),
            PHONE_DEVICE,
            NOW_MS,
        )
        .expect("create successor");
        {
            let mut conn = acquire(&original.pool).await;
            let report = Engine::vault()
                .reseal_fork(
                    &mut conn,
                    &original.ctx,
                    fork.writer,
                    fork.keep_through,
                    &mut original.writer.local(),
                    NOW_MS,
                )
                .await
                .expect("re-seal fork");
            let seq = report.seal.last_seq.expect("re-sealed operations");
            original.writer.committed(seq).expect("record commit");
        }
        let original_writers = BTreeSet::from([forked_writer, original.writer.id()]);
        let exchanged = done(
            original
                .exchange_with(
                    &transport,
                    &phone_ids(),
                    original.writer.id(),
                    &original_writers,
                    MAX_EXCHANGE_BYTES,
                )
                .await
                .expect("exchange after re-seal"),
        );
        assert_eq!(exchanged.problem, None);
        hub_replica.apply().await;
        assert!(hub_replica.has_note("note-original").await);
        assert!(hub_replica.has_note("note-clone").await);
        hub_replica.close().await;
        original.close().await;
        clone.close().await;
    });
}

#[test]
fn fork_evidence_travels_between_members_through_the_hub() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-evidence", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-evidence", PHONE_DEVICE).await;
        let tablet = Replica::new("tablet-evidence", TABLET_DEVICE).await;
        let (transport, changes) = hub(&hub_replica);
        let shared = phone.seal_note("note-shared").await;
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );

        // Two copies of the phone's writer seal different operations at the next sequence.
        let phone_path = phone.directory.path().join("vault.sqlite");
        drop(phone.pool);
        phone.registry.close_all().await.expect("close phone");
        let clone_directory = TestDirectory::new("phone-evidence-clone");
        let clone_path = clone_directory.path().join("vault.sqlite");
        copy_database(&phone_path, &clone_path);
        phone.pool = phone
            .registry
            .connect_path(&phone_path)
            .await
            .expect("reopen phone");
        let clone_registry = DatabasePoolRegistry::default();
        let clone_pool = clone_registry
            .connect_path(&clone_path)
            .await
            .expect("open clone");
        let forked_seq = phone.seal_note("note-a").await;
        let mut clone_conn = acquire(&clone_pool).await;
        add_note(&mut clone_conn, "note-b").await;
        Engine::vault()
            .seal(
                &mut clone_conn,
                &phone.ctx,
                &mut phone.writer.local(),
                NOW_MS,
            )
            .await
            .expect("seal clone");
        let writer = phone.writer.id();
        let first = Engine::vault()
            .operation_at(
                &mut *acquire(&phone.pool).await,
                &phone.ctx,
                writer,
                forked_seq,
            )
            .await
            .expect("read operation")
            .expect("phone operation");
        let second = Engine::vault()
            .operation_at(&mut clone_conn, &phone.ctx, writer, forked_seq)
            .await
            .expect("read operation")
            .expect("clone operation");
        drop(clone_conn);
        clone_pool.close().await;
        clone_registry.close_all().await.expect("close clone");

        // The tablet holds evidence the hub lacks and offers it.
        let outcome = Engine::vault()
            .store_fork_evidence(
                &mut *acquire(&tablet.pool).await,
                &tablet.ctx,
                &first,
                &second,
                NOW_MS,
            )
            .await
            .expect("store evidence");
        assert_eq!(outcome, EvidenceOutcome::Stored);
        let before = *changes.borrow();
        let exchanged = done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(exchanged.problem, None);
        assert!(
            *changes.borrow() > before,
            "stored evidence wakes waiting peers"
        );
        let hub_forked = Engine::vault()
            .forked_writers(&mut *acquire(&hub_replica.pool).await, &hub_replica.ctx)
            .await
            .expect("hub forked writers");
        assert_eq!(hub_forked, vec![(writer, forked_seq)]);
        hub_replica.apply().await;

        // Another installation pulls the writer's prefix and then the evidence, in one exchange.
        let late = Replica::new("tablet-evidence-late", TABLET_DEVICE).await;
        let exchanged = done(
            late.exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(exchanged.problem, None);
        let mut conn = acquire(&late.pool).await;
        let evidence = Engine::vault()
            .fork_evidence(&mut conn, &late.ctx, writer)
            .await
            .expect("read evidence")
            .expect("forwarded evidence");
        assert_eq!(evidence.keep_through(), shared);
        drop(conn);
        // A second exchange asks for nothing it already holds.
        let exchanged = done(
            late.exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(exchanged.problem, None);

        // The phone that owns the writer learns its fork the same way.
        let fork = match phone
            .exchange(&transport, &phone_ids())
            .await
            .expect("exchange")
        {
            Exchange::Fork(fork) => fork,
            Exchange::Done(done) => panic!("the reported fork went unnoticed: {done:?}"),
            Exchange::UnprovenFork(fork) => panic!("the fork went unproven: {fork:?}"),
        };
        assert_eq!((fork.writer, fork.keep_through), (writer, shared));
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
        late.close().await;
    });
}

/// A hub, the phone that owns a forked writer, and a tablet, all holding one branch of that
/// writer through a sequence past the fork, with both operations at the fork in hand.
struct ForkedVault {
    hub: Replica,
    phone: Replica,
    tablet: Replica,
    transport: DirectTransport,
    writer: WriterId,
    fork_seq: u64,
    pair: [Vec<u8>; 2],
}

impl ForkedVault {
    async fn new(name: &str) -> Self {
        let mut hub_replica = Replica::new(&format!("hub-{name}"), HUB_DEVICE).await;
        let mut phone = Replica::new(&format!("phone-{name}"), PHONE_DEVICE).await;
        let mut tablet = Replica::new(&format!("tablet-{name}"), TABLET_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        phone.seal_note("note-shared").await;

        let phone_path = phone.directory.path().join("vault.sqlite");
        drop(phone.pool);
        phone.registry.close_all().await.expect("close phone");
        let clone_directory = TestDirectory::new(&format!("phone-{name}-clone"));
        let clone_path = clone_directory.path().join("vault.sqlite");
        copy_database(&phone_path, &clone_path);
        phone.pool = phone
            .registry
            .connect_path(&phone_path)
            .await
            .expect("reopen phone");
        let clone_registry = DatabasePoolRegistry::default();
        let clone_pool = clone_registry
            .connect_path(&clone_path)
            .await
            .expect("open clone");
        let fork_seq = phone.seal_note("note-a").await;
        let mut clone_conn = acquire(&clone_pool).await;
        add_note(&mut clone_conn, "note-b").await;
        Engine::vault()
            .seal(
                &mut clone_conn,
                &phone.ctx,
                &mut phone.writer.local(),
                NOW_MS,
            )
            .await
            .expect("seal clone");
        let writer = phone.writer.id();
        let first = Engine::vault()
            .operation_at(
                &mut *acquire(&phone.pool).await,
                &phone.ctx,
                writer,
                fork_seq,
            )
            .await
            .expect("read operation")
            .expect("phone operation");
        let second = Engine::vault()
            .operation_at(&mut clone_conn, &phone.ctx, writer, fork_seq)
            .await
            .expect("read operation")
            .expect("clone operation");
        drop(clone_conn);
        clone_pool.close().await;
        clone_registry.close_all().await.expect("close clone");

        phone.seal_note("note-c").await;
        done(
            phone
                .exchange(&transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        done(
            tablet
                .exchange(&transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        hub_replica.apply().await;
        phone.apply().await;
        tablet.apply().await;
        Self {
            hub: hub_replica,
            phone,
            tablet,
            transport,
            writer,
            fork_seq,
            pair: [first, second],
        }
    }

    /// Offers the pair to `replica`, as evidence arriving from a peer.
    async fn offer(&self, replica: &Replica) -> EvidenceOutcome {
        Engine::vault()
            .store_fork_evidence(
                &mut *acquire(&replica.pool).await,
                &replica.ctx,
                &self.pair[0],
                &self.pair[1],
                NOW_MS,
            )
            .await
            .expect("store evidence")
    }

    async fn forked(&self, replica: &Replica) -> Vec<(WriterId, u64)> {
        Engine::vault()
            .forked_writers(&mut *acquire(&replica.pool).await, &replica.ctx)
            .await
            .expect("forked writers")
    }

    async fn close(self) {
        self.hub.close().await;
        self.phone.close().await;
        self.tablet.close().await;
    }
}

#[test]
fn evidence_the_compacted_hub_refuses_sends_its_holder_to_a_fresh_copy() {
    block_on(async {
        let mut vault = ForkedVault::new("compacted-hub").await;
        assert!(vault.hub.compact().await > 0);
        assert_eq!(
            vault.offer(&vault.tablet).await,
            EvidenceOutcome::Stored,
            "the tablet still holds both bodies"
        );

        let offered = done(
            vault
                .tablet
                .exchange(&vault.transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(offered.problem, None);
        assert!(vault.forked(&vault.hub).await.is_empty());

        vault.tablet.apply().await;
        let cut = done(
            vault
                .tablet
                .exchange(&vault.transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert_eq!(cut.problem, None);
        assert!(cut.bootstrap_required);
        assert_eq!(cut.pulled, 0);
        vault.hub.apply().await;
        assert!(vault.hub.has_note("note-a").await);
        vault.close().await;
    });
}

#[test]
fn a_member_that_compacted_past_the_hubs_cut_takes_a_fresh_copy() {
    block_on(async {
        let mut vault = ForkedVault::new("compacted-member").await;
        assert!(vault.tablet.compact().await > 0);
        assert_eq!(vault.offer(&vault.hub).await, EvidenceOutcome::Stored);
        vault.hub.apply().await;

        let exchanged = done(
            vault
                .tablet
                .exchange(&vault.transport, &tablet_ids())
                .await
                .expect("exchange"),
        );
        assert!(exchanged.bootstrap_required);
        assert_eq!(exchanged.problem, None);
        assert!(vault.forked(&vault.tablet).await.is_empty());
        vault.tablet.apply().await;
        assert!(vault.tablet.has_note("note-a").await);
        vault.close().await;
    });
}

#[test]
fn a_writers_device_that_compacted_past_the_hubs_cut_takes_a_fresh_copy() {
    block_on(async {
        let mut vault = ForkedVault::new("compacted-own").await;
        assert!(vault.phone.compact().await > 0);
        assert_eq!(vault.offer(&vault.hub).await, EvidenceOutcome::Stored);
        vault.hub.apply().await;
        assert_eq!(
            vault.forked(&vault.hub).await,
            vec![(vault.writer, vault.fork_seq)]
        );

        let exchanged = done(
            vault
                .phone
                .exchange(&vault.transport, &phone_ids())
                .await
                .expect("exchange"),
        );
        assert!(exchanged.bootstrap_required);
        assert!(vault.forked(&vault.phone).await.is_empty());
        vault.close().await;
    });
}

/// The hub of a member listener on `replica`: it serves exchanges but sequences no proposal and
/// forwards none, and it reports the anchor's remembered contact.
fn member_hub(replica: &Replica) -> DirectTransport {
    DirectTransport(
        SyncHub::new(open_hub(replica), watch::Sender::new(0))
            .with_contact(contact_source(test_contact()))
            .with_relayed_geneses(),
    )
}

#[test]
fn a_member_hub_relays_the_genesis_of_another_member() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-relay", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-relay", PHONE_DEVICE).await;
        let mut tablet = Replica::new("tablet-relay", TABLET_DEVICE).await;
        let (anchor, _changes) = hub(&hub_replica);
        phone.seal_note("note-phone-relayed").await;
        done(
            phone
                .exchange(&anchor, &phone_ids())
                .await
                .expect("exchange"),
        );
        hub_replica.seal_note("note-hub-relayed").await;
        let hub_ids = PeerIds {
            vault_id: VAULT_ID.to_string(),
            device_id: HUB_DEVICE.to_string(),
        };

        // The coordinator's hub takes a genesis only from the device it certifies.
        let strict = DirectTransport(
            SyncHub::new(open_hub(&tablet), watch::Sender::new(0))
                .with_contact(contact_source(test_contact())),
        );
        let refused = done(
            hub_replica
                .exchange(&strict, &hub_ids)
                .await
                .expect("exchange with a strict hub"),
        );
        assert_eq!(refused.refusal, Some(SyncRefusalCode::UnknownWriter));
        assert!(!tablet.stored().await.dominates(&phone.stored().await));

        // A member hub relays the phone's genesis the hub pushes, so the tablet holds every
        // writer the hub holds after one exchange.
        let relayed = done(
            hub_replica
                .exchange(&member_hub(&tablet), &hub_ids)
                .await
                .expect("exchange with the tablet"),
        );
        assert_eq!(relayed.problem, None);
        assert_eq!(tablet.stored().await, hub_replica.stored().await);
        tablet.apply().await;
        assert!(tablet.has_note("note-phone-relayed").await);
        assert!(tablet.has_note("note-hub-relayed").await);
        hub_replica.close().await;
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn two_members_exchange_while_the_anchor_is_offline() {
    block_on(async {
        let hub_replica = Replica::new("hub-peers", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-peers", PHONE_DEVICE).await;
        let mut tablet = Replica::new("tablet-peers", TABLET_DEVICE).await;
        let (anchor, _changes) = hub(&hub_replica);
        for (replica, ids) in [(&phone, phone_ids()), (&tablet, tablet_ids())] {
            done(replica.exchange(&anchor, &ids).await.expect("exchange"));
        }
        let anchor_vector = hub_replica.stored().await;
        hub_replica.close().await;

        // The anchor is gone; the phone and the tablet reach each other's member listeners.
        phone.seal_note("note-phone-offline").await;
        tablet.seal_note("note-tablet-offline").await;
        let exchanged = done(
            phone
                .exchange(&member_hub(&tablet), &phone_ids())
                .await
                .expect("exchange with the tablet"),
        );
        assert_eq!(exchanged.problem, None);
        assert!(exchanged.pushed > 0 && exchanged.pulled > 0);
        assert_eq!(phone.stored().await, tablet.stored().await);
        assert!(phone.stored().await.dominates(&anchor_vector));
        phone.apply().await;
        tablet.apply().await;
        for replica in [&phone, &tablet] {
            assert!(replica.has_note("note-phone-offline").await);
            assert!(replica.has_note("note-tablet-offline").await);
        }
        phone.close().await;
        tablet.close().await;
    });
}

#[test]
fn a_new_installation_takes_the_anchor_chain_from_a_member_before_other_writers() {
    block_on(async {
        let mut hub_replica = Replica::new("hub-chain-first", HUB_DEVICE).await;
        let mut phone = Replica::new("phone-chain-first", PHONE_DEVICE).await;
        let (anchor, _changes) = hub(&hub_replica);
        hub_replica.seal_note("note-anchor").await;
        phone.seal_note("note-phone").await;
        done(
            phone
                .exchange(&anchor, &phone_ids())
                .await
                .expect("exchange"),
        );
        hub_replica.close().await;

        // A tablet installation that never met the anchor pulls everything from the phone in
        // one exchange: arrival order puts the anchor's chain before the geneses it enrolled,
        // so no writer is refused as unknown or unenrolled.
        let tablet = Replica::new("tablet-chain-first", TABLET_DEVICE).await;
        let exchanged = done(
            tablet
                .exchange(&member_hub(&phone), &tablet_ids())
                .await
                .expect("exchange with the phone"),
        );
        assert_eq!(exchanged.problem, None);
        assert_eq!(tablet.stored().await, phone.stored().await);
        phone.close().await;
        tablet.close().await;
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

async fn recovery_titles(replica: &Replica) -> Vec<(String, String)> {
    Engine::vault()
        .recovery_entries(&mut *acquire(&replica.pool).await, &replica.ctx)
        .await
        .expect("recovery entries")
        .into_iter()
        .map(|entry| (entry.row_key, entry.presentation.title))
        .collect()
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

/// Records the first seal of the dense benchmark Music library, sealed in batches until no
/// capture remains. No budget applies; the time is recorded with the sync budgets.
#[test]
#[ignore = "timings are meaningful only in the release profile"]
fn first_seal_of_the_dense_music_library_is_recorded() {
    block_on(async {
        let mut replica = Replica::new("dense-music-seal", HUB_DEVICE).await;
        let summary = ganbaru_music_library::fixtures::seed_dense_music_fixture(&replica.pool)
            .await
            .expect("seed dense music library");
        // The fixture's Calendar events join the migration-seeded calendar, which vault open
        // captures before the service starts.
        Engine::vault()
            .seed_captures(&mut *acquire(&replica.pool).await, NOW_MS)
            .await
            .expect("seed captures");
        let (mut batches, mut ops, mut changes) = (0_usize, 0_usize, 0_usize);
        let started = std::time::Instant::now();
        loop {
            let mut conn = acquire(&replica.pool).await;
            let report = Engine::vault()
                .seal(&mut conn, &replica.ctx, &mut replica.writer.local(), NOW_MS)
                .await
                .expect("seal");
            if let Some(seq) = report.last_seq {
                replica.writer.committed(seq).expect("record commit");
            }
            batches += 1;
            ops += report.sealed_ops;
            changes += report.changes;
            if !report.more {
                break;
            }
        }
        let elapsed = started.elapsed();
        println!(
            "first seal of the dense Music library ({} items): {elapsed:?} over {batches} \
             batches, {ops} operations, {changes} changes",
            summary.item_count
        );
        assert!(changes > summary.item_count);
        replica.close().await;
    });
}

#[test]
fn reset_sync_state_founds_a_new_space_that_republishes_every_row() {
    use crate::reset;
    use crate::writer::{SyncKey, SyncKeyStore, read_record};
    block_on(async {
        let mut hub = Replica::new("reset-sync-state", HUB_DEVICE).await;
        hub.seal_note("note-before-reset").await;
        let files = hub.directory.files();
        let keys = hub.directory.keys();
        let old_writer = hub.writer.record().writer_id.clone();
        std::fs::create_dir_all(files.vault_directory()).expect("vault sync directory");
        std::fs::write(files.outbox_path(), b"[]").expect("write outbox");
        std::fs::write(files.carry_path(), b"carry").expect("write carry bundle");
        let old_charter = hub
            .engine
            .charter(&mut *acquire(&hub.pool).await, &hub.ctx)
            .await
            .expect("read charter")
            .expect("charter");
        let old_enroll = authority::propose(
            hub.ctx.space,
            old_charter.hash(),
            &hub_keys().admin,
            authority::enroll_entry(Enrollment {
                device_id: "device-laptop".to_string(),
                device_key: device_key("device-laptop").public_key(),
                tls_fingerprint: [27; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            }),
            NOW_MS,
        )
        .expect("sign enrollment");
        let old_remove = authority::propose(
            hub.ctx.space,
            old_charter.hash(),
            &hub_keys().admin,
            authority::remove_entry(PHONE_DEVICE),
            NOW_MS,
        )
        .expect("sign removal");

        reset::reset_database(&mut *acquire(&hub.pool).await)
            .await
            .expect("reset database");
        reset::remove_installation_state(&files, &keys).expect("remove installation state");

        assert!(
            read_record(&files.record_path())
                .expect("read record")
                .is_none()
        );
        assert!(!files.outbox_path().exists() && !files.carry_path().exists());
        assert!(
            keys.read(VAULT_ID, SyncKey::Writer(&old_writer))
                .expect("read writer key")
                .is_none()
        );

        let mut conn = acquire(&hub.pool).await;
        hub.engine
            .init_space(&mut conn, VAULT_ID, NOW_MS + 1)
            .await
            .expect("initialize space");
        hub.engine
            .refuse_legacy_state(&mut conn, &hub.ctx)
            .await
            .expect("the reset space is current");
        hub.engine
            .seed_captures(&mut conn, NOW_MS + 1)
            .await
            .expect("seed captures");
        let founding = Founding {
            device_id: HUB_DEVICE,
            tls_fingerprint: [23; 32],
            discovery_key: [24; 32],
            person: &person(),
            now_ms: NOW_MS + 1,
        };
        let charter = ensure_charter(&hub.engine, &mut conn, &hub.ctx, &hub_keys(), &founding)
            .await
            .expect("found the space again");
        assert_ne!(
            charter, old_charter,
            "phones pinned the old charter and pair again"
        );
        let view = hub
            .engine
            .authorization(&mut conn, &hub.ctx)
            .await
            .expect("read authorization")
            .expect("authorization view");
        assert_eq!(view.anchor_device_id, HUB_DEVICE);
        assert!(
            view.devices
                .iter()
                .all(|device| device.device_id == HUB_DEVICE),
            "devices the old space enrolled pair again"
        );
        drop(conn);

        hub.writer = create_writer(
            &files,
            &keys,
            SuccessorPlan::default(),
            &device_key(HUB_DEVICE),
            HUB_DEVICE,
            NOW_MS + 1,
        )
        .expect("create writer");
        hub.seal().await;
        let mut conn = acquire(&hub.pool).await;
        for replayed in [&old_enroll, &old_remove] {
            assert_eq!(
                authority::sequence(
                    &hub.engine,
                    &mut conn,
                    &hub.ctx,
                    &mut hub.writer.local(),
                    replayed,
                    NOW_MS + 2,
                )
                .await
                .expect("sequence"),
                Err(ProposalRejection::WrongSpace),
                "a proposal signed for the old charter does not replay into the new one"
            );
        }
        assert_eq!(
            local::pending_captures(&mut conn).await.expect("captures"),
            0
        );
        let published: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_rows WHERE row_key = ?")
            .bind("note-before-reset")
            .fetch_one(&mut *conn)
            .await
            .expect("read rows");
        assert_eq!(published, 1, "the note is published again");
        drop(conn);
        hub.close().await;
    });
}

#[test]
fn a_reset_interrupted_after_the_database_is_cleared_is_repaired_on_the_next_start() {
    use crate::reset;
    use crate::writer::{WriterCheck, check_writer, read_record, read_stored_writer};
    block_on(async {
        // The crash lands before the installation state goes, or after the outbox went but
        // before the record and the writer keys did.
        for outbox_removed in [false, true] {
            let label = format!("reset-interrupted-{outbox_removed}");
            let mut hub = Replica::new(&label, HUB_DEVICE).await;
            hub.seal_note("note-before-reset").await;
            let files = hub.directory.files();
            let keys = hub.directory.keys();
            let old_charter = hub.charter().await;
            {
                let mut conn = acquire(&hub.pool).await;
                authority::enroll(
                    &hub.engine,
                    &mut conn,
                    &hub.ctx,
                    &hub_keys().admin,
                    &mut hub.writer.local(),
                    Enrollment {
                        device_id: "laptop-before-reset".to_string(),
                        device_key: device_key("laptop-before-reset").public_key(),
                        tls_fingerprint: [27; 32],
                        label: "Laptop".to_string(),
                        device_kind: DeviceKind::Desktop,
                    },
                    NOW_MS,
                )
                .await
                .expect("sequence an enrollment before the reset");
            }
            let recorded = hub
                .writer
                .record()
                .sequenced
                .clone()
                .expect("the anchor recorded its high-water");
            assert_eq!(recorded.charter(), Some(old_charter));
            let mut outbox = Outbox::load(&files.outbox_path()).expect("empty outbox");
            outbox
                .add(
                    authority::propose(
                        hub.ctx.space,
                        old_charter,
                        &hub_keys().admin,
                        authority::remove_entry(PHONE_DEVICE),
                        NOW_MS,
                    )
                    .expect("sign removal"),
                )
                .expect("queue removal");

            // A database without a charter beside a record naming a high-water was restored from
            // before founding, so only the Reset's first step lets a new charter be founded.
            let record = || read_record(&files.record_path()).expect("read record");
            assert!(reset::refuse_refounding(record().as_ref()).is_err());
            reset::forget_high_water(&files).expect("forget the high-water");
            assert_eq!(record().and_then(|record| record.sequenced), None);
            reset::refuse_refounding(record().as_ref()).expect("the reset may found anew");
            reset::reset_database(&mut *acquire(&hub.pool).await)
                .await
                .expect("reset database");
            if outbox_removed {
                Outbox::remove(&files.outbox_path()).expect("remove outbox");
            }

            let mut conn = acquire(&hub.pool).await;
            hub.engine
                .init_space(&mut conn, VAULT_ID, NOW_MS + 1)
                .await
                .expect("initialize space");
            hub.engine
                .refuse_legacy_state(&mut conn, &hub.ctx)
                .await
                .expect("the reset space is current");
            hub.engine
                .seed_captures(&mut conn, NOW_MS + 1)
                .await
                .expect("seed captures");
            let founding = Founding {
                device_id: HUB_DEVICE,
                tls_fingerprint: [23; 32],
                discovery_key: [24; 32],
                person: &person(),
                now_ms: NOW_MS + 1,
            };
            ensure_charter(&hub.engine, &mut conn, &hub.ctx, &hub_keys(), &founding)
                .await
                .expect("found the space again");
            let stored = read_stored_writer(&files, &keys).expect("read stored writer");
            assert!(stored.is_some(), "the crash left the old record");
            let check = check_writer(
                &hub.engine,
                &mut conn,
                &hub.ctx,
                &files,
                &device_key(HUB_DEVICE).public_key(),
                stored,
            )
            .await
            .expect("check writer");
            let WriterCheck::Successor(plan) = check else {
                panic!("the cleared space lacks the recorded writer");
            };
            drop(conn);
            hub.writer = create_writer(
                &files,
                &keys,
                plan,
                &device_key(HUB_DEVICE),
                HUB_DEVICE,
                NOW_MS + 1,
            )
            .expect("create successor");
            assert_eq!(
                hub.writer.record().sequenced,
                None,
                "a high-water of the cleared log never reads as a restore"
            );
            let new_charter = hub.charter().await;
            assert_ne!(
                new_charter, old_charter,
                "the reset founded another charter"
            );
            // A record that still named the old charter is cleared against the new one.
            let mut kept = hub.writer.record().clone();
            kept.sequenced = Some(recorded.clone());
            crate::writer::write_record(&files.record_path(), &kept).expect("write record");
            let stored = read_stored_writer(&files, &keys).expect("read stored writer");
            let mut conn = acquire(&hub.pool).await;
            let WriterCheck::Ready(writer) = check_writer(
                &hub.engine,
                &mut conn,
                &hub.ctx,
                &files,
                &device_key(HUB_DEVICE).public_key(),
                stored,
            )
            .await
            .expect("check writer") else {
                panic!("the successor still matches its chain");
            };
            drop(conn);
            hub.writer = *writer;
            hub.writer
                .settle_sequenced(None)
                .expect("settle without a charter");
            assert!(
                hub.writer.record().sequenced.is_some(),
                "a space without a charter keeps the high-water"
            );
            hub.writer
                .settle_sequenced(Some(new_charter))
                .expect("settle against the new charter");
            assert_eq!(hub.writer.record().sequenced, None);
            let stored = std::fs::read_to_string(files.record_path()).expect("read record");
            assert!(
                !stored.contains("\"sequenced\""),
                "the cleared high-water is durable"
            );

            let mut outbox = Outbox::load(&files.outbox_path()).expect("load outbox");
            let mut conn = acquire(&hub.pool).await;
            let sequenced = authority::sequence_outbox(
                &hub.engine,
                &mut conn,
                &hub.ctx,
                &mut hub.writer.local(),
                &mut outbox,
                &Default::default(),
                &Default::default(),
                NOW_MS + 2,
            )
            .await
            .expect("sequence outbox");
            assert_eq!(sequenced, 0, "the old proposal does not replay");
            assert!(
                outbox
                    .entries()
                    .iter()
                    .all(|entry| entry.rejection() == Some(ProposalRejection::WrongSpace)),
                "a proposal for the old charter is refused for good"
            );
            drop(conn);

            hub.seal().await;
            let mut conn = acquire(&hub.pool).await;
            let published: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sync_rows WHERE row_key = ?")
                    .bind("note-before-reset")
                    .fetch_one(&mut *conn)
                    .await
                    .expect("read rows");
            assert_eq!(published, 1, "the successor publishes the note again");
            drop(conn);
            hub.close().await;
        }
    });
}

mod assets;
mod calendar_conflicts;
mod calendar_recovery;
mod contacts;
mod flood;
mod forged_forks;
mod handover;
mod held;
mod notes_conflicts;
mod notes_recovery;
mod projects_conflicts;
mod projects_recovery;
mod records;
mod recovery_offers;
mod relay;
