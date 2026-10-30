use super::*;
use crate::sync::host::HeadlessHost;
use crate::sync::hub::{HubContact, HubVault};
use crate::vault::handoff::pairing::CoordinatorContact;
use crate::vault::handoff::transport::tests::{LocalPair, TestDirectory};
use crate::vault::process_managers::ProcessManagers;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync::hooks::{ChangeSink, HookFuture, MaterializeHook};
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::quick_notes::NOTES_TABLE;
use ganbaru_sync::{Engine, Hooks};
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKeyPair, DeviceKind, RecoverySeed, SignedCharter,
};
use ganbaru_sync_replica::assets::ContentAddressedStore;
use ganbaru_sync_replica::authority::Enrollment;
use ganbaru_sync_replica::writer::{
    ActiveWriter, FileKeyStore, SuccessorPlan, SyncFiles, create_writer,
};
use sqlx::SqliteConnection;
use std::path::Path;
use std::sync::atomic::AtomicUsize;

const VAULT_ID: &str = "vault-1";
const COORDINATOR_DEVICE: &str = "device-desktop";
const PHONE_DEVICE: &str = "device-phone";
/// Topic the counting hook reports for each changed note.
const NOTE_TOPIC: &str = "test-notes";
/// Long enough for a pass over a local hub, short enough to fail a stuck test quickly.
const PASS_WAIT: Duration = Duration::from_secs(30);
/// How long a blocked pass is watched before the test lets it continue.
const BLOCKED_WAIT: Duration = Duration::from_millis(200);

/// Counts the notes it sees materialized and reports each one through the change sink.
struct CountingHook(Arc<AtomicUsize>);

impl MaterializeHook for CountingHook {
    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            for key in changed_keys {
                self.0.fetch_add(1, Ordering::SeqCst);
                changes.report(NOTE_TOPIC, key.clone());
            }
            Ok(())
        })
    }
}

