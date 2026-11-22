//! Founding a space while the user creates the recovery kit: the charter, the kit keys the vault
//! database keeps, and the seed waiting in the key store always name one kit.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::{NOW_MS, TestDirectory, VAULT_ID, acquire, block_on, create_vault, person};
use crate::authority::{self, Founding, RECOVERY_KIT_LOCK};
use crate::writer::{MemoryKeyStore, SyncKey, SyncKeyStore};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_sync::{Engine, SpaceContext, local};
use ganbaru_sync_contracts::{AdminPublicKey, RecoverySeed};
use sqlx::SqlitePool;
use tokio::sync::oneshot;

/// How long founding stays paused after its seed read when the user's creation cannot finish
/// meanwhile, as when founding holds the recovery kit lock.
const PAUSE: Duration = Duration::from_millis(300);

/// A key store that pauses the first recovery seed read after reading, until the test releases
/// it or [`PAUSE`] passes, so the user's kit creation runs between founding's read and its
/// writes.
struct PausingKeyStore {
    inner: MemoryKeyStore,
    pause: Mutex<Option<(oneshot::Sender<()>, mpsc::Receiver<()>)>>,
}

impl SyncKeyStore for PausingKeyStore {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        let pauses = matches!(key, SyncKey::RecoverySeed);
        let secret = self.inner.read(vault_id, key)?;
        if pauses {
            let pause = self.pause.lock().expect("pause").take();
            if let Some((reached, release)) = pause {
                let _ = reached.send(());
                let _ = release.recv_timeout(PAUSE);
            }
        }
        Ok(secret)
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        self.inner.write(vault_id, key, secret)
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        self.inner.remove(vault_id, key)
    }
}

/// A vault whose space has no charter yet.
async fn unfounded_vault(
    directory: &TestDirectory,
) -> (DatabasePoolRegistry, SqlitePool, SpaceContext) {
    let registry = DatabasePoolRegistry::default();
    let pool = create_vault(&registry, &directory.path().join("vault.sqlite")).await;
    let mut conn = acquire(&pool).await;
    Engine::vault()
        .init_space(&mut conn, VAULT_ID, NOW_MS)
        .await
        .expect("initialize space");
    let ctx = local::space_context(&mut conn, VAULT_ID)
        .await
        .expect("read space")
        .expect("space context");
    drop(conn);
    (registry, pool, ctx)
}

/// The user's kit creation on a vault without a charter, as the kit command runs it: under the
/// recovery kit lock, it replaces the waiting seed and stores its keys, refusing once a charter
/// names a kit. Returns the seed it created.
async fn create_kit(
    pool: &SqlitePool,
    ctx: &SpaceContext,
    keys: &Arc<PausingKeyStore>,
) -> Result<RecoverySeed, String> {
    let _kit = RECOVERY_KIT_LOCK.lock().await;
    let mut conn = acquire(pool).await;
    let founded = Engine::vault()
        .charter(&mut conn, ctx)
        .await
        .expect("read charter")
        .is_some();
    if founded {
        return Err("the vault started syncing".to_string());
    }
    let seed = authority::create_recovery_seed(keys.as_ref(), VAULT_ID)?;
    local::store_recovery_kit_keys(
        &mut conn,
        &authority::recovery_public_keys(&seed)?,
        NOW_MS + 1,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(seed)
}

fn admin_key(seed: &RecoverySeed) -> AdminPublicKey {
    authority::recovery_public_keys(seed)
        .expect("recovery keys")
        .admin
}

/// Founds the space while the user creates a kit between founding's seed read and its writes,
/// then checks that the charter, the stored kit keys, and the waiting seed name one kit, and
/// that a creation that reported success made that kit.
fn founding_and_kit_creation_name_one_kit(label: &str, pending: bool) {
    block_on(async {
        let directory = TestDirectory::new(label);
        let (_registry, pool, ctx) = unfounded_vault(&directory).await;
        let (reached, reached_rx) = oneshot::channel();
        let (release, release_rx) = mpsc::channel();
        let keys = Arc::new(PausingKeyStore {
            inner: MemoryKeyStore::default(),
            pause: Mutex::new(Some((reached, release_rx))),
        });
        if pending {
            authority::create_recovery_seed(&keys.inner, VAULT_ID).expect("pending seed");
        }
        let person = person();
        let founding = Founding {
            device_id: "device-anchor",
            tls_fingerprint: [41; 32],
            discovery_key: [42; 32],
            person: &person,
            now_ms: NOW_MS,
        };
        let engine = Engine::vault();
        let store: Arc<dyn SyncKeyStore> = keys.clone();
        let found =
            authority::found_with_recovery_kit(&engine, &pool, &ctx, store, VAULT_ID, &founding);
        let user = async {
            reached_rx.await.expect("founding reads the seed");
            let create = create_kit(&pool, &ctx, &keys);
            tokio::pin!(create);
            // A creation that waits on founding finishes after founding is released.
            let created = tokio::select! {
                created = &mut create => Some(created),
                () = tokio::time::sleep(PAUSE / 2) => None,
            };
            let _ = release.send(());
            match created {
                Some(created) => created,
                None => create.await,
            }
        };
        let (charter, created) = tokio::join!(found, user);
        let charter = charter.expect("found the space");

        let named = charter.charter.recovery_admin_key;
        let stored = local::recovery_kit_keys(&mut *acquire(&pool).await)
            .await
            .expect("read stored kit")
            .expect("the vault keeps the founding kit");
        assert_eq!(
            stored.admin, named,
            "the vault keeps the kit the charter names"
        );
        let waiting = authority::pending_recovery_seed(keys.as_ref(), VAULT_ID)
            .expect("read waiting seed")
            .expect("the founding kit waits for the user to save it");
        assert_eq!(
            admin_key(&waiting),
            named,
            "the kit the user is shown is the kit the charter names"
        );
        if let Ok(seed) = created {
            assert_eq!(
                admin_key(&seed),
                named,
                "a kit creation that succeeded made the kit the charter names"
            );
        }
    });
}

#[test]
fn a_kit_created_while_founding_reads_a_waiting_seed_matches_the_charter() {
    founding_and_kit_creation_name_one_kit("founding-pending-kit", true);
}

#[test]
fn a_kit_created_while_founding_creates_its_seed_matches_the_charter() {
    founding_and_kit_creation_name_one_kit("founding-new-kit", false);
}
