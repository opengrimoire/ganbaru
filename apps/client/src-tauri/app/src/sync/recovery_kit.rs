//! The recovery kit of the active vault outside the sync service: its state, its creation on a
//! vault without a sync space, and the confirmation that the user saved it. Replacing the kit
//! of a vault with a sync space proposes authorization entries, so it runs through the service.
//!
//! The recovery seed waits in the device key store only until the user confirms saving its kit;
//! the vault database keeps the kit's two public keys, which found the charter at first link.

use std::sync::Arc;

use ganbaru_handoff::protocol::{QrMatrix, qr_matrix_for_bytes};
use ganbaru_sync::{AuthorizationView, Engine, local};
use ganbaru_sync_contracts::{RecoveryPublicKeys, RecoverySeed};
use ganbaru_sync_replica::authority;
use ganbaru_sync_replica::writer::SyncKeyStore;
use serde::Serialize;
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
    /// The grouped kit code with its checksum.
    pub(crate) code: String,
    /// The kit code as a QR module matrix, for the printable page.
    pub(crate) qr: QrMatrix,
    /// Whether the kit protects the vault: the selected log names its recovery admin key, or,
    /// without a sync space, the vault database keeps its keys for the first charter. A
    /// regenerated kit is inactive until the anchor sequences it.
    pub(crate) active: bool,
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
    let qr = qr_matrix_for_bytes(code.as_bytes())?;
    Ok(PendingRecoveryKit { code, qr, active })
}

/// Whether the kit of `seed` protects the vault, as [`PendingRecoveryKit::active`] defines it.
fn kit_active(
    view: Option<&AuthorizationView>,
    stored: Option<RecoveryPublicKeys>,
    seed: &RecoverySeed,
) -> Result<bool, String> {
    match view {
        Some(view) => authority::recovery_kit_active(view, seed),
        None => Ok(stored == Some(authority::recovery_public_keys(seed)?)),
    }
}

/// The recovery kit of the vault `vault_id`.
pub(crate) async fn state(
    conn: &mut SqliteConnection,
    keys: &Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<RecoveryKitState, String> {
    let view = authorization(conn, vault_id).await?;
    let stored = stored_keys(conn).await?;
    let pending = match pending_seed(keys, vault_id).await? {
        Some(seed) => Some(pending_kit(
            &seed,
            kit_active(view.as_ref(), stored, &seed)?,
        )?),
        None => None,
    };
    Ok(RecoveryKitState {
        stored: stored.is_some(),
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
    if has_charter(conn, vault_id).await? {
        return Err("this vault already syncs; an admin computer replaces its kit".to_string());
    }
    let seed = {
        let keys = keys.clone();
        let vault_id = vault_id.to_string();
        blocking(move || authority::create_recovery_seed(keys.as_ref(), &vault_id)).await?
    };
    let public = authority::recovery_public_keys(&seed)?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|error| format!("begin the recovery kit transaction: {error}"))?;
    // The first link may have founded a charter since the check above; that charter names the
    // kit the vault kept then, and the new kit stays inactive until the user replaces it again.
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
    let Some(seed) = pending_seed(keys, vault_id).await? else {
        return Err("no recovery kit waits to be saved".to_string());
    };
    if !seed.last_group_matches(typed_last_group) {
        return Ok(false);
    }
    let view = authorization(conn, vault_id).await?;
    let stored = stored_keys(conn).await?;
    let keys = keys.clone();
    let vault_id = vault_id.to_string();
    blocking(move || match view {
        Some(view) => authority::forget_recovery_seed(keys.as_ref(), &vault_id, &view),
        None => authority::forget_unlinked_recovery_seed(keys.as_ref(), &vault_id, stored),
    })
    .await?;
    Ok(true)
}

#[cfg(test)]
mod tests;