async fn insert_identity(pool: &SqlitePool, person: &PersonKeyPair) {
    sqlx::query(
        "INSERT INTO contacts_local_identity
            (singleton, public_key, created_at)
         VALUES (1, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(person.public_key().to_text())
    .execute(pool)
    .await
    .expect("insert person identity");
}

async fn insert_note(pool: &SqlitePool, id: &str) {
    sqlx::query("INSERT INTO quick_notes (id, title, order_key) VALUES (?, 'Note', 'a0')")
        .bind(id)
        .execute(pool)
        .await
        .expect("insert quick note");
}

async fn has_note(pool: &SqlitePool, id: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM quick_notes WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("count notes")
        > 0
}

/// The anchor keys of [`COORDINATOR_DEVICE`].
/// Seed of the recovery kit the test anchor founds its space with.
const RECOVERY_SEED: [u8; 32] = [10; 32];

fn anchor_keys() -> AnchorKeys {
    AnchorKeys {
        device: DeviceKeyPair::from_seed(&[8; 32]).expect("anchor device key"),
        admin: AdminKeyPair::from_seed(&[9; 32]).expect("anchor admin key"),
        recovery: RecoverySeed::from_bytes(RECOVERY_SEED)
            .public_keys()
            .expect("recovery kit keys"),
    }
}

/// The founded space of a coordinator vault and the writer its anchor seals with.
struct HubSpace {
    ctx: SpaceContext,
    charter: SignedCharter,
    writer: ActiveWriter,
}

impl HubSpace {
    /// Enrolls the phone with `device_key` through the anchor's writer.
    async fn enroll_phone(&mut self, pool: &SqlitePool, device_key: DevicePublicKey) {
        self.enroll(
            pool,
            Enrollment {
                device_id: PHONE_DEVICE.to_string(),
                device_key,
                tls_fingerprint: [14; 32],
                label: "Phone".to_string(),
                device_kind: DeviceKind::Mobile,
            },
        )
        .await;
    }

    /// Enrolls the device `enrollment` names through the anchor's writer.
    async fn enroll(&mut self, pool: &SqlitePool, enrollment: Enrollment) {
        let mut conn = acquire(pool).await.expect("hub connection");
        authority::enroll(
            &Engine::vault(),
            &mut conn,
            &self.ctx,
            &anchor_keys().admin,
            &mut self.writer.local(),
            enrollment,
            now_ms().expect("clock"),
        )
        .await
        .expect("enroll the device");
    }
}

/// A coordinator vault at `directory`, founded with [`COORDINATOR_DEVICE`] as its anchor and
/// holding a note the anchor sealed.
async fn hub_vault(
    directory: &Path,
    registry: &DatabasePoolRegistry,
    person: &PersonKeyPair,
) -> (SqlitePool, HubSpace) {
    ganbaru_db::test_support::write_migrated_database(directory.join("ganbaru.sqlite"))
        .await
        .expect("write hub vault database");
    let pool = registry
        .connect_path(directory.join("ganbaru.sqlite"))
        .await
        .expect("migrate hub vault");
    ganbaru_projects::conversion::convert(&pool, directory)
        .await
        .expect("convert hub projects");
    ganbaru_calendar::conversion::convert(&pool)
        .await
        .expect("convert hub calendar");
    ganbaru_notes::sync_keys::convert(&pool)
        .await
        .expect("convert hub notes");
    ganbaru_chat::conversion::convert(&pool)
        .await
        .expect("convert hub chat");
    ganbaru_chat::attachment_files::convert(&pool, directory)
        .await
        .expect("convert hub chat attachments");
    insert_identity(&pool, person).await;
    let now = now_ms().expect("clock");
    let mut conn = acquire(&pool).await.expect("hub connection");
    let engine = Engine::vault();
    engine
        .init_space(&mut conn, VAULT_ID, now)
        .await
        .expect("initialize hub space");
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read hub space")
        .expect("hub space context");
    let anchor = anchor_keys();
    let founding = Founding {
        device_id: COORDINATOR_DEVICE,
        tls_fingerprint: [12; 32],
        discovery_key: [7; ganbaru_handoff::protocol::DISCOVERY_KEY_BYTES],
        person,
        now_ms: now,
    };
    let charter = ensure_charter(&engine, &mut conn, &ctx, &anchor, &founding)
        .await
        .expect("found hub space");
    let mut writer = create_writer(
        &SyncFiles::new(directory.join("sync"), VAULT_ID).expect("hub sync files"),
        &FileKeyStore::new(
            directory.join("keys"),
            crate::sync::key_protection::test_wrapper(),
        ),
        SuccessorPlan::default(),
        &anchor.device,
        COORDINATOR_DEVICE,
        now,
    )
    .expect("create hub writer");
    drop(conn);
    insert_note(&pool, "note-hub").await;
    let mut conn = acquire(&pool).await.expect("hub connection");
    let seq = engine
        .seal(&mut conn, &ctx, &mut writer.local(), now)
        .await
        .expect("seal hub note")
        .last_seq
        .expect("sealed hub operations");
    writer.committed(seq).expect("record hub commit");
    (
        pool,
        HubSpace {
            ctx,
            charter,
            writer,
        },
    )
}

/// A hub serving `pool` over the coordinator listener as the device `device_id`, reporting a
/// LAN contact.
fn serving_hub(pool: SqlitePool, assets: PathBuf, device_id: &'static str) -> SyncHub {
    let assets: Arc<dyn AssetStore> = Arc::new(ContentAddressedStore::new(assets));
    let open: OpenHubVault = Arc::new(move || {
        let pool = pool.clone();
        let assets = Arc::clone(&assets);
        Box::pin(async move {
            Ok(Some(HubVault {
                pool,
                vault_id: VAULT_ID.to_string(),
                assets,
                quarantined: std::collections::BTreeSet::new(),
                device_id: device_id.to_string(),
            }))
        })
    });
    let contact = HubContact {
        members: vec![ganbaru_handoff::protocol::MemberEndpoint {
            device_id: "device-hub".to_string(),
            endpoint: "192.168.1.20:43821".to_string(),
            observed_ms: 1,
        }],
    };
    SyncHub::new(open, watch::Sender::new(0))
        .with_contact(Arc::new(move |_: &str| Ok(contact.clone())))
}

/// Makes the linked vault active on the phone with a note of its own and the hub's charter, as
/// the vault bundle delivers it, and stores the person key where the phone keeps it.
async fn phone_vault(
    config_dir: &Path,
    person: &PersonKeyPair,
    person_pkcs8: &[u8],
    charter: &SignedCharter,
) -> PathBuf {
    let database = crate::vault::activate_test_vault(
        config_dir,
        &config_dir.join("vault"),
        VAULT_ID,
        PHONE_DEVICE,
    )
    .expect("activate phone vault");
    ganbaru_db::test_support::write_migrated_database(&database)
        .await
        .expect("write phone vault database");
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path(&database)
        .await
        .expect("migrate phone vault");
    ganbaru_projects::conversion::convert(&pool, database.parent().expect("phone vault folder"))
        .await
        .expect("convert phone projects");
    ganbaru_calendar::conversion::convert(&pool)
        .await
        .expect("convert phone calendar");
    ganbaru_notes::sync_keys::convert(&pool)
        .await
        .expect("convert phone notes");
    ganbaru_chat::conversion::convert(&pool)
        .await
        .expect("convert phone chat");
    ganbaru_chat::attachment_files::convert(&pool, database.parent().expect("phone vault folder"))
        .await
        .expect("convert phone chat attachments");
    insert_identity(&pool, person).await;
    insert_note(&pool, "note-phone").await;
    let engine = Engine::vault();
    let mut conn = acquire(&pool).await.expect("phone connection");
    engine
        .init_space(&mut conn, VAULT_ID, now_ms().expect("clock"))
        .await
        .expect("initialize phone space");
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read phone space")
        .expect("phone space context");
    engine
        .install_charter(&mut conn, &ctx, charter)
        .await
        .expect("install the hub charter");
    drop(conn);
    drop(pool);
    registry.close_all().await.expect("close phone pools");
    let key_path = crate::contacts::identity::person_key_path_in(config_dir, VAULT_ID);
    crate::contacts::identity::write_key_file(
        crate::sync::key_protection::test_wrapper().as_ref(),
        &key_path,
        VAULT_ID,
        person_pkcs8,
    )
    .expect("store person key");
    database
}

/// A live runtime whose task finishes each requested pass, or ignores requests when `answer`
/// is false.
fn live_runtime(answer: bool) -> SyncRuntime {
    let runtime = SyncRuntime::default();
    let worker = runtime.clone();
    let stop = Arc::new(Notify::new());
    let stopped = Arc::clone(&stop);
    let handle = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                () = stopped.notified() => return,
                () = worker.now.notified() => {}
            }
            if answer {
                let number = worker.pass_started();
                worker.pass_done(number);
            }
        }
    });
    *runtime.task.lock().expect("task lock") = Some(ServiceTask { stop, handle });
    runtime
}

