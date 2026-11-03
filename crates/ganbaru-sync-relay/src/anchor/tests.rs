use super::state::DataPaths;
use crate::config::{AnchorBackupConfig, AnchorSetupConfig};
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::dial::LoopbackPolicy;
use ganbaru_handoff::pairing::PinnedCharter;
use ganbaru_handoff::protocol::anchor::AnchorSetupCode;
use ganbaru_handoff::protocol::{
    self, BundleMetadata, HandoffCompatibility, encode_sync_charter, sha256_file,
};
use ganbaru_sync::{
    AuthorizationView, Engine, LocalWriter, SequenceReservation, SpaceContext, local,
};
use ganbaru_sync_contracts::authorization::{Enroll, Entry};
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKeyPair, DeviceKind, DeviceRole, RecoverySeed, SignedCertificate,
    SignedCharter, WriterCertificate, WriterKeyPair, sign_certificate, sign_charter,
};
use ganbaru_sync_replica::anchor_link::{dial_server, local_compatibility, request_on};
use ganbaru_sync_replica::anchor_setup::{AnchorSetupError, open_setup, setup_target};
use ganbaru_sync_replica::assets::STALE_STAGING_AGE;
use ganbaru_sync_replica::authority::{
    self, AnchorKeys, Enrollment, Founding, charter_hash, ensure_charter,
};
use ganbaru_sync_replica::backup::{ArchiveGate, DATABASE_FILE, create_snapshot_archive};
use ganbaru_sync_replica::serve::PreAuthPolicy;
use ganbaru_sync_replica::serve::limits::ConnectionLimits;
use sqlx::SqlitePool;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

const VAULT_ID: &str = "vault-anchor-test";
const FOUNDER: &str = "device-founder";
const NOW_MS: u64 = 1_800_000_000_000;
const APP_VERSION: &str = "1.2.3";
const WAIT: Duration = Duration::from_secs(30);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = ganbaru_atomic_files::create_private_temporary_directory(
            &std::env::temp_dir(),
            "ganbaru-anchor-test-",
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

fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&[7; 32]).unwrap()
}

fn recovery_seed() -> RecoverySeed {
    RecoverySeed::from_bytes([22; 32])
}

struct Unreserved;

impl SequenceReservation for Unreserved {
    fn ensure_reserved(&mut self, _seq: u64) -> std::io::Result<()> {
        Ok(())
    }
}

/// A founding desktop whose vault the anchor receives.
struct Founder {
    directory: TempDir,
    pool: SqlitePool,
    ctx: SpaceContext,
    charter: SignedCharter,
    device: DeviceKeyPair,
    admin: AdminKeyPair,
    writer: WriterKeyPair,
    certificate: SignedCertificate,
}

impl Founder {
    async fn new() -> Self {
        let directory = TempDir::new();
        std::fs::write(
            directory.path().join("vault.json"),
            format!(
                "{{\"app\":\"ganbaru\",\"schemaVersion\":1,\"vaultId\":\"{VAULT_ID}\",\"displayName\":\"Test\",\"createdAt\":\"2026-01-01T00:00:00Z\"}}"
            ),
        )
        .unwrap();
        let path = directory.path().join(DATABASE_FILE);
        ganbaru_db::test_support::write_migrated_database(&path)
            .await
            .unwrap();
        let registry = DatabasePoolRegistry::default();
        let pool = registry.connect_path(&path).await.unwrap();
        sqlx::query(
            "INSERT INTO contacts_local_identity (singleton, public_key, created_at)
             VALUES (1, ?, '2026-01-01T00:00:00Z')",
        )
        .bind(person().public_key().to_text())
        .execute(&pool)
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        let engine = Engine::vault();
        engine
            .init_space(&mut conn, VAULT_ID, NOW_MS)
            .await
            .unwrap();
        let ctx = local::space_context(&mut conn, VAULT_ID)
            .await
            .unwrap()
            .unwrap();
        let device = DeviceKeyPair::from_seed(&[20; 32]).unwrap();
        let admin = AdminKeyPair::from_seed(&[21; 32]).unwrap();
        let keys = AnchorKeys {
            device: DeviceKeyPair::from_seed(&[20; 32]).unwrap(),
            admin: AdminKeyPair::from_seed(&[21; 32]).unwrap(),
            recovery: recovery_seed().public_keys().unwrap(),
        };
        let founding = Founding {
            device_id: FOUNDER,
            tls_fingerprint: [23; 32],
            discovery_key: [24; 32],
            person: &person(),
            now_ms: NOW_MS,
        };
        let charter = ensure_charter(&engine, &mut conn, &ctx, &keys, &founding)
            .await
            .unwrap();
        let writer = WriterKeyPair::from_seed(&[25; 32]).unwrap();
        let certificate = sign_certificate(
            &WriterCertificate {
                writer_key: writer.public_key(),
                device_id: FOUNDER.to_string(),
                device_key: device.public_key(),
                created_at_ms: i64::try_from(NOW_MS).unwrap(),
                predecessor: None,
            },
            &device,
        )
        .unwrap();
        drop(conn);
        Self {
            directory,
            pool,
            ctx,
            charter,
            device,
            admin,
            writer,
            certificate,
        }
    }

