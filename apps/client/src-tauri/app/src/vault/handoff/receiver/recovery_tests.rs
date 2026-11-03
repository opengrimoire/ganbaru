//! A copy recovered through a server opens only when its log names the kit through a key the
//! server answered before the kit's public key left this device.

use super::settle_recovered_copy;
use crate::sync::{
    AnsweredKitKeys, RECOVERY_KIT_MISMATCH, RecoveredCopyError, RecoveryBinding,
    verify_recovered_copy,
};
use crate::vault::APP_SQLITE_FILE;
use ganbaru_contacts::{PersonKeyPair, PersonPublicKey};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::{Engine, LocalWriter, SequenceReservation, local};
use ganbaru_sync_contracts::{
    AdminKeyPair, AdminPublicKey, DeviceKeyPair, Entry, RecoveryPublicKeys, RecoverySeed, SetRole,
    SignedCharter, WriterCertificate, WriterKeyPair, sign_certificate,
};
use ganbaru_sync_replica::authority::{self, AnchorKeys, Founding, charter_hash, ensure_charter};
use std::path::{Path, PathBuf};

const VAULT_ID: &str = "vault-recovered";
const ANCHOR_DEVICE: &str = "device-anchor";
const NOW_MS: u64 = 1_800_000_000_000;

/// A private temporary folder removed with the value.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        Self(
            ganbaru_atomic_files::create_private_temporary_directory(
                &std::env::temp_dir(),
                "ganbaru-recovery-test-",
            )
            .unwrap(),
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Unreserved;

impl SequenceReservation for Unreserved {
    fn ensure_reserved(&mut self, _seq: u64) -> std::io::Result<()> {
        Ok(())
    }
}

fn person() -> PersonKeyPair {
    PersonKeyPair::from_seed(&[61; 32]).unwrap()
}

fn anchor_admin() -> AdminKeyPair {
    AdminKeyPair::from_seed(&[62; 32]).unwrap()
}

fn anchor_device() -> DeviceKeyPair {
    DeviceKeyPair::from_seed(&[64; 32]).unwrap()
}

fn seed(byte: u8) -> RecoverySeed {
    RecoverySeed::from_bytes([byte; 32])
}

fn keys_of(byte: u8) -> RecoveryPublicKeys {
    seed(byte).public_keys().unwrap()
}

fn kit_signer(byte: u8) -> AdminKeyPair {
    seed(byte).recovery_admin_key().unwrap()
}

/// The victim's printed kit.
const VICTIM: u8 = 70;
/// A kit the attacker founds a forged log with.
const ATTACKER: u8 = 71;

fn set_recovery(key: AdminPublicKey) -> Entry {
    Entry::SetRole(SetRole::Recovery {
        recovery_admin_key: key,
    })
}

/// Stages a vault of [`VAULT_ID`] at `staging` whose log is founded with the kit `founding_kit`
/// and then sequences `entries`, each proposed by its signer. Returns the charter and the person
/// key a server running it answers.
async fn stage_log(
    staging: &Path,
    founding_kit: RecoveryPublicKeys,
    entries: Vec<(AdminKeyPair, Entry)>,
) -> (SignedCharter, PersonPublicKey) {
    std::fs::create_dir_all(staging).unwrap();
    let path = staging.join(APP_SQLITE_FILE);
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
    let engine = Engine::vault();
    let mut conn = pool.acquire().await.unwrap();
    engine
        .init_space(&mut conn, VAULT_ID, NOW_MS)
        .await
        .unwrap();
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .unwrap()
        .unwrap();
    let keys = AnchorKeys {
        device: anchor_device(),
        admin: anchor_admin(),
        recovery: founding_kit,
    };
    let founding = Founding {
        device_id: ANCHOR_DEVICE,
        tls_fingerprint: [65; 32],
        discovery_key: [66; 32],
        person: &person(),
        now_ms: NOW_MS,
    };
    let charter = ensure_charter(&engine, &mut conn, &ctx, &keys, &founding)
        .await
        .unwrap();
    let writer = WriterKeyPair::from_seed(&[67; 32]).unwrap();
    let certificate = sign_certificate(
        &WriterCertificate {
            writer_key: writer.public_key(),
            device_id: ANCHOR_DEVICE.to_string(),
            device_key: anchor_device().public_key(),
            created_at_ms: i64::try_from(NOW_MS).unwrap(),
            predecessor: None,
        },
        &anchor_device(),
    )
    .unwrap();
    for (signer, entry) in entries {
        let hash = charter_hash(&engine, &mut conn, &ctx).await.unwrap();
        let proposal = authority::propose(ctx.space, hash, &signer, entry, NOW_MS).unwrap();
        let mut reservation = Unreserved;
        let mut local_writer = LocalWriter {
            key: &writer,
            certificate: &certificate,
            reservation: &mut reservation,
            keys: None,
        };
        authority::sequence(
            &engine,
            &mut conn,
            &ctx,
            &mut local_writer,
            &proposal,
            NOW_MS,
        )
        .await
        .unwrap()
        .unwrap();
    }
    drop(conn);
    registry.close_all().await.unwrap();
    (charter, ctx.person_key)
}