/// A host that must never be opened, for passes the live runtime runs.
async fn unopened() -> Result<HeadlessHost, String> {
    panic!("a delegated pass opens no host")
}

#[tokio::test]
async fn headless_pass_converges_with_the_hub_and_runs_hooks() {
    let (person_pkcs8, person) = PersonKeyPair::generate().expect("person key");
    let hub_directory = TestDirectory::new("headless-hub");
    let hub_registry = DatabasePoolRegistry::default();
    let (hub_pool, mut hub_space) = hub_vault(hub_directory.path(), &hub_registry, &person).await;
    let pair = LocalPair::start_with_hub_charter(
        Some(serving_hub(
            hub_pool.clone(),
            hub_directory.path().join("assets"),
            COORDINATOR_DEVICE,
        )),
        PinnedCharter {
            charter: ganbaru_handoff::protocol::encode_sync_charter(&hub_space.charter),
            person_key: person.public_key().to_text(),
        },
    )
    .await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir().to_path_buf();
    let phone_database = phone_vault(&config_dir, &person, &person_pkcs8, &hub_space.charter).await;

    let materialized = Arc::new(AtomicUsize::new(0));
    let open_host = || {
        let counter = Arc::clone(&materialized);
        HeadlessHost::open_with(&config_dir, &ProcessManagers::default())
            .expect("open headless host")
            .with_hooks(move || {
                Hooks::new().with(NOTES_TABLE, Box::new(CountingHook(Arc::clone(&counter))))
            })
    };
    let host = open_host();
    let phone_key = crate::sync::ensure_device_key(&host, VAULT_ID)
        .await
        .expect("phone device key");
    hub_space.enroll_phone(&hub_pool, phone_key).await;
    let live = Mutex::new(Weak::new());
    let gate = tokio::sync::Mutex::new(());
    // Without the enrollment a vault bundle carries, the first pass only pulls it and the hub's
    // operations; the phone seals nothing before it is enrolled.
    let priming = tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
    )
    .await
    .expect("the priming pass finishes")
    .expect("priming headless pass");
    assert_eq!(
        priming.status.state,
        SyncState::Idle,
        "{:?}",
        priming.status
    );
    let host = open_host();
    let pass = tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
    )
    .await
    .expect("the pass finishes")
    .expect("headless pass");

    assert!(!pass.delegated, "no runtime was live");
    assert_eq!(pass.status.state, SyncState::Idle, "{:?}", pass.status);
    let phone_registry = DatabasePoolRegistry::default();
    let phone_pool = phone_registry
        .connect_path_read_only(&phone_database)
        .await
        .expect("read phone vault");
    assert!(
        has_note(&phone_pool, "note-hub").await,
        "the pass pulled and applied the hub note"
    );
    let phone_writers: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_writers WHERE device_id = ?")
            .bind(PHONE_DEVICE)
            .fetch_one(&hub_pool)
            .await
            .expect("count phone writers");
    assert_eq!(phone_writers, 1, "the pass pushed the phone writer");
    let acknowledgements: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_ops WHERE kind = 3")
        .fetch_one(&phone_pool)
        .await
        .expect("count phone acknowledgements");
    assert_eq!(acknowledgements, 1, "the phone acknowledged the hub note");
    assert!(
        materialized.load(Ordering::SeqCst) > 0,
        "the registered hook ran on the materialized notes"
    );

    let host = HeadlessHost::open_with(&config_dir, &ProcessManagers::default())
        .expect("reopen headless host");
    let pass = tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
    )
    .await
    .expect("the follow-up pass finishes")
    .expect("follow-up headless pass");
    assert!(!pass.more, "pushing the acknowledgement settles the pass");
    let phone_acknowledgements: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sync_ops WHERE kind = 3")
            .fetch_one(&phone_pool)
            .await
            .expect("recount phone acknowledgements");
    assert_eq!(phone_acknowledgements, 1, "an idle pass seals nothing");
    let pushed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_ops WHERE kind = 3")
        .fetch_one(&hub_pool)
        .await
        .expect("count hub acknowledgements");
    assert_eq!(pushed, 1, "the hub holds the phone acknowledgement");
    drop(phone_pool);
    phone_registry
        .close_all()
        .await
        .expect("close phone reader");
    drop(hub_pool);
    hub_registry.close_all().await.expect("close hub pools");
}

