//! What encrypted vault backups read from sync state: the keys a backup is sealed to, the
//! epoch key that opens a backup of the active space, and how many other devices a space log
//! enrolled, which decides how a backup restores.

use ganbaru_sync_contracts::{SpaceId, SpaceKey};
use ganbaru_sync_replica::backup::BackupSealKeys;
use ganbaru_sync_replica::backup::seal_keys::{self, opened_space_keys, space_state};
use sqlx::SqliteConnection;
use tauri::{AppHandle, Runtime};

use super::host::{AppHost, SyncHost};

/// The space of a vault and how many devices other than this one its log enrolled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkSummary {
    /// The vault's personal space.
    pub(crate) space: SpaceId,
    /// Devices other than this one that the selected log enrolled, removed ones included,
    /// since a removed device may still hold acknowledgements of the space.
    pub(crate) other_devices: usize,
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

/// The keys a backup of the active vault is sealed to, see
/// [`seal_keys::backup_seal_keys`].
pub(crate) async fn backup_seal_keys<R: Runtime>(
    app: &AppHandle<R>,
    conn: &mut SqliteConnection,
    vault_id: &str,
) -> Result<Option<BackupSealKeys>, String> {
    seal_keys::backup_seal_keys(conn, AppHost::of(app).key_store()?, vault_id).await
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
    let space_keys = opened_space_keys(conn, AppHost::of(app).key_store()?, vault_id, &ctx).await?;
    Ok(space_keys.epoch_key(epoch))
}