fn binding(
    (charter, person_key): (SignedCharter, PersonPublicKey),
    kit: RecoveryPublicKeys,
    kit_admin_key: AdminPublicKey,
    recovery_admin_key: AdminPublicKey,
) -> RecoveryBinding {
    RecoveryBinding {
        charter,
        person_key,
        kit,
        answered: AnsweredKitKeys {
            kit_admin_key,
            recovery_admin_key,
        },
    }
}

/// An empty marker file at `path`, as a recovery that sent its enrollment leaves.
fn write_marker(path: &Path) {
    std::fs::write(path, b"").unwrap();
}

/// Settles a forged copy and checks that it is refused as a kit mismatch and leaves nothing.
async fn assert_refused_as_mismatch(root: &Path, staging: &Path, binding: &RecoveryBinding) {
    let marker = root.join("server-recovery");
    write_marker(&marker);
    let settled = settle_recovered_copy(staging, &marker, VAULT_ID, binding).await;
    assert_eq!(settled, Err(RECOVERY_KIT_MISMATCH.to_string()));
    assert!(!staging.exists(), "the refused copy is removed");
    assert!(!marker.exists(), "a mismatch withdraws the recovery marker");
}

#[tokio::test]
async fn recovery_refuses_a_forged_log_rotated_to_the_kit() {
    let victim = keys_of(VICTIM);
    let installed = AdminKeyPair::from_seed(&[72; 32]).unwrap().public_key();
    let other = AdminKeyPair::from_seed(&[73; 32]).unwrap().public_key();

    // The forged log rotates its own kit to the victim's key, which sets both keys, and its
    // anchor then installs a recovery key of its own. The server answered the victim's key only
    // as the recovery key, so the log names it through a key the answer did not.
    let root = TempDir::new();
    let staging = root.0.join("staging");
    let trust = stage_log(
        &staging,
        keys_of(ATTACKER),
        vec![
            (kit_signer(ATTACKER), set_recovery(victim.admin)),
            (anchor_admin(), set_recovery(installed)),
        ],
    )
    .await;
    let forged = binding(trust, victim, other, victim.admin);
    assert_refused_as_mismatch(&root.0, &staging, &forged).await;

    // The mirror case: the anchor installs the victim's key as the recovery key, and the server
    // answered it only as the kit key.
    let mirror_root = TempDir::new();
    let mirror_staging = mirror_root.0.join("staging");
    let trust = stage_log(
        &mirror_staging,
        keys_of(ATTACKER),
        vec![(anchor_admin(), set_recovery(victim.admin))],
    )
    .await;
    let mirror = binding(trust, victim, victim.admin, other);
    assert_refused_as_mismatch(&mirror_root.0, &mirror_staging, &mirror).await;
}

