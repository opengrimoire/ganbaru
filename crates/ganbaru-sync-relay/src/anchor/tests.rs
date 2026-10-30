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
    SignedCharter, WriterCertificate, WriterKeyPair, sign_certificate,
};
use ganbaru_sync_replica::anchor_link::local_compatibility;
use ganbaru_sync_replica::anchor_setup::{AnchorSetupError, open_setup};
use ganbaru_sync_replica::authority::{
    self, AnchorKeys, Enrollment, Founding, charter_hash, ensure_charter,
};
use ganbaru_sync_replica::backup::{ArchiveGate, DATABASE_FILE, create_snapshot_archive};
use sqlx::SqlitePool;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let (_stop, shutdown) = oneshot::channel();
    let server = super::serve_on(listener, data_dir, shutdown);
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
    let (metadata, archive) = founder.snapshot().await;
    open_setup(code, APP_VERSION, Some(VAULT_ID), LoopbackPolicy::Allow)
        .await
        .unwrap()
        .upload(metadata, &archive, founder.pinned(), true)
        .await
        .unwrap();
    server.device_id
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
                .err()
                .expect("the upload should be refused");
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
