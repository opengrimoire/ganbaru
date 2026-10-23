use super::*;
use crate::sync::host::HeadlessHost;
use crate::sync::hub::{HubContact, HubVault};
use crate::vault::handoff::transport::tests::{LocalPair, TestDirectory};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync::hooks::{ChangeSink, HookFuture, MaterializeHook};
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::quick_notes::NOTES_TABLE;
use ganbaru_sync::{Engine, Hooks};
use ganbaru_sync_replica::assets::ContentAddressedStore;
use ganbaru_sync_replica::writer::{FileKeyStore, SuccessorPlan, SyncFiles, create_writer};
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
            (singleton, public_key, card_nonce, created_at, updated_at)
         VALUES (1, ?, 'AAAAAAAAAAAAAAAAAAAAAA', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
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

/// A coordinator vault at `directory` holding a note its own writer sealed.
async fn hub_vault(
    directory: &Path,
    registry: &DatabasePoolRegistry,
    person: &PersonKeyPair,
) -> SqlitePool {
    let pool = registry
        .connect_path(directory.join("ganbaru.sqlite"))
        .await
        .expect("migrate hub vault");
    insert_identity(&pool, person).await;
    let now = now_ms().expect("clock");
    let mut conn = acquire(&pool).await.expect("hub connection");
    let engine = Engine::vault();
    engine
        .init_space(&mut conn, VAULT_ID, now)
        .await
        .expect("initialize hub space");
    let ctx = local::space_context(&mut conn, VAULT_ID, COORDINATOR_DEVICE)
        .await
        .expect("read hub space")
        .expect("hub space context");
    let mut writer = create_writer(
        &SyncFiles::new(directory.join("sync"), VAULT_ID).expect("hub sync files"),
        &FileKeyStore {
            directory: directory.join("keys"),
        },
        SuccessorPlan::default(),
        person,
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
    pool
}

/// A hub serving `pool` over the coordinator listener, reporting a LAN contact.
fn serving_hub(pool: SqlitePool, assets: PathBuf) -> SyncHub {
    let assets: Arc<dyn AssetStore> = Arc::new(ContentAddressedStore::new(assets));
    let open: OpenHubVault = Arc::new(move || {
        let pool = pool.clone();
        let assets = Arc::clone(&assets);
        Box::pin(async move {
            Ok(Some(HubVault {
                pool,
                vault_id: VAULT_ID.to_string(),
                device_id: COORDINATOR_DEVICE.to_string(),
                assets,
            }))
        })
    });
    let contact = HubContact {
        endpoints: vec!["192.168.1.20:43821".to_string()],
        discovery_key: URL_SAFE_NO_PAD.encode([7; ganbaru_handoff::protocol::DISCOVERY_KEY_BYTES]),
    };
    SyncHub::new(open, watch::Sender::new(0))
        .with_contact(Arc::new(move |_: &str| Ok(contact.clone())))
}

/// Makes the linked vault active on the phone with a note of its own, and stores the person key
/// where the phone keeps it.
async fn phone_vault(config_dir: &Path, person: &PersonKeyPair, person_pkcs8: &[u8]) -> PathBuf {
    let database = crate::vault::activate_test_vault(
        config_dir,
        &config_dir.join("vault"),
        VAULT_ID,
        PHONE_DEVICE,
    )
    .expect("activate phone vault");
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path(&database)
        .await
        .expect("migrate phone vault");
    insert_identity(&pool, person).await;
    insert_note(&pool, "note-phone").await;
    drop(pool);
    registry.close_all().await.expect("close phone pools");
    let key_path = crate::contacts::identity::person_key_path_in(config_dir, VAULT_ID);
    std::fs::create_dir_all(key_path.parent().expect("key directory"))
        .expect("create key directory");
    std::fs::write(&key_path, person_pkcs8).expect("store person key");
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
    let hub_pool = hub_vault(hub_directory.path(), &hub_registry, &person).await;
    let pair = LocalPair::start_with_hub(Some(serving_hub(
        hub_pool.clone(),
        hub_directory.path().join("assets"),
    )))
    .await;
    pair.enroll().await;
    let config_dir = pair.phone_config_dir().to_path_buf();
    let phone_database = phone_vault(&config_dir, &person, &person_pkcs8).await;

    let materialized = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&materialized);
    let host = HeadlessHost::open(&config_dir)
        .expect("open headless host")
        .with_hooks(move || {
            Hooks::new().with(NOTES_TABLE, Box::new(CountingHook(Arc::clone(&counter))))
        });
    let live = Mutex::new(Weak::new());
    let gate = tokio::sync::Mutex::new(());
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
    assert!(pass.more, "the next pass pushes the acknowledgement");
    assert!(
        materialized.load(Ordering::SeqCst) > 0,
        "the registered hook ran on the materialized notes"
    );
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
    let host = HeadlessHost::open(config.path()).expect("open headless host");
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
