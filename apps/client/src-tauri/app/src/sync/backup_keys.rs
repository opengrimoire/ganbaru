//! What encrypted vault backups read from sync state: the keys a backup is sealed to, the
//! epoch key that opens a backup of the active space, and how many other devices a space log
//! enrolled, which decides how a backup restores.

use ganbaru_sync::{AuthorizationView, Engine, SpaceContext, local};
use ganbaru_sync_contracts::{SpaceId, SpaceKey};
use ganbaru_sync_replica::backup::BackupSealKeys;
use ganbaru_sync_replica::keys::SpaceKeys;
use ganbaru_sync_replica::writer::SyncKeyStore;
use sqlx::SqliteConnection;
use std::sync::Arc;
use tauri::{AppHandle, Runtime};

use super::host::SyncHost;
use super::service::blocking;

/// The space of a vault and how many devices other than this one its log enrolled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkSummary {
    /// The vault's personal space.
    pub(crate) space: SpaceId,
    /// Devices other than this one that the selected log enrolled, removed ones included,
    /// since a removed device may still hold acknowledgements of the space.
    pub(crate) other_devices: usize,
}

/// The space context and selected authorization state of a vault, `None` where it has none.
async fn space_state(
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

/// The link summary of the vault `vault_id` whose database `conn` opens, seen from the device
/// `device_id`.
pub(crate) async fn link_summary(
    conn: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<LinkSummary, String> {
    let other_devices = space_state(conn, vault_id)
        .await?
        .map(|(_, view)| {
            view.devices
                .iter()
                .filter(|device| device.device_id != device_id)
                .count()
        })
        .unwrap_or(0);
    Ok(LinkSummary {
        space: SpaceId::personal(vault_id),
        other_devices,
    })
}

/// The installation keys of a vault with an authorization log, with its epoch keys opened.
async fn opened_space_keys(
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

/// The keys a backup of the active vault is sealed to: the recovery encryption key the
/// selected log published, or else the one the vault keeps for its first charter, and the
/// newest epoch key this installation opened. `None` when the vault has no recovery kit.
pub(crate) async fn backup_seal_keys<R: Runtime>(
    app: &AppHandle<R>,
    conn: &mut SqliteConnection,
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
            let space_keys = opened_space_keys(conn, app.key_store()?, vault_id, ctx).await?;
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

/// The key of `epoch` in the active vault's space when this installation opened it, so a
/// backup of this space opens without the kit. `None` for another space, epoch 0, or an
/// epoch this installation never held.
pub(crate) async fn active_epoch_key<R: Runtime>(
    app: &AppHandle<R>,
    conn: &mut SqliteConnection,
    vault_id: &str,
    space: &SpaceId,
    epoch: u32,
) -> Result<Option<SpaceKey>, String> {
    if epoch == 0 || *space != SpaceId::personal(vault_id) {
        return Ok(None);
    }
    let Some((ctx, _)) = space_state(conn, vault_id).await? else {
        return Ok(None);
    };
    let space_keys = opened_space_keys(conn, app.key_store()?, vault_id, &ctx).await?;
    Ok(space_keys.epoch_key(epoch))
}