/// Whether a file under `directory`, at any depth, has `name_part` in its name.
fn holds_file(directory: &Path, name_part: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            holds_file(&path, name_part)
        } else {
            entry.file_name().to_string_lossy().contains(name_part)
        }
    })
}

/// A minimal PNG header of 40 by 30 pixels, made unique by `seed`.
fn png(seed: u8) -> Vec<u8> {
    let mut bytes = vec![0; 34];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
    bytes[12..16].copy_from_slice(b"IHDR");
    bytes[16..20].copy_from_slice(&40u32.to_be_bytes());
    bytes[20..24].copy_from_slice(&30u32.to_be_bytes());
    bytes[24..29].copy_from_slice(&[8, 6, 0, 0, 0]);
    bytes[33] = seed;
    bytes.resize(64, seed);
    bytes
}

async fn count_rows(pool: &SqlitePool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("count rows")
}

#[tokio::test]
async fn a_member_gives_a_coordinator_its_log_removed_record_exchange_only() {
    use sha2::{Digest, Sha256};
    const LAPTOP_DEVICE: &str = "device-laptop";
    let (person_pkcs8, person) = PersonKeyPair::generate().expect("person key");
    let hub_directory = TestDirectory::new("headless-removed-hub");
    let hub_registry = DatabasePoolRegistry::default();
    let (hub_pool, mut hub_space) = hub_vault(hub_directory.path(), &hub_registry, &person).await;
    let stability = HubStability::default();
    let pair = LocalPair::start_with_hub_charter(
        Some(
            // Rooted at the vault folder as on a coordinator, so an upload would land, and
            // serving under another device id, so the hub never refuses on its own and only
            // the member's gate keeps the lanes closed.
            serving_hub(
                hub_pool.clone(),
                hub_directory.path().to_path_buf(),
                "device-unchecked",
            )
            .with_stability(stability.clone()),
        ),
        PinnedCharter {
            charter: ganbaru_handoff::protocol::encode_sync_charter(&hub_space.charter),
            person_key: person.public_key().to_text(),
        },
    )
    .await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir().to_path_buf();
    let phone_database = phone_vault(&config_dir, &person, &person_pkcs8, &hub_space.charter).await;
    let open_host = || {
        HeadlessHost::open_with(&config_dir, &ProcessManagers::default())
            .expect("open headless host")
    };
    let host = open_host();
    let phone_key = crate::sync::ensure_device_key(&host, VAULT_ID)
        .await
        .expect("phone device key");
    hub_space.enroll_phone(&hub_pool, phone_key).await;
    let laptop_key = DeviceKeyPair::from_seed(&[20; 32]).expect("laptop device key");
    hub_space
        .enroll(
            &hub_pool,
            Enrollment {
                device_id: LAPTOP_DEVICE.to_string(),
                device_key: laptop_key.public_key(),
                tls_fingerprint: [21; 32],
                label: "Laptop".to_string(),
                device_kind: DeviceKind::Desktop,
            },
        )
        .await;
    let live = Mutex::new(Weak::new());
    let gate = tokio::sync::Mutex::new(());
    for label in ["the priming pass", "the linked pass"] {
        let host = open_host();
        let pass = tokio::time::timeout(
            PASS_WAIT,
            headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
        )
        .await
        .unwrap_or_else(|_| panic!("{label} finishes"))
        .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!(pass.status.state, SyncState::Idle, "{:?}", pass.status);
    }
    let hub_ops = count_rows(&hub_pool, "SELECT COUNT(*) FROM sync_ops").await;
    assert!(hub_ops > 1, "the linked passes pushed the phone writer");

    // The laptop forces the anchor to itself with the recovery kit, which removes the old
    // anchor, and its records reach the phone before they reach the coordinator.
    let laptop_directory = TestDirectory::new("headless-removed-laptop");
    let phone_registry = DatabasePoolRegistry::default();
    let phone_pool = phone_registry
        .connect_path(&phone_database)
        .await
        .expect("open phone vault");
    let mut conn = acquire(&phone_pool).await.expect("phone connection");
    let engine = Engine::vault();
    let phone_ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read phone space")
        .expect("phone space context");
    let view = engine
        .authorization(&mut conn, &phone_ctx)
        .await
        .expect("read phone authorization")
        .expect("phone charter");
    let recovery =
        authority::recovery_admin(&view, &RecoverySeed::from_bytes(RECOVERY_SEED).kit_code())
            .expect("the kit recovers the vault");
    let mut laptop_writer = create_writer(
        &SyncFiles::new(laptop_directory.path().join("sync"), VAULT_ID).expect("laptop files"),
        &FileKeyStore::new(
            laptop_directory.path().join("keys"),
            crate::sync::key_protection::test_wrapper(),
        ),
        SuccessorPlan::default(),
        &laptop_key,
        LAPTOP_DEVICE,
        now_ms().expect("clock"),
    )
    .expect("create laptop writer");
    authority::force_handover_here(
        &engine,
        &mut conn,
        &phone_ctx,
        &authority::ForcedHandover {
            signer: &recovery,
            promote: None,
            point: None,
            removals: Vec::new(),
            recovery: None,
        },
        &mut laptop_writer.local(),
        now_ms().expect("clock"),
    )
    .await
    .expect("force handover")
    .expect("the kit hands the anchor to the laptop");
    let view = engine
        .authorization(&mut conn, &phone_ctx)
        .await
        .expect("read phone authorization")
        .expect("phone charter");
    assert!(
        ganbaru_sync_replica::hub::removes_device(&view, COORDINATOR_DEVICE),
        "the phone's selected log removed the coordinator"
    );
    let stable_before = engine
        .stable_vector(&mut conn, &phone_ctx)
        .await
        .expect("read phone stable vector");
    drop(conn);
    // An edit and an image the phone would push and upload to a coordinator it still follows.
    insert_note(&phone_pool, "note-after-removal").await;
    let image = png(5);
    let sha256: String = Sha256::digest(&image)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    ganbaru_quick_notes::images::import_image(
        phone_database.parent().expect("phone vault folder"),
        &image,
    )
    .expect("import image");
    sqlx::query(
        "INSERT INTO quick_note_images
            (note_id, sort_order, sha256, content_type, byte_size, width, height)
         VALUES ('note-after-removal', 0, ?, 'image/png', ?, 40, 30)",
    )
    .bind(&sha256)
    .bind(i64::try_from(image.len()).expect("image size"))
    .execute(&phone_pool)
    .await
    .expect("add image");
    drop(phone_pool);
    phone_registry.close_all().await.expect("close phone pools");
    let mut hub_stable = VersionVector::new();
    hub_stable.set(hub_space.writer.id(), 1);
    stability.publish(VAULT_ID, hub_stable);
    let hub_envelopes = count_rows(&hub_pool, "SELECT COUNT(*) FROM sync_auth_envelopes").await;

    let host = open_host();
    let pass = tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
    )
    .await
    .expect("the pass finishes")
    .expect("headless pass");

    assert_eq!(
        count_rows(&hub_pool, "SELECT COUNT(*) FROM sync_ops").await,
        hub_ops,
        "the phone pushed no operations to the removed coordinator"
    );
    assert!(
        !holds_file(&hub_directory.path().join("assets"), &sha256),
        "the phone uploaded no asset to the removed coordinator"
    );
    assert!(
        count_rows(&hub_pool, "SELECT COUNT(*) FROM sync_auth_envelopes").await > hub_envelopes,
        "record exchange still carried the removal to the coordinator"
    );
    let phone_registry = DatabasePoolRegistry::default();
    let phone_pool = phone_registry
        .connect_path_read_only(&phone_database)
        .await
        .expect("read phone vault");
    let phone_ops = |pool: SqlitePool| async move {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sync_ops JOIN sync_writers USING (space_id, writer_id)
             WHERE sync_writers.device_id = ?",
        )
        .bind(PHONE_DEVICE)
        .fetch_one(&pool)
        .await
        .expect("count phone operations")
    };
    let (sealed, pushed) = (
        phone_ops(phone_pool.clone()).await,
        phone_ops(hub_pool.clone()).await,
    );
    assert!(
        sealed > pushed,
        "the phone sealed operations ({sealed}) the removed coordinator never took ({pushed})"
    );
    // The stable vector comes only in the answer to the hello that opens the operation lanes,
    // so keeping the old one also shows the phone never offered the coordinator a push.
    let mut conn = acquire(&phone_pool).await.expect("phone connection");
    assert_eq!(
        engine
            .stable_vector(&mut conn, &phone_ctx)
            .await
            .expect("read phone stable vector"),
        stable_before,
        "the phone took no stable vector from the removed coordinator"
    );
    assert_eq!(
        pass.status.last_error_code,
        Some(SyncErrorCode::CoordinatorRemoved),
        "{:?}",
        pass.status
    );
    // The coordinator, having learned its removal, answers record exchange only.
    let removed_hub = serving_hub(
        hub_pool.clone(),
        hub_directory.path().to_path_buf(),
        COORDINATOR_DEVICE,
    );
    let answer = removed_hub
        .respond(ganbaru_handoff::protocol::ControlMessage::SyncHello {
            protocol_version: ganbaru_handoff::protocol::PROTOCOL_VERSION,
            vault_id: VAULT_ID.to_string(),
            device_id: PHONE_DEVICE.to_string(),
            manifest_version: engine.manifest().version,
            stored: Vec::new(),
            probe: None,
            listening: Vec::new(),
        })
        .await;
    assert!(
        matches!(
            &answer,
            ganbaru_handoff::protocol::ControlMessage::Error { code, .. }
                if code == ganbaru_sync_replica::hub::HUB_REMOVED
        ),
        "{answer:?}"
    );
    drop(conn);
    drop(phone_pool);
    phone_registry
        .close_all()
        .await
        .expect("close phone reader");
    drop(hub_pool);
    hub_registry.close_all().await.expect("close hub pools");
}