#[tokio::test]
async fn recovery_refuses_a_log_that_never_names_the_kit() {
    let victim = keys_of(VICTIM);
    let root = TempDir::new();
    let staging = root.0.join("staging");
    let trust = stage_log(&staging, keys_of(ATTACKER), Vec::new()).await;
    let forged = binding(trust, victim, victim.admin, victim.admin);
    assert_refused_as_mismatch(&root.0, &staging, &forged).await;
}

#[tokio::test]
async fn recovery_refuses_a_copy_of_another_charter() {
    let printed = keys_of(VICTIM);
    let root = TempDir::new();
    let staging = root.0.join("staging");
    let (staged, person_key) = stage_log(&staging, printed, Vec::new()).await;
    let mut other = staged.charter.clone();
    other.created_at_ms += 1;
    let charter = ganbaru_sync_contracts::sign_charter(&other, &person()).unwrap();
    let answered = binding((charter, person_key), printed, printed.admin, printed.admin);
    assert_refused_as_mismatch(&root.0, &staging, &answered).await;
}

#[tokio::test]
async fn recovery_keeps_the_marker_on_a_read_failure() {
    let printed = keys_of(VICTIM);
    let honest = TempDir::new();
    let trust = stage_log(&honest.0.join("staging"), printed, Vec::new()).await;
    let answered = binding(trust, printed, printed.admin, printed.admin);

    let root = TempDir::new();
    let unreadable = root.0.join("unreadable");
    std::fs::create_dir_all(&unreadable).unwrap();
    std::fs::write(
        unreadable.join(APP_SQLITE_FILE),
        b"these bytes are not a SQLite database header at all",
    )
    .unwrap();
    assert!(
        matches!(
            verify_recovered_copy(&unreadable, VAULT_ID, &answered).await,
            Err(RecoveredCopyError::Failed(_))
        ),
        "a database that does not read says nothing about the kit"
    );

    let staging = root.0.join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(
        staging.join(APP_SQLITE_FILE),
        b"these bytes are not a SQLite database header at all",
    )
    .unwrap();
    let marker = root.0.join("server-recovery");
    write_marker(&marker);
    let settled = settle_recovered_copy(&staging, &marker, VAULT_ID, &answered).await;
    let Err(error) = settled else {
        panic!("an unreadable copy never activates");
    };
    assert_ne!(error, RECOVERY_KIT_MISMATCH);
    assert!(!staging.exists(), "the failed copy is removed");
    assert!(
        marker.exists(),
        "a failed copy keeps the marker so a retry skips the enrollment"
    );
}

#[tokio::test]
async fn recovery_accepts_the_printed_and_replaced_kits() {
    let printed = keys_of(VICTIM);
    let replaced = keys_of(74);
    let next = keys_of(75);

    // "Make a recovery key" on the anchor installs a recovery role beside the printed kit.
    let root = TempDir::new();
    let staging = root.0.join("staging");
    let trust = stage_log(
        &staging,
        printed,
        vec![(anchor_admin(), set_recovery(replaced.admin))],
    )
    .await;
    for kit in [printed, replaced] {
        let answered = binding(trust.clone(), kit, printed.admin, replaced.admin);
        assert_eq!(
            verify_recovered_copy(&staging, VAULT_ID, &answered).await,
            Ok(()),
            "a kit the server answered and the log names recovers the vault"
        );
    }
    let newest = binding(trust.clone(), next, printed.admin, replaced.admin);
    assert_eq!(
        verify_recovered_copy(&staging, VAULT_ID, &newest).await,
        Err(RecoveredCopyError::Mismatch),
        "a kit the log never names is not the vault's kit"
    );

    // A genuine copy settles and keeps its marker for the activation.
    let marker = root.0.join("server-recovery");
    write_marker(&marker);
    let answered = binding(trust, printed, printed.admin, replaced.admin);
    assert_eq!(
        settle_recovered_copy(&staging, &marker, VAULT_ID, &answered).await,
        Ok(())
    );
    assert!(
        staging.exists(),
        "the verified copy stays staged to activate"
    );
    assert!(marker.exists());
}