    /// Sequences `entry` as the founding anchor.
    async fn sequence(&self, entry: Entry) {
        let engine = Engine::vault();
        let mut conn = self.pool.acquire().await.unwrap();
        let charter = charter_hash(&engine, &mut conn, &self.ctx).await.unwrap();
        let proposal =
            authority::propose(self.ctx.space, charter, &self.admin, entry, NOW_MS).unwrap();
        let mut reservation = Unreserved;
        let mut writer = LocalWriter {
            key: &self.writer,
            certificate: &self.certificate,
            reservation: &mut reservation,
            keys: None,
        };
        authority::sequence(
            &engine,
            &mut conn,
            &self.ctx,
            &mut writer,
            &proposal,
            NOW_MS,
        )
        .await
        .unwrap()
        .unwrap();
    }

    fn pinned(&self) -> PinnedCharter {
        PinnedCharter {
            charter: encode_sync_charter(&self.charter),
            person_key: person().public_key().to_text(),
        }
    }

    /// The vault as a snapshot archive with its metadata.
    async fn snapshot(&self) -> (BundleMetadata, PathBuf) {
        let database = self.directory.path().join("snapshot.sqlite");
        let archive = self.directory.path().join("snapshot.zip");
        create_snapshot_archive(
            &ArchiveGate::process(),
            self.directory.path(),
            &database,
            &archive,
        )
        .await
        .unwrap();
        let compatibility: HandoffCompatibility = local_compatibility(APP_VERSION);
        let metadata = BundleMetadata {
            protocol_version: protocol::PROTOCOL_VERSION,
            compatibility,
            vault_id: VAULT_ID.to_string(),
            device_id: FOUNDER.to_string(),
            transfer_id: "transfer-anchor-test".to_string(),
            archive_bytes: archive.metadata().unwrap().len(),
            archive_sha256: sha256_file(&archive).unwrap(),
            derived_state_stale: false,
        };
        (metadata, archive)
    }
}

/// Runs `scenario` against an anchor listening on loopback, which stops with it.
async fn with_anchor<F, T>(data_dir: &Path, scenario: impl FnOnce(String) -> F) -> T
where
    F: Future<Output = T>,
{
    with_anchor_budget(
        data_dir,
        crate::limits::anchor_connection_limits(),
        crate::limits::anchor_pre_auth_policy(),
        scenario,
    )
    .await
}

/// Runs `scenario` against an anchor with the given connection budget.
async fn with_anchor_budget<F, T>(
    data_dir: &Path,
    limits: ConnectionLimits,
    policy: PreAuthPolicy,
    scenario: impl FnOnce(String) -> F,
) -> T
where
    F: Future<Output = T>,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (_stop, shutdown) = oneshot::channel();
    let server = super::serve_with(listener, data_dir, shutdown, limits, policy);
    let scenario = scenario(endpoint);
    tokio::pin!(server);
    tokio::pin!(scenario);
    tokio::select! {
        result = &mut server => panic!("the anchor stopped: {result:?}"),
        value = &mut scenario => value,
    }
}

/// A fresh setup code printed by `anchor setup`.
fn printed_code(data_dir: &Path, endpoint: &str) -> AnchorSetupCode {
    let text = super::setup(&AnchorSetupConfig {
        data_dir: data_dir.to_path_buf(),
        endpoints: vec![endpoint.to_string()],
    })
    .unwrap();
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("Setup code: "))
        .unwrap();
    AnchorSetupCode::from_text(line).unwrap()
}

fn is_refused(error: &AnchorSetupError) -> bool {
    matches!(error, AnchorSetupError::Refused)
}