#[tokio::test]
async fn headless_pass_waits_for_the_pass_gate() {
    let config = TestDirectory::new("headless-gate");
    let host = HeadlessHost::open_with(config.path(), &ProcessManagers::default())
        .expect("open headless host");
    let live = Mutex::new(Weak::new());
    let gate = tokio::sync::Mutex::new(());
    let held = gate.lock().await;
    let pass = headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT);
    tokio::pin!(pass);
    assert!(
        tokio::time::timeout(BLOCKED_WAIT, &mut pass).await.is_err(),
        "the pass waits while another pass holds the gate"
    );
    drop(held);
    let pass = tokio::time::timeout(PASS_WAIT, pass)
        .await
        .expect("the pass runs once the gate is free")
        .expect("headless pass");
    assert!(!pass.delegated);
    assert_eq!(
        pass.status.state,
        SyncState::Error,
        "a device without an active vault reports why it cannot sync"
    );
}

#[tokio::test]
async fn the_headless_host_never_resurrects_a_pin_the_runtime_unlinked() {
    let pair = LocalPair::start_with_hub(None).await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir();
    crate::vault::activate_test_vault(
        config_dir,
        &config_dir.join("vault"),
        VAULT_ID,
        PHONE_DEVICE,
    )
    .expect("activate phone vault");
    // App startup loads the process managers before a headless pass opens its host on them.
    let managers = ProcessManagers::default();
    managers
        .load_pairing(config_dir, PHONE_DEVICE.to_string())
        .expect("load pairing");
    managers.load_ownership(config_dir).expect("load ownership");
    let host = HeadlessHost::open_with(config_dir, &managers).expect("open headless host");
    let pin = host
        .pairing()
        .coordinator_pin()
        .expect("read pin")
        .expect("the phone is enrolled");

    managers
        .pairing()
        .unlink()
        .expect("unlink through the runtime");

    assert!(
        host.pairing()
            .coordinator_pin()
            .expect("read pin")
            .is_none(),
        "the host sees the unlink"
    );
    let recorded = host
        .pairing()
        .record_coordinator_contact(
            &pin.vault_id,
            &pin.certificate_fingerprint,
            CoordinatorContact {
                last_endpoint: Some(pin.endpoint.clone()),
                ..Default::default()
            },
        )
        .expect("record coordinator contact");
    assert!(!recorded, "the host has no pin left to save");
    assert!(
        managers
            .load_pairing(config_dir, PHONE_DEVICE.to_string())
            .is_ok()
            && managers
                .pairing()
                .coordinator_pin()
                .expect("read pin")
                .is_none(),
        "a later load keeps the shared state"
    );
    let reloaded = PairingManager::default();
    reloaded
        .initialize(config_dir.to_path_buf(), PHONE_DEVICE.to_string())
        .expect("reload pairing");
    assert!(
        reloaded
            .coordinator_pin()
            .expect("read saved pin")
            .is_none(),
        "the saved state keeps the unlink"
    );
}

