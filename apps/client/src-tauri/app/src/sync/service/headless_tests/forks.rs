//! Fork claims of the coordinator about the phone's writer: only signed proof retires it.

use super::{
    COORDINATOR_DEVICE, HubSpace, PASS_WAIT, PHONE_DEVICE, VAULT_ID, count_rows, hub_vault,
    insert_note, phone_vault, serving_hub,
};
use crate::sync::host::{HeadlessHost, SyncHost};
use crate::sync::status::{SyncErrorCode, SyncState};
use crate::vault::handoff::pairing::PinnedCharter;
use crate::vault::handoff::transport::tests::{LocalPair, TestDirectory};
use crate::vault::process_managers::ProcessManagers;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::{Engine, SpaceContext, local};
use ganbaru_sync_contracts::Seq;
use ganbaru_sync_replica::now_ms;
use ganbaru_sync_replica::service::{
    HeadlessPass, ensure_device_key, headless_pass_with, sync_files,
};
use ganbaru_sync_replica::writer::{
    ActiveWriter, WriterCheck, check_writer, read_record, read_stored_writer,
};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Weak};

/// A phone linked to a coordinator serving `hub_pool`, after the passes that pushed its writer.
struct LinkedPhone {
    _hub_directory: TestDirectory,
    hub_registry: DatabasePoolRegistry,
    hub_pool: SqlitePool,
    hub_space: HubSpace,
    _pair: LocalPair,
    config_dir: PathBuf,
    phone_database: PathBuf,
}

async fn linked_phone(label: &str) -> LinkedPhone {
    let (person_pkcs8, person) = PersonKeyPair::generate().expect("person key");
    let hub_directory = TestDirectory::new(label);
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
    let phone_key = ensure_device_key(&open_host(&config_dir), VAULT_ID)
        .await
        .expect("phone device key");
    hub_space.enroll_phone(&hub_pool, phone_key).await;
    for label in ["the priming pass", "the linked pass"] {
        let pass = run_pass(&config_dir, label).await;
        assert_eq!(pass.status.state, SyncState::Idle, "{:?}", pass.status);
    }
    LinkedPhone {
        _hub_directory: hub_directory,
        hub_registry,
        hub_pool,
        hub_space,
        _pair: pair,
        config_dir,
        phone_database,
    }
}

fn open_host(config_dir: &Path) -> HeadlessHost {
    HeadlessHost::open_with(config_dir, &ProcessManagers::default()).expect("open headless host")
}

async fn run_pass(config_dir: &Path, label: &str) -> HeadlessPass {
    try_pass(config_dir, label)
        .await
        .unwrap_or_else(|error| panic!("{label}: {error}"))
}

async fn try_pass(config_dir: &Path, label: &str) -> Result<HeadlessPass, String> {
    let live = Mutex::new(Weak::new());
    let gate = tokio::sync::Mutex::new(());
    let host = open_host(config_dir);
    tokio::time::timeout(
        PASS_WAIT,
        headless_pass_with(async { Ok(host) }, &live, &gate, PASS_WAIT),
    )
    .await
    .unwrap_or_else(|_| panic!("{label} finishes"))
}

/// The writer id the phone's writer record names.
fn recorded_writer(config_dir: &Path) -> String {
    let files = sync_files(&open_host(config_dir), VAULT_ID).expect("phone sync files");
    read_record(&files.record_path())
        .expect("read writer record")
        .expect("writer record")
        .writer_id
}

async fn phone_writer_rows(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sync_writers WHERE device_id = ?")
        .bind(PHONE_DEVICE)
        .fetch_one(pool)
        .await
        .expect("count phone writers")
}

impl LinkedPhone {
    async fn open_phone(&self) -> (DatabasePoolRegistry, SqlitePool) {
        let registry = DatabasePoolRegistry::default();
        let pool = registry
            .connect_path(&self.phone_database)
            .await
            .expect("open phone vault");
        (registry, pool)
    }

    async fn close(self) {
        drop(self.hub_pool);
        self.hub_registry
            .close_all()
            .await
            .expect("close hub pools");
    }
}

/// The phone's active writer, as its next pass loads it.
async fn phone_writer(config_dir: &Path, pool: &SqlitePool, ctx: &SpaceContext) -> ActiveWriter {
    let host = open_host(config_dir);
    let files = sync_files(&host, VAULT_ID).expect("phone sync files");
    let keys = host.key_store().expect("phone key store");
    let device_key = ensure_device_key(&host, VAULT_ID)
        .await
        .expect("phone device key");
    let stored = read_stored_writer(&files, keys.as_ref()).expect("read phone writer");
    let mut conn = pool.acquire().await.expect("phone connection");
    match check_writer(
        &Engine::vault(),
        &mut conn,
        ctx,
        &files,
        &device_key,
        stored,
    )
    .await
    .expect("check phone writer")
    {
        WriterCheck::Ready(writer) => *writer,
        WriterCheck::Successor(_) => panic!("the phone writer is in place"),
    }
}

