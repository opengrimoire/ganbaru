//! The recovery kit of the active vault outside the sync service: its state, its creation on a
//! vault without a sync space, and the confirmation that the user saved it. Replacing the kit
//! of a vault with a sync space proposes authorization entries, so it runs through the service.
//!
//! The recovery seed waits in the device key store only until the user confirms saving its kit;
//! the vault database keeps the kit's two public keys, which found the charter at first link.

use std::sync::Arc;

use ganbaru_handoff::protocol::{QrMatrix, qr_matrix_for_bytes};
use ganbaru_sync::{AuthorizationView, Engine, local};
use ganbaru_sync_contracts::{KitCode, RecoveryPublicKeys, RecoverySeed};
use ganbaru_sync_replica::authority;
use ganbaru_sync_replica::writer::{SyncKey, SyncKeyStore};
use serde::{Serialize, Serializer};
use sqlx::SqliteConnection;

use super::service::blocking;

/// Longest last group a confirmation accepts, as typed, with room for separators and spaces.
pub(crate) const MAX_TYPED_GROUP_CHARS: usize = 16;

/// Serializes the kit commands, so a confirmation never forgets a seed another command just
/// replaced.
pub(crate) static KIT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The recovery kit of the active vault.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecoveryKitState {
    /// Whether the vault database keeps a kit's public keys.
    pub(crate) stored: bool,
    /// The kit that waits for the user to save it, if any.
    pub(crate) pending: Option<PendingRecoveryKit>,
}

/// A recovery kit whose seed the key store holds until the user saves it. Its debug form never
/// shows the code or its QR code.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingRecoveryKit {
    /// The grouped kit code with its checksum, serialized as its text. The serialized copy goes
    /// to the IPC layer, which cannot wipe it.
    #[serde(serialize_with = "serialize_kit_code")]
    pub(crate) code: KitCode,
    /// The kit code as a QR module matrix, for the printable page.
    pub(crate) qr: QrMatrix,
    /// Whether the kit protects the vault: the selected log names its recovery admin key, or,
    /// without a sync space, the vault database keeps its keys for the first charter. A
    /// regenerated kit is inactive until the anchor sequences it.
    pub(crate) active: bool,
}

/// Writes the kit code as its text, the shape the frontend reads.
fn serialize_kit_code<S: Serializer>(code: &KitCode, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(code.as_str())
}

impl std::fmt::Debug for RecoveryKitState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecoveryKitState")
            .field("stored", &self.stored)
            .field("pending", &self.pending)
            .finish()
    }
}

impl std::fmt::Debug for PendingRecoveryKit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingRecoveryKit")
            .field("code", &"<redacted>")
            .field("qr", &"<redacted>")
            .field("active", &self.active)
            .finish()
    }
}

/// The selected authorization state of the vault's sync space, `None` before it has one.
async fn authorization(
    conn: &mut SqliteConnection,
    vault_id: &str,
) -> Result<Option<AuthorizationView>, String> {
    let Some(ctx) = local::space_context(conn, vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    Engine::vault()
        .authorization(conn, &ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))
}

/// Whether the vault's sync space has a charter, which names its kit from then on.
async fn has_charter(conn: &mut SqliteConnection, vault_id: &str) -> Result<bool, String> {
    let Some(ctx) = local::space_context(conn, vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(false);
    };
    Engine::vault()
        .charter(conn, &ctx)
        .await
        .map(|charter| charter.is_some())
        .map_err(|error| format!("read the sync charter: {error}"))
}

async fn stored_keys(conn: &mut SqliteConnection) -> Result<Option<RecoveryPublicKeys>, String> {
    local::recovery_kit_keys(conn)
        .await
        .map_err(|error| format!("read the recovery kit keys: {error}"))
}