#[tokio::test]
async fn headless_pass_hands_its_work_to_the_live_runtime() {
    let runtime = live_runtime(true);
    let live = Mutex::new(Arc::downgrade(&runtime.0));
    let gate = tokio::sync::Mutex::new(());
    let held = gate.lock().await;
    let pass = tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(unopened(), &live, &gate, PASS_WAIT),
    )
    .await
    .expect("the live runtime answers")
    .expect("delegated pass");
    drop(held);
    assert!(pass.delegated);
    assert!(!pass.more);
    assert_eq!(
        runtime.passes_started.load(Ordering::SeqCst),
        1,
        "only the live runtime ran a pass"
    );
}

#[tokio::test]
async fn a_live_runtime_that_does_not_answer_fails_without_a_second_pass() {
    let runtime = live_runtime(false);
    let live = Mutex::new(Arc::downgrade(&runtime.0));
    let gate = tokio::sync::Mutex::new(());
    let result = headless_pass_with(unopened(), &live, &gate, BLOCKED_WAIT).await;
    assert!(result.is_err(), "a timed out delegation is an error");
    assert!(gate.try_lock().is_ok(), "no pass took the gate");
    assert_eq!(runtime.passes_started.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_stopped_live_runtime_fails_the_delegation() {
    let runtime = SyncRuntime::default();
    let live = Mutex::new(Arc::downgrade(&runtime.0));
    let gate = tokio::sync::Mutex::new(());
    let result = headless_pass_with(unopened(), &live, &gate, PASS_WAIT).await;
    assert!(
        result.is_err(),
        "a registered service without a task cannot run the pass"
    );
}

/// A phone enrolled with a local hub, its managers sealing with `wrapper`, and a headless host
/// on them.
async fn linked_phone_host(
    wrapper: Arc<ganbaru_handoff::key_wrap::TestKeyWrapper>,
) -> (LocalPair, ProcessManagers, HeadlessHost) {
    let pair = LocalPair::start_with_hub(None).await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir();
    crate::vault::activate_test_vault(
        config_dir,
        &config_dir.join("vault"),
        VAULT_ID,
        PHONE_DEVICE,
    )
    .expect("activate phone vault");
    let managers = ProcessManagers::with_key_wrapper(wrapper);
    managers
        .load_pairing(config_dir, PHONE_DEVICE.to_string())
        .expect("load pairing");
    managers.load_ownership(config_dir).expect("load ownership");
    let host = HeadlessHost::open_with(config_dir, &managers).expect("open headless host");
    assert!(host.pairing().has_links().expect("links"));
    (pair, managers, host)
}

/// Seals the key `reference` of `vault_id` into `directory` under the app config directory.
fn seal_vault_key(
    wrapper: &dyn ganbaru_handoff::key_wrap::KeyWrapper,
    config_dir: &Path,
    directory: &str,
    vault_id: &str,
    reference: &str,
) -> PathBuf {
    let path = config_dir
        .join(directory)
        .join(format!("{}.key", reference.replace(':', "-")));
    ganbaru_handoff::key_wrap::write_protected_file(
        wrapper,
        &ganbaru_handoff::key_wrap::vault_keys_alias(vault_id),
        reference,
        &path,
        b"private key",
    )
    .expect("seal key file");
    path
}

#[tokio::test]
async fn a_failed_pass_without_a_recorded_key_loss_keeps_the_phone_linked() {
    // A pass error can carry any text from the hub, including the lost-key message, so the
    // hook reads only the losses the key wrapper recorded and takes no error text at all.
    let wrapper = Arc::new(ganbaru_handoff::key_wrap::TestKeyWrapper::default());
    let (pair, _managers, host) = linked_phone_host(wrapper.clone()).await;
    let device_key = seal_vault_key(
        wrapper.as_ref(),
        pair.phone_config_dir(),
        "sync",
        VAULT_ID,
        &format!("device-key:{VAULT_ID}"),
    );
    recover_if_key_protection_lost(&host).await;
    assert!(host.pairing().has_links().expect("links"));
    assert!(device_key.exists());
}

#[tokio::test]
async fn a_recorded_vault_key_loss_unlinks_and_removes_only_that_vaults_keys() {
    use ganbaru_handoff::key_wrap::{
        KeyWrapper, is_key_protection_lost, read_protected_file, vault_keys_alias,
    };
    let wrapper = Arc::new(ganbaru_handoff::key_wrap::TestKeyWrapper::default());
    let (pair, managers, host) = linked_phone_host(wrapper.clone()).await;
    let config_dir = pair.phone_config_dir();
    let device_reference = format!("device-key:{VAULT_ID}");
    let lost_device = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        "sync",
        VAULT_ID,
        &device_reference,
    );
    let lost_writer = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        "sync",
        VAULT_ID,
        &format!("writer-key:{VAULT_ID}:0a1b"),
    );
    let lost_person = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        crate::contacts::identity::KEY_DIRECTORY,
        VAULT_ID,
        &format!("person-key:{VAULT_ID}"),
    );
    let other_vault = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        "sync",
        "vault-2",
        "device-key:vault-2",
    );
    let lost_alias = vault_keys_alias(VAULT_ID);
    wrapper.forget(&lost_alias).expect("lose the vault key");
    let read = || {
        read_protected_file(
            managers.key_wrapper().as_ref(),
            &lost_alias,
            &device_reference,
            &lost_device,
        )
        .expect_err("lost key")
    };
    assert!(is_key_protection_lost(&read()));

    wrapper.set_unavailable(true);
    recover_if_key_protection_lost(&host).await;
    assert!(
        host.pairing().has_links().expect("links"),
        "an unavailable key store is retried later"
    );
    assert!(lost_device.exists());
    wrapper.set_unavailable(false);

    read();
    recover_if_key_protection_lost(&host).await;
    assert!(!host.pairing().has_links().expect("links"));
    assert_eq!(
        host.pairing().unlink_reason().expect("reason"),
        Some(crate::vault::handoff::pairing::UnlinkReason::KeyProtectionLost)
    );
    assert!(!lost_device.exists() && !lost_writer.exists() && !lost_person.exists());
    assert!(other_vault.exists(), "another vault keeps its keys");
    assert!(wrapper.has_key(&vault_keys_alias("vault-2")));
}

