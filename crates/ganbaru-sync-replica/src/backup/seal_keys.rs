//! What an encrypted backup reads from sync state: the keys a backup is sealed to and the epoch
//! keys this installation opened.

use super::BackupSealKeys;
use crate::keys::SpaceKeys;
use crate::service::blocking;
use crate::writer::SyncKeyStore;
use ganbaru_sync::{AuthorizationView, Engine, SpaceContext, local};
use ganbaru_sync_contracts::SpaceId;
use sqlx::SqliteConnection;
use std::sync::Arc;

/// The space context and selected authorization state of a vault, `None` where it has none.
pub async fn space_state(
    conn: &mut SqliteConnection,
    vault_id: &str,
) -> Result<Option<(SpaceContext, AuthorizationView)>, String> {
    let Some(ctx) = local::space_context(conn, vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    let view = Engine::vault()
        .authorization(conn, &ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?;
    Ok(view.map(|view| (ctx, view)))
}

/// The installation keys of a vault with an authorization log, with its epoch keys opened.
pub async fn opened_space_keys(
    conn: &mut SqliteConnection,
    keys: Arc<dyn SyncKeyStore>,
    vault_id: &str,
    ctx: &SpaceContext,
) -> Result<SpaceKeys, String> {
    let owned_vault_id = vault_id.to_string();
    let space_keys = blocking(move || SpaceKeys::load(keys.as_ref(), &owned_vault_id)).await?;
    space_keys.open_epochs(&Engine::vault(), conn, ctx).await?;
    Ok(space_keys)
}

/// The keys a backup of the vault is sealed to: the recovery encryption key the selected log
/// published, or else the one the vault keeps for its first charter, and the newest epoch key
/// this installation opened. `None` when the vault has no recovery kit.
pub async fn backup_seal_keys(
    conn: &mut SqliteConnection,
    keys: Arc<dyn SyncKeyStore>,
    vault_id: &str,
) -> Result<Option<BackupSealKeys>, String> {
    let state = space_state(conn, vault_id).await?;
    let stored = local::recovery_kit_keys(conn)
        .await
        .map_err(|error| format!("read the recovery kit keys: {error}"))?;
    let recovery = state
        .as_ref()
        .and_then(|(_, view)| view.recovery_encryption_key)
        .or(stored.map(|keys| keys.encryption));
    let Some(recovery) = recovery else {
        return Ok(None);
    };
    let epoch_key = match &state {
        Some((ctx, _)) => {
            let space_keys = opened_space_keys(conn, keys, vault_id, ctx).await?;
            space_keys
                .newest_epoch()
                .and_then(|epoch| space_keys.epoch_key(epoch))
        }
        None => None,
    };
    Ok(Some(BackupSealKeys {
        space: SpaceId::personal(vault_id),
        epoch_key,
        recovery,
    }))
}
