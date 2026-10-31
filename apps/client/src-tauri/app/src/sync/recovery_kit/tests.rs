use super::*;
use crate::vault::handoff::transport::tests::TestDirectory;
use ganbaru_contacts::PersonKeyPair;
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync_contracts::{AdminKeyPair, DeviceKeyPair};
use ganbaru_sync_replica::authority::{AnchorKeys, Founding, ensure_charter};
use ganbaru_sync_replica::writer::MemoryKeyStore;
use sqlx::SqlitePool;

const VAULT_ID: &str = "vault-kit";
const NOW_MS: u64 = 1_800_000_000_000;

async fn vault(registry: &DatabasePoolRegistry, directory: &TestDirectory) -> SqlitePool {
    let path = directory.path().join("vault.sqlite");
    ganbaru_db::test_support::write_migrated_database(&path)
        .await
        .expect("write migrated database");
    registry.connect_path(&path).await.expect("open vault")
}

fn last_group(code: &str) -> &str {
    code.rsplit('-').next().expect("a code has groups")
}

fn keys() -> Arc<dyn SyncKeyStore> {
    Arc::new(MemoryKeyStore::default())
}

#[tokio::test]
async fn an_unlinked_recovery_kit_is_kept_for_the_first_charter_once_confirmed() {
    let directory = TestDirectory::new("recovery-kit-unlinked");
    let registry = DatabasePoolRegistry::default();
    let pool = vault(&registry, &directory).await;
    let mut conn = pool.acquire().await.expect("connection");
    let keys = keys();

    let empty = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    assert!(!empty.stored && empty.pending.is_none());

    create(&mut conn, &keys, VAULT_ID, NOW_MS)
        .await
        .expect("create");
    let created = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    let kit = created.pending.expect("the kit waits to be saved");
    assert!(created.stored);
    assert!(kit.active, "the vault keeps the kit for its first charter");
    assert!(kit.qr.width > 0);
    assert_eq!(kit.qr.modules.len(), kit.qr.width * kit.qr.width);
    assert!(
        !format!("{kit:?}").contains(&kit.code),
        "debug never shows the code"
    );
    let seed = RecoverySeed::parse_kit_code(&kit.code).expect("parse");
    assert_eq!(
        local::recovery_kit_keys(&mut conn).await.expect("read"),
        Some(seed.public_keys().expect("kit keys")),
        "the vault database keeps the kit's public keys"
    );

    create(&mut conn, &keys, VAULT_ID, NOW_MS + 1)
        .await
        .expect("create again");
    let replaced = state(&mut conn, &keys, VAULT_ID)
        .await
        .expect("state")
        .pending
        .expect("replaced kit");
    assert_ne!(replaced.code, kit.code, "creating again replaces the kit");
    let seed = RecoverySeed::parse_kit_code(&replaced.code).expect("parse");
    assert_eq!(
        local::recovery_kit_keys(&mut conn).await.expect("read"),
        Some(seed.public_keys().expect("kit keys"))
    );

    assert!(
        !confirm_saved(&mut conn, &keys, VAULT_ID, last_group(&kit.code))
            .await
            .expect("confirm"),
        "the replaced kit's last group does not confirm"
    );
    assert!(
        !confirm_saved(
            &mut conn,
            &keys,
            VAULT_ID,
            &"0".repeat(MAX_TYPED_GROUP_CHARS + 1)
        )
        .await
        .expect("confirm"),
        "an overlong group does not confirm"
    );
    assert!(
        state(&mut conn, &keys, VAULT_ID)
            .await
            .expect("state")
            .pending
            .is_some(),
        "a failed confirmation keeps the kit"
    );
    let typed = last_group(&replaced.code).to_ascii_lowercase();
    assert!(
        confirm_saved(&mut conn, &keys, VAULT_ID, &format!(" {typed} "))
            .await
            .expect("confirm"),
        "the last group confirms as a person may type it"
    );
    let saved = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    assert!(saved.stored && saved.pending.is_none());
    assert!(
        confirm_saved(&mut conn, &keys, VAULT_ID, &typed)
            .await
            .is_err(),
        "no kit waits after it is saved"
    );
    drop(conn);
    registry.close_all().await.expect("close pools");
}

#[tokio::test]
async fn a_recovery_kit_is_created_only_before_the_vault_has_a_sync_space() {
    let directory = TestDirectory::new("recovery-kit-space");
    let registry = DatabasePoolRegistry::default();
    let pool = vault(&registry, &directory).await;
    let mut conn = pool.acquire().await.expect("connection");
    let keys = keys();
    let person = PersonKeyPair::from_seed(&[7; 32]).expect("person key");
    sqlx::query(
        "INSERT INTO contacts_local_identity (singleton, public_key, created_at)
         VALUES (1, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(person.public_key().to_text())
    .execute(&mut *conn)
    .await
    .expect("person identity");
    let engine = Engine::vault();
    engine
        .init_space(&mut conn, VAULT_ID, NOW_MS)
        .await
        .expect("initialize space");
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read space")
        .expect("the person identity names the space");

    create(&mut conn, &keys, VAULT_ID, NOW_MS)
        .await
        .expect("a space without a charter takes a kit");
    let kept = local::recovery_kit_keys(&mut conn)
        .await
        .expect("read")
        .expect("the kit keys are kept");

    let anchor = AnchorKeys {
        device: DeviceKeyPair::from_seed(&[8; 32]).expect("device key"),
        admin: AdminKeyPair::from_seed(&[9; 32]).expect("admin key"),
        recovery: kept,
    };
    let founding = Founding {
        device_id: "device-anchor",
        tls_fingerprint: [12; 32],
        discovery_key: [13; 32],
        person: &person,
        now_ms: NOW_MS,
    };
    ensure_charter(&engine, &mut conn, &ctx, &anchor, &founding)
        .await
        .expect("found the space");

    assert!(
        create(&mut conn, &keys, VAULT_ID, NOW_MS + 1)
            .await
            .is_err(),
        "a chartered space replaces its kit through the anchor"
    );
    assert_eq!(
        local::recovery_kit_keys(&mut conn).await.expect("read"),
        Some(kept),
        "a refused creation keeps the founding kit"
    );
    let founded = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    assert!(
        founded.pending.expect("the kit waits to be saved").active,
        "the charter names the kit created before the first link"
    );
    drop(conn);
    registry.close_all().await.expect("close pools");
}