/// Enrolls the server the code leads to and uploads the founder's vault.
async fn enroll_and_upload(founder: &Founder, code: &AnchorSetupCode) -> String {
    let server = enroll_server(founder, code).await;
    upload(founder, code).await.unwrap();
    server
}

/// Enrolls the server the code leads to as an admin of the founder's log.
async fn enroll_server(founder: &Founder, code: &AnchorSetupCode) -> String {
    let identity = open_setup(code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
        .await
        .unwrap()
        .identity()
        .clone();
    let server = identity.enrollment;
    assert!(identity.space.is_none());
    founder
        .sequence(Entry::Enroll(Enroll {
            device_id: server.device_id.clone(),
            device_key: server.device_key,
            tls_fingerprint: server.server.certificate_fingerprint,
            label: server.label.clone(),
            device_kind: DeviceKind::Server,
            role: DeviceRole::Admin(server.admin_key),
        }))
        .await;
    server.device_id
}

/// Uploads the founder's vault to the server the code leads to.
async fn upload(founder: &Founder, code: &AnchorSetupCode) -> Result<(), AnchorSetupError> {
    let (metadata, archive) = founder.snapshot().await;
    open_setup(code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
        .await
        .unwrap()
        .upload(metadata, &archive, founder.pinned(), true)
        .await
}

/// The authorization view of the anchor's vault, read through a connection of its own.
async fn anchor_view(data_dir: &Path) -> AuthorizationView {
    let registry = DatabasePoolRegistry::default();
    let pool = registry
        .connect_path(DataPaths::new(data_dir.to_path_buf()).database())
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .unwrap()
        .unwrap();
    let view = Engine::vault()
        .authorization(&mut conn, &ctx)
        .await
        .unwrap()
        .unwrap();
    drop(conn);
    registry.close_all().await.unwrap();
    view
}

/// Waits until the server sequences as the anchor.
async fn wait_for_anchor(data_dir: &Path, server: &str) -> AuthorizationView {
    let deadline = Instant::now() + WAIT;
    loop {
        let view = anchor_view(data_dir).await;
        if view.anchor_device_id == server {
            return view;
        }
        assert!(
            Instant::now() < deadline,
            "the server never took the anchor"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn a_proven_desktop_activates_a_server_that_takes_the_anchor() {
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let code = printed_code(root, &endpoint);
            let server = enroll_and_upload(&founder, &code).await;
            let view = wait_for_anchor(root, &server).await;
            assert!(view.devices.iter().any(|device| device.device_id == server
                && device.device_key != founder.device.public_key()));
            let refused = open_setup(&code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .err()
                .expect("a used code should be refused");
            assert!(is_refused(&refused), "{refused}");
        })
        .await;
    });
}

#[test]
fn the_pre_auth_deadline_closes_a_stalled_connection_and_spares_an_upload() {
    // Well below the TLS and control frame timeouts, so only the deadline closes a stall early.
    const STALL_LIMIT: Duration = Duration::from_secs(5);
    let deadline = Duration::from_millis(800);
    let policy = PreAuthPolicy {
        tls_timeout: crate::limits::TLS_HANDSHAKE_TIMEOUT,
        deadline,
    };
    // One pending slot, so a proven connection that kept it would lock out the next one.
    let limits = ConnectionLimits::new(1, 1, 8, 8).with_setup_slots(2, 2);
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor_budget(root, limits, policy, |endpoint| async move {
            let code = printed_code(root, &endpoint);

            let mut silent = TcpStream::connect(&endpoint).await.unwrap();
            let started = Instant::now();
            let mut byte = [0_u8; 1];
            let read = tokio::time::timeout(STALL_LIMIT, silent.read(&mut byte)).await;
            assert!(
                matches!(read, Ok(Ok(0) | Err(_))),
                "a socket that never starts TLS is closed at the deadline"
            );
            assert!(started.elapsed() < STALL_LIMIT);

            let mut stalled = dial_server(&setup_target(&code), None, LoopbackPolicy::Allow)
                .await
                .unwrap();
            let hello = protocol::ControlMessage::AnchorHello {
                protocol_version: protocol::PROTOCOL_VERSION,
                compatibility: local_compatibility(APP_VERSION),
                client_nonce: protocol::relay::encode_relay_bytes(
                    &protocol::anchor::anchor_nonce().unwrap(),
                ),
            };
            let challenge = request_on(&mut stalled, &hello).await.unwrap();
            assert!(matches!(
                challenge,
                protocol::ControlMessage::AnchorChallenge { .. }
            ));
            let started = Instant::now();
            let next =
                tokio::time::timeout(STALL_LIMIT, protocol::read_control(&mut stalled)).await;
            assert!(
                matches!(next, Ok(Err(_))),
                "a client that greets and never proves is closed at the deadline"
            );
            assert!(started.elapsed() < STALL_LIMIT);

            let server = enroll_server(&founder, &code).await;
            let (metadata, archive) = founder.snapshot().await;
            let proven = open_setup(&code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .unwrap();
            tokio::time::sleep(deadline * 3).await;
            let other = open_setup(&code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .expect("a proven connection gives its pending slot back");
            drop(other);
            proven
                .upload(metadata, &archive, founder.pinned(), true)
                .await
                .expect("an upload that runs past the deadline finishes");
            wait_for_anchor(root, &server).await;
        })
        .await;
    });
}

#[test]
fn a_wrong_or_replaced_code_opens_nothing_and_keeps_the_newest_secret() {
    block_on(async {
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let older = printed_code(root, &endpoint);
            let newer = printed_code(root, &endpoint);
            let refused = open_setup(&older, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .err()
                .expect("a replaced code should be refused");
            assert!(is_refused(&refused), "{refused}");
            open_setup(&newer, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .expect("the newest code still works after a wrong proof");
        })
        .await;
    });
}

#[test]
fn a_snapshot_that_does_not_enroll_the_server_is_refused() {
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let code = printed_code(root, &endpoint);
            let (metadata, archive) = founder.snapshot().await;
            let refused = open_setup(&code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .unwrap()
                .upload(metadata, &archive, founder.pinned(), true)
                .await
                .expect_err("the upload should be refused");
            assert!(refused.to_string().contains("does not enroll"), "{refused}");
            let paths = DataPaths::new(root.to_path_buf());
            assert!(!paths.activation().exists());
            assert!(!paths.vault().exists());
            open_setup(&code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
                .await
                .expect("a refused upload does not spend the secret");
        })
        .await;
    });
}