/// Inserts a note into `pool` and seals it with `writer`, returning the sealed sequence.
async fn seal_note(
    pool: &SqlitePool,
    ctx: &SpaceContext,
    writer: &mut ActiveWriter,
    id: &str,
) -> Seq {
    insert_note(pool, id).await;
    let mut conn = pool.acquire().await.expect("connection");
    Engine::vault()
        .seal(
            &mut conn,
            ctx,
            &mut writer.local(),
            now_ms().expect("clock"),
        )
        .await
        .expect("seal note")
        .last_seq
        .expect("sealed operations")
}

#[tokio::test]
async fn unproven_fork_claim_keeps_the_writer_and_reports_it() {
    let linked = linked_phone("headless-unproven-fork").await;
    let writer = recorded_writer(&linked.config_dir);
    // The coordinator claims one more operation of the phone's writer than it sealed, and
    // holds no operation there.
    let raised =
        sqlx::query("UPDATE sync_writers SET stored_seq = stored_seq + 1 WHERE device_id = ?")
            .bind(PHONE_DEVICE)
            .execute(&linked.hub_pool)
            .await
            .expect("raise the claimed head")
            .rows_affected();
    assert_eq!(raised, 1);

    for label in ["the claimed pass", "the next pass"] {
        let pass = run_pass(&linked.config_dir, label).await;
        assert_eq!(recorded_writer(&linked.config_dir), writer, "{label}");
        assert_eq!(
            pass.status.state,
            SyncState::Error,
            "{label}: {:?}",
            pass.status
        );
        assert_eq!(
            pass.status.last_error_code,
            Some(SyncErrorCode::ForkClaimUnproven),
            "{label}: {:?}",
            pass.status
        );
    }
    let (registry, pool) = linked.open_phone().await;
    assert_eq!(
        phone_writer_rows(&pool).await,
        1,
        "no successor writer was sealed"
    );
    drop(pool);
    registry.close_all().await.expect("close phone pools");
    linked.close().await;
}

#[tokio::test]
async fn refused_reseal_consumes_no_writer() {
    let linked = linked_phone("headless-refused-reseal").await;
    let writer_id = recorded_writer(&linked.config_dir);
    let (registry, pool) = linked.open_phone().await;
    let ctx = local::space_context(
        &mut pool.acquire().await.expect("phone connection"),
        VAULT_ID,
    )
    .await
    .expect("read phone space")
    .expect("phone space context");
    let mut writer = phone_writer(&linked.config_dir, &pool, &ctx).await;

    // A copy of the phone continues the same writer with another note, and the coordinator
    // stores that branch before the phone's own.
    let clone_directory = TestDirectory::new("headless-refused-reseal-clone");
    let clone_path = clone_directory.path().join("clone.sqlite");
    sqlx::query("VACUUM INTO ?")
        .bind(clone_path.to_string_lossy().into_owned())
        .execute(&pool)
        .await
        .expect("copy the phone vault");
    let clone_registry = DatabasePoolRegistry::default();
    let clone_pool = clone_registry
        .connect_path(&clone_path)
        .await
        .expect("open the copy");
    let fork_seq = seal_note(&clone_pool, &ctx, &mut writer, "note-clone-branch").await;
    assert!(
        fork_seq >= 3,
        "the shared prefix holds operations past genesis"
    );
    let clone_op = Engine::vault()
        .operation_at(
            &mut clone_pool.acquire().await.expect("copy connection"),
            &ctx,
            writer.id(),
            fork_seq,
        )
        .await
        .expect("read the copy's operation")
        .expect("the copy's operation");
    drop(clone_pool);
    clone_registry.close_all().await.expect("close the copy");
    Engine::vault()
        .store(
            &mut linked.hub_pool.acquire().await.expect("hub connection"),
            &linked.hub_space.ctx,
            &clone_op,
            now_ms().expect("clock"),
        )
        .await
        .expect("store the copy's branch at the coordinator");

    let phone_seq = seal_note(&pool, &ctx, &mut writer, "note-phone-branch").await;
    assert_eq!(phone_seq, fork_seq, "both copies sealed the same sequence");
    writer
        .committed(phone_seq)
        .expect("record the phone's commit");
    // The phone's copy of the shared prefix reads as not applied, so a re-seal through the
    // proven fork point is refused.
    sqlx::query("UPDATE sync_writers SET applied_seq = ? WHERE writer_id = ?")
        .bind(i64::try_from(fork_seq - 2).expect("sequence"))
        .bind(writer.id().as_bytes().as_slice())
        .execute(&pool)
        .await
        .expect("lower the applied head");
    let writers = phone_writer_rows(&pool).await;
    drop(pool);
    registry.close_all().await.expect("close phone pools");

    let refused = try_pass(&linked.config_dir, "the forked pass")
        .await
        .map(|pass| pass.status);
    let (registry, pool) = linked.open_phone().await;
    assert!(
        count_rows(&pool, "SELECT COUNT(*) FROM sync_fork_evidence").await > 0,
        "the phone holds proof of the fork: {refused:?}"
    );
    assert_eq!(
        recorded_writer(&linked.config_dir),
        writer_id,
        "a refused re-seal keeps the writer: {refused:?}"
    );
    assert_eq!(phone_writer_rows(&pool).await, writers);
    drop(pool);
    registry.close_all().await.expect("close phone pools");
    linked.close().await;
}