async fn pending_seed(
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<Option<RecoverySeed>, String> {
    let keys = keys.clone();
    let vault_id = vault_id.to_string();
    blocking(move || authority::pending_recovery_seed(keys.as_ref(), &vault_id)).await
}

/// The kit of `seed` as the frontend shows it.
fn pending_kit(seed: &RecoverySeed, active: bool) -> Result<PendingRecoveryKit, String> {
    let code = seed.kit_code();
    let qr = qr_matrix_for_bytes(code.as_str().as_bytes())?;
    Ok(PendingRecoveryKit { code, qr, active })
}

/// What decides whether a waiting kit protects the vault.
enum Protection {
    /// The selected log of the vault's sync space names the kit's recovery admin key.
    Charter(Box<AuthorizationView>),
    /// Without a charter, the vault database keeps these kit keys for the first one.
    Unlinked(Option<RecoveryPublicKeys>),
}

impl Protection {
    /// Whether the kit of `seed` protects the vault, as [`PendingRecoveryKit::active`] defines
    /// it.
    fn protects(&self, seed: &RecoverySeed) -> Result<bool, String> {
        match self {
            Self::Charter(view) => authority::recovery_kit_active(view, seed),
            Self::Unlinked(stored) => Ok(*stored == Some(authority::recovery_public_keys(seed)?)),
        }
    }
}

/// The recovery kit lifecycle of a vault, read once from its database and the key store.
struct KitLifecycle {
    /// Whether the vault database keeps a kit's public keys.
    stored: bool,
    /// What decides whether the waiting kit protects the vault.
    protection: Protection,
    /// The seed whose kit waits for the user to save it, with whether it protects the vault.
    waiting: Option<(RecoverySeed, bool)>,
}

/// Reads the recovery kit lifecycle of the vault `vault_id`.
async fn lifecycle(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<KitLifecycle, String> {
    let view = authorization(conn, vault_id).await?;
    let stored = stored_keys(conn).await?;
    let protection = match view {
        Some(view) => Protection::Charter(Box::new(view)),
        None => Protection::Unlinked(stored),
    };
    let waiting = match pending_seed(keys, vault_id).await? {
        Some(seed) => {
            let active = protection.protects(&seed)?;
            Some((seed, active))
        }
        None => None,
    };
    Ok(KitLifecycle {
        stored: stored.is_some(),
        protection,
        waiting,
    })
}

/// The recovery kit of the vault `vault_id`.
pub(crate) async fn state(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<RecoveryKitState, String> {
    let lifecycle = lifecycle(conn, keys, vault_id).await?;
    let pending = match &lifecycle.waiting {
        Some((seed, active)) => Some(pending_kit(seed, *active)?),
        None => None,
    };
    Ok(RecoveryKitState {
        stored: lifecycle.stored,
        pending,
    })
}

/// Creates the recovery kit of a vault without a sync space, replacing a kit created earlier:
/// the seed waits in the key store until the user saves the kit, and the vault database keeps
/// its public keys, which found the charter at first link. A vault with a sync space replaces
/// its kit through the anchor instead.
pub(crate) async fn create(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
    now_ms: u64,
) -> Result<(), String> {
    let replaced = replace_seed(conn, keys, vault_id).await?;
    finish_create(conn, keys, vault_id, &replaced, now_ms).await
}

/// A seed [`create`] wrote over the one that waited in the key store.
struct ReplacedSeed {
    previous: Option<RecoverySeed>,
    seed: RecoverySeed,
}

/// Writes a new seed for a vault without a charter, remembering the seed it replaced.
async fn replace_seed(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<ReplacedSeed, String> {
    if has_charter(conn, vault_id).await? {
        return Err("this vault already syncs; an admin computer replaces its kit".to_string());
    }
    let previous = pending_seed(keys, vault_id).await?;
    let seed = {
        let keys = keys.clone();
        let vault_id = vault_id.to_string();
        blocking(move || authority::create_recovery_seed(keys.as_ref(), &vault_id)).await?
    };
    Ok(ReplacedSeed { previous, seed })
}

/// Stores the public keys of the replaced seed, unless the first link founded a charter since
/// [`replace_seed`] checked. A creation that fails puts the replaced seed back, unless the
/// charter founded meanwhile names the new one, so a refused creation never loses the kit the
/// vault keeps nor leaves an unnamed seed waiting forever.
async fn finish_create(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
    replaced: &ReplacedSeed,
    now_ms: u64,
) -> Result<(), String> {
    let Err(error) = store_unless_founded(conn, vault_id, &replaced.seed, now_ms).await else {
        return Ok(());
    };
    match undo_replaced_seed(conn, keys, vault_id, replaced).await {
        Ok(()) => Err(error),
        Err(undo) => Err(format!("{error}; the replaced recovery seed stays: {undo}")),
    }
}

/// Puts back the seed `replaced` wrote over, unless the new seed already protects the vault:
/// the charter founded meanwhile names it, or its keys were stored after all.
async fn undo_replaced_seed(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
    replaced: &ReplacedSeed,
) -> Result<(), String> {
    let current = lifecycle(conn, keys, vault_id).await?;
    if current
        .waiting
        .is_some_and(|(seed, active)| active && seed.ct_eq(&replaced.seed))
    {
        return Ok(());
    }
    let keys = keys.clone();
    let vault_id = vault_id.to_string();
    let previous = replaced
        .previous
        .as_ref()
        .map(|seed| RecoverySeed::from_bytes(*seed.as_bytes()));
    blocking(move || match previous {
        Some(previous) => keys.write(&vault_id, SyncKey::RecoverySeed, previous.as_bytes()),
        None => keys.remove(&vault_id, SyncKey::RecoverySeed),
    })
    .await
}

/// Stores the public keys of `seed` as the vault's kit, unless the vault has a charter.
async fn store_unless_founded(
    conn: &mut SqliteConnection,
    vault_id: &str,
    seed: &RecoverySeed,
    now_ms: u64,
) -> Result<(), String> {
    let public = authority::recovery_public_keys(seed)?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|error| format!("begin the recovery kit transaction: {error}"))?;
    if has_charter(&mut tx, vault_id).await? {
        return Err("this vault started syncing; create the kit again".to_string());
    }
    local::store_recovery_kit_keys(&mut tx, &public, now_ms)
        .await
        .map_err(|error| format!("store the recovery kit keys: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit the recovery kit keys: {error}"))
}

/// Confirms that the user saved the waiting kit by typing back the last group of its code,
/// then deletes its seed, so no device keeps it. Returns false, keeping the seed, when the
/// typed group does not match. A kit that does not protect the vault yet is kept.
pub(crate) async fn confirm_saved(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
    typed_last_group: &str,
) -> Result<bool, String> {
    if typed_last_group.chars().count() > MAX_TYPED_GROUP_CHARS {
        return Ok(false);
    }
    let KitLifecycle {
        protection,
        waiting,
        ..
    } = lifecycle(conn, keys, vault_id).await?;
    let Some((seed, _)) = waiting else {
        return Err("no recovery kit waits to be saved".to_string());
    };
    if !seed.last_group_matches(typed_last_group) {
        return Ok(false);
    }
    // The key store checks protection again as it forgets, against the seed it holds then.
    let keys = keys.clone();
    let vault_id = vault_id.to_string();
    blocking(move || match protection {
        Protection::Charter(view) => {
            authority::forget_recovery_seed(keys.as_ref(), &vault_id, &view)
        }
        Protection::Unlinked(stored) => {
            authority::forget_unlinked_recovery_seed(keys.as_ref(), &vault_id, stored)
        }
    })
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
