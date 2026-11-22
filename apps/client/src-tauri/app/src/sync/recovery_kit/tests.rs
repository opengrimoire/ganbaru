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
        !format!("{kit:?}").contains(kit.code.as_str()),
        "debug never shows the code"
    );
    let seed = RecoverySeed::parse_kit_code(kit.code.as_str()).expect("parse");
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
    let seed = RecoverySeed::parse_kit_code(replaced.code.as_str()).expect("parse");
    assert_eq!(
        local::recovery_kit_keys(&mut conn).await.expect("read"),
        Some(seed.public_keys().expect("kit keys"))
    );

    assert!(
        !confirm_saved(&mut conn, &keys, VAULT_ID, last_group(kit.code.as_str()))
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
    let typed = last_group(replaced.code.as_str()).to_ascii_lowercase();
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

#[test]
fn pending_recovery_kit_serializes_the_code() {
    let seed = RecoverySeed::from_bytes([0x52; 32]);
    let text = seed.kit_code().as_str().to_string();
    let kit = pending_kit(&seed, true).expect("pending kit");
    let json = serde_json::to_value(&kit).expect("serialize");
    assert_eq!(json["code"], serde_json::Value::String(text));
    assert_eq!(json["active"], serde_json::Value::Bool(true));
    assert!(json["qr"].is_object());
}

/// A vault with a sync space but no charter yet, and the founding inputs of its first link.
async fn unfounded_space(
    conn: &mut SqliteConnection,
) -> (ganbaru_sync::SpaceContext, PersonKeyPair) {
    let person = PersonKeyPair::from_seed(&[7; 32]).expect("person key");
    sqlx::query(
        "INSERT INTO contacts_local_identity (singleton, public_key, created_at)
         VALUES (1, ?, '2026-01-01T00:00:00Z')",
    )
    .bind(person.public_key().to_text())
    .execute(&mut *conn)
    .await
    .expect("person identity");
    Engine::vault()
        .init_space(conn, VAULT_ID, NOW_MS)
        .await
        .expect("initialize space");
    let ctx = local::space_context(conn, VAULT_ID)
        .await
        .expect("read space")
        .expect("the person identity names the space");
    (ctx, person)
}

/// Founds the charter as the first link does, naming the kit `recovery`.
async fn found(
    conn: &mut SqliteConnection,
    ctx: &ganbaru_sync::SpaceContext,
    person: &PersonKeyPair,
    recovery: RecoveryPublicKeys,
) {
    local::store_recovery_kit_keys(conn, &recovery, NOW_MS)
        .await
        .expect("store the founding kit");
    let anchor = AnchorKeys {
        device: DeviceKeyPair::from_seed(&[8; 32]).expect("device key"),
        admin: AdminKeyPair::from_seed(&[9; 32]).expect("admin key"),
        recovery,
    };
    let founding = Founding {
        device_id: "device-anchor",
        tls_fingerprint: [12; 32],
        discovery_key: [13; 32],
        person,
        now_ms: NOW_MS,
    };
    ensure_charter(&Engine::vault(), conn, ctx, &anchor, &founding)
        .await
        .expect("found the space");
}

async fn waiting_seed(keys: &Arc<dyn SyncKeyStore>) -> Option<[u8; 32]> {
    pending_seed(keys, VAULT_ID)
        .await
        .expect("read seed")
        .map(|seed| *seed.as_bytes())
}

#[tokio::test]
async fn a_kit_created_as_the_first_link_founds_keeps_the_kit_the_charter_names() {
    let directory = TestDirectory::new("recovery-kit-race-kept");
    let registry = DatabasePoolRegistry::default();
    let pool = vault(&registry, &directory).await;
    let mut conn = pool.acquire().await.expect("connection");
    let keys = keys();
    let (ctx, person) = unfounded_space(&mut conn).await;
    create(&mut conn, &keys, VAULT_ID, NOW_MS)
        .await
        .expect("create");
    let founding = waiting_seed(&keys).await.expect("the first kit waits");
    let kept = local::recovery_kit_keys(&mut conn)
        .await
        .expect("read")
        .expect("the kit keys are kept");

    // The first link founds with the kept kit between the two charter checks of a second
    // creation.
    let replaced = replace_seed(&mut conn, &keys, VAULT_ID)
        .await
        .expect("replace seed");
    found(&mut conn, &ctx, &person, kept).await;
    assert!(
        finish_create(&mut conn, &keys, VAULT_ID, &replaced, NOW_MS + 1)
            .await
            .is_err(),
        "the founded vault replaces its kit through the anchor"
    );

    assert_eq!(
        waiting_seed(&keys).await,
        Some(founding),
        "the seed of the kit the charter names waits again"
    );
    let state = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    assert!(state.pending.expect("the founding kit waits").active);
    assert_eq!(
        local::recovery_kit_keys(&mut conn).await.expect("read"),
        Some(kept)
    );
    drop(conn);
    registry.close_all().await.expect("close pools");
}

#[tokio::test]
async fn a_kit_the_first_link_founds_with_while_it_is_created_stays_waiting() {
    let directory = TestDirectory::new("recovery-kit-race-named");
    let registry = DatabasePoolRegistry::default();
    let pool = vault(&registry, &directory).await;
    let mut conn = pool.acquire().await.expect("connection");
    let keys = keys();
    let (ctx, person) = unfounded_space(&mut conn).await;

    // Without kept keys, the first link founds with the seed that waits, which is already the
    // one being created.
    let replaced = replace_seed(&mut conn, &keys, VAULT_ID)
        .await
        .expect("replace seed");
    assert!(replaced.previous.is_none());
    let named = authority::recovery_public_keys(&replaced.seed).expect("kit keys");
    found(&mut conn, &ctx, &person, named).await;
    assert!(
        finish_create(&mut conn, &keys, VAULT_ID, &replaced, NOW_MS + 1)
            .await
            .is_err()
    );

    assert_eq!(waiting_seed(&keys).await, Some(*replaced.seed.as_bytes()));
    let state = state(&mut conn, &keys, VAULT_ID).await.expect("state");
    assert!(
        state.pending.expect("the named kit waits").active,
        "the charter names the kit being created"
    );
    drop(conn);
    registry.close_all().await.expect("close pools");
}

#[tokio::test]
async fn a_creation_refused_before_any_kit_leaves_no_orphaned_seed() {
    let directory = TestDirectory::new("recovery-kit-race-none");
    let registry = DatabasePoolRegistry::default();
    let pool = vault(&registry, &directory).await;
    let mut conn = pool.acquire().await.expect("connection");
    let keys = keys();
    let (ctx, person) = unfounded_space(&mut conn).await;

    let replaced = replace_seed(&mut conn, &keys, VAULT_ID)
        .await
        .expect("replace seed");
    let other = RecoverySeed::from_bytes([0x33; 32]);
    found(
        &mut conn,
        &ctx,
        &person,
        authority::recovery_public_keys(&other).expect("kit keys"),
    )
    .await;
    assert!(
        finish_create(&mut conn, &keys, VAULT_ID, &replaced, NOW_MS + 1)
            .await
            .is_err()
    );

    assert_eq!(
        waiting_seed(&keys).await,
        None,
        "no unnamed seed is left waiting forever"
    );
    drop(conn);
    registry.close_all().await.expect("close pools");
}