/// Breaks the writer signature of the newest authorization record in the founder's store and
/// keeps the row verified, as a sender that planted the record would.
async fn forge_newest_record(founder: &Founder) {
    let (hash, mut envelope): (Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT envelope_hash, envelope FROM sync_auth_envelopes WHERE role = 'record'
         ORDER BY stored_order DESC LIMIT 1",
    )
    .fetch_one(&founder.pool)
    .await
    .unwrap();
    let last = envelope
        .last_mut()
        .expect("an envelope ends with its signature");
    *last ^= 0xff;
    sqlx::query("UPDATE sync_auth_envelopes SET envelope = ? WHERE envelope_hash = ?")
        .bind(envelope)
        .bind(hash)
        .execute(&founder.pool)
        .await
        .unwrap();
}

/// Replaces the founder's charter and local person key with ones another person signed. The
/// sender controls its database, so the trigger that keeps the identity fixed is lifted for the
/// update and restored after it.
async fn replace_person(founder: &Founder) {
    let other = PersonKeyPair::from_seed(&[8; 32]).unwrap();
    let charter = sign_charter(&founder.charter.charter, &other).unwrap();
    sqlx::query("UPDATE sync_spaces SET charter = ?")
        .bind(charter.encode())
        .execute(&founder.pool)
        .await
        .unwrap();
    let trigger: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master
         WHERE type = 'trigger' AND name = 'contacts_local_identity_immutable_update'",
    )
    .fetch_one(&founder.pool)
    .await
    .unwrap();
    sqlx::query("DROP TRIGGER contacts_local_identity_immutable_update")
        .execute(&founder.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE contacts_local_identity SET public_key = ?")
        .bind(other.public_key().to_text())
        .execute(&founder.pool)
        .await
        .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(trigger))
        .execute(&founder.pool)
        .await
        .unwrap();
}

#[test]
fn activation_reverifies_a_staged_authorization_store() {
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let code = printed_code(root, &endpoint);
            enroll_server(&founder, &code).await;
            forge_newest_record(&founder).await;
            let refused = upload(&founder, &code)
                .await
                .expect_err("an enrollment with a broken signature should not activate");
            assert!(refused.to_string().contains("does not enroll"), "{refused}");
            let paths = DataPaths::new(root.to_path_buf());
            assert!(!paths.activation().exists());
            assert!(!paths.vault().exists());

            replace_person(&founder).await;
            let refused = upload(&founder, &code)
                .await
                .expect_err("a copy under another person's charter should not activate");
            assert!(refused.to_string().contains("charter"), "{refused}");
            assert!(!paths.vault().exists());
        })
        .await;
    });
}