#[tokio::test]
async fn a_replaced_linking_key_on_a_relinked_phone_keeps_it_linked_after_a_failed_pass() {
    use ganbaru_handoff::key_wrap::{linking_key_alias, open, seal};
    let wrapper = Arc::new(ganbaru_handoff::key_wrap::TestKeyWrapper::default());
    let (_pair, managers, host) = linked_phone_host(wrapper).await;
    let protection = managers.key_protection();
    let alias = linking_key_alias(PHONE_DEVICE);
    // The linking key was found lost while the phone was unlinked, then replaced and sealed with
    // a key the platform store generated again; the phone linked anew afterwards.
    open(protection.as_ref(), &alias, "linking-key", b"lost").expect_err("lost key");
    seal(protection.as_ref(), &alias, "linking-key", b"fresh key").expect("seal");
    recover_if_key_protection_lost(&host).await;
    assert!(host.pairing().has_links().expect("links"));
    assert_eq!(host.pairing().unlink_reason().expect("reason"), None);
}

#[tokio::test]
async fn a_lost_key_of_a_vault_without_links_removes_only_its_keys_and_keeps_the_phone_linked() {
    use ganbaru_handoff::key_wrap::{KeyWrapper, read_protected_file, vault_keys_alias};
    let wrapper = Arc::new(ganbaru_handoff::key_wrap::TestKeyWrapper::default());
    let (pair, managers, host) = linked_phone_host(wrapper.clone()).await;
    let config_dir = pair.phone_config_dir();
    let linked_key = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        "sync",
        VAULT_ID,
        &format!("device-key:{VAULT_ID}"),
    );
    let inactive_key = seal_vault_key(
        wrapper.as_ref(),
        config_dir,
        "sync",
        "vault-2",
        "device-key:vault-2",
    );
    wrapper
        .forget(&vault_keys_alias("vault-2"))
        .expect("lose the inactive vault key");
    read_protected_file(
        managers.key_wrapper().as_ref(),
        &vault_keys_alias("vault-2"),
        "device-key:vault-2",
        &inactive_key,
    )
    .expect_err("lost key");

    recover_if_key_protection_lost(&host).await;
    assert!(host.pairing().has_links().expect("links"));
    assert_eq!(host.pairing().unlink_reason().expect("reason"), None);
    assert!(!inactive_key.exists());
    assert!(linked_key.exists(), "the linked vault keeps its keys");
}

#[tokio::test]
async fn unsealed_keys_on_a_linked_phone_unlink_it_and_are_removed() {
    let pair = LocalPair::start_with_hub(None).await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir();
    let plaintext = config_dir
        .join("sync")
        .join(format!("device-key-{VAULT_ID}.key"));
    std::fs::create_dir_all(plaintext.parent().expect("parent")).expect("sync directory");
    std::fs::write(&plaintext, [0x30, 0x51, 0x02]).expect("plaintext key");
    let managers = ProcessManagers::with_key_wrapper(Arc::new(
        ganbaru_handoff::key_wrap::TestKeyWrapper::default(),
    ));
    managers
        .load_pairing(config_dir, PHONE_DEVICE.to_string())
        .expect("load pairing");
    assert!(!plaintext.exists());
    assert!(!managers.pairing().has_links().expect("links"));
    assert_eq!(
        managers.pairing().unlink_reason().expect("reason"),
        Some(crate::vault::handoff::pairing::UnlinkReason::KeyProtectionLost)
    );
}