#[test]
fn a_recovery_kit_enrolls_a_new_desktop_through_the_server_as_anchor() {
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let code = printed_code(root, &endpoint);
            let server = enroll_and_upload(&founder, &code).await;
            wait_for_anchor(root, &server).await;

            let fresh = printed_code(root, &endpoint);
            let mut connection = open_setup(&fresh, APP_VERSION, None, LoopbackPolicy::Allow)
                .await
                .unwrap();
            let space = connection.identity().space.clone().expect("active space");
            assert_eq!(space.vault_id, VAULT_ID);
            assert_eq!(space.charter, encode_sync_charter(&founder.charter));
            let device = DeviceKeyPair::from_seed(&[40; 32]).unwrap();
            let admin = AdminKeyPair::from_seed(&[41; 32]).unwrap();
            let engine = Engine::vault();
            let mut conn = founder.pool.acquire().await.unwrap();
            let charter = charter_hash(&engine, &mut conn, &founder.ctx)
                .await
                .unwrap();
            drop(conn);
            let proposal = authority::recovery_enrollment(
                founder.ctx.space,
                charter,
                &recovery_seed().kit_code(),
                Enrollment {
                    device_id: "device-recovered".to_string(),
                    device_key: device.public_key(),
                    tls_fingerprint: [42; 32],
                    label: "Recovered".to_string(),
                    device_kind: DeviceKind::Desktop,
                },
                admin.public_key(),
                NOW_MS,
            )
            .unwrap();
            connection.recover(&proposal).await.unwrap();
            let view = anchor_view(root).await;
            assert!(view.devices.iter().any(|device| {
                device.device_id == "device-recovered"
                    && device.role == DeviceRole::Admin(admin.public_key())
            }));
        })
        .await;
    });
}

#[test]
fn a_backup_needs_a_stopped_activated_server() {
    block_on(async {
        let data = TempDir::new();
        let root = data.path();
        let out = root.join("out.ganbaru-backup");
        let config = AnchorBackupConfig {
            data_dir: root.to_path_buf(),
            out: out.clone(),
        };
        let refused = super::backup(config).await.unwrap_err();
        assert!(refused.contains("no vault"), "{refused}");
        assert!(!out.exists());
    });
}

#[test]
fn anchor_activation_record_damage_still_lets_the_anchor_start() {
    block_on(async {
        let data = TempDir::new();
        let root = data.path();
        std::fs::write(DataPaths::new(root.to_path_buf()).activation(), b"{damaged").unwrap();
        with_anchor(root, |endpoint| async move {
            tokio::net::TcpStream::connect(&endpoint).await.unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;
        })
        .await;
    });
}

/// A staged temporary file name for `target`, as an interrupted transfer leaves it.
fn staged_name(target: &str, nonce: char) -> String {
    format!(".{target}.{}.tmp", nonce.to_string().repeat(32))
}

#[test]
fn anchor_activation_removes_stale_staging() {
    block_on(async {
        let founder = Founder::new().await;
        let data = TempDir::new();
        let root = data.path();
        with_anchor(root, |endpoint| async move {
            let code = printed_code(root, &endpoint);
            let server = enroll_and_upload(&founder, &code).await;
            wait_for_anchor(root, &server).await;
        })
        .await;

        let engine = Engine::vault();
        let class = engine
            .manifest()
            .asset_classes()
            .next()
            .expect("the vault declares an asset class");
        let directory = class.directory(&DataPaths::new(root.to_path_buf()).vault());
        std::fs::create_dir_all(&directory).unwrap();
        let target = "a".repeat(64);
        let abandoned = directory.join(staged_name(&target, 'b'));
        let fresh = directory.join(staged_name(&target, 'c'));
        for path in [&abandoned, &fresh] {
            std::fs::write(path, b"partial").unwrap();
        }
        let old = SystemTime::now() - STALE_STAGING_AGE - Duration::from_secs(1);
        std::fs::File::options()
            .write(true)
            .open(&abandoned)
            .unwrap()
            .set_modified(old)
            .unwrap();

        with_anchor(root, |_endpoint| async {
            let deadline = Instant::now() + WAIT;
            while abandoned.exists() {
                assert!(
                    Instant::now() < deadline,
                    "the anchor kept a stale staged file"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(fresh.exists(), "the anchor removed a fresh staged file");
        })
        .await;
    });
}
