//! Read models of the vault's sync authority for Settings: the enrolled devices with their
//! roles, the proposals waiting in or refused from this device's outbox, and the late changes
//! of removed devices. Every read is local; changing authority runs through the service.

use std::collections::BTreeSet;

use ganbaru_sync::{
    AuthorizationView, DeviceNames, DeviceRef, Engine, LateChange, LateKind, local,
};
use ganbaru_sync_contracts::{DeviceKind, DeviceRole, Entry, LateRow, SetRole};
use ganbaru_sync_replica::outbox::{MAX_OUTBOX_PROPOSALS, Outbox, OutboxEntry};
use ganbaru_sync_replica::quarantine::Quarantine;
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use super::host::SyncHost;
use super::service::blocking;

/// The selected authorization state of the active vault's sync space.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthorityView {
    /// Device that sequences authorization changes.
    pub anchor_device_id: String,
    pub epoch: u32,
    pub revision: u64,
    /// Whether a fork of the anchor froze the epoch, so only a handover continues the log.
    pub frozen: bool,
    /// Whether this installation holds the admin key the log names for it. Always false on
    /// mobile, where admin actions are unavailable.
    pub own_is_admin: bool,
    /// Every enrolled device, removed ones included, in device id order.
    pub devices: Vec<AuthorityDeviceView>,
}

/// One enrolled device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AuthorityDeviceView {
    /// The device, with its pairing label or else the label its enrollment recorded.
    pub device: DeviceRef,
    pub kind: DeviceKindView,
    pub role: DeviceRoleView,
    pub enrolled_revision: u64,
    pub removed_revision: Option<u64>,
    /// Whether this installation quarantined the device.
    pub quarantined: bool,
    /// Whether the device anchors the space.
    pub anchor: bool,
}

/// Form factor of an enrolled device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DeviceKindView {
    Desktop,
    Mobile,
}

/// Role of an enrolled device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DeviceRoleView {
    Admin,
    Member,
}

/// A proposal in this device's outbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutboxEntryView {
    pub change: ProposalChangeView,
    /// Device the change names, when it names one.
    pub target: Option<DeviceRef>,
    /// When the proposal was made, in Unix milliseconds.
    pub created_at_ms: u64,
    pub state: OutboxStateView,
    /// The anchor's final reason for refusing it, by stable name.
    pub rejection: Option<&'static str>,
}

/// What a proposal changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ProposalChangeView {
    Enroll,
    Remove,
    MakeAdmin,
    MakeMember,
    ReplaceRecoveryKit,
    ResolveLate,
    Handover,
    Unknown,
}

/// Where a proposal stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OutboxStateView {
    /// Waits for the anchor.
    Pending,
    /// The local log holds it.
    Held,
    /// The anchor refused it for good.
    Rejected,
}

/// A row a removed device changed after its removal cutoff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LateChangeView {
    pub removal_revision: u64,
    /// The removed device.
    pub device: DeviceRef,
    /// Domain table, by name.
    pub table: String,
    pub row_key: String,
    pub kind: LateKindView,
    pub title: String,
    pub preview: String,
    /// When the newest late value was written, in Unix milliseconds; `None` for a deletion.
    pub changed_at_ms: Option<u64>,
    /// Whether this device already proposed a decision the anchor has not sequenced yet.
    pub decision_pending: bool,
}

/// Late changes one listing carries at most, the same bound as the proposal outbox, so a
/// removed device that changed many rows cannot make Settings render an unbounded list.
pub(crate) const MAX_LISTED_LATE_CHANGES: usize = MAX_OUTBOX_PROPOSALS;

/// The first late changes no admin decided yet, with how many there are in all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LateChangesView {
    /// At most [`MAX_LISTED_LATE_CHANGES`], in removal, table, and key order.
    pub changes: Vec<LateChangeView>,
    /// Every undecided late change, listed or not.
    pub total: usize,
}

/// What a removed device's late operations did to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LateKindView {
    Values,
    Tombstone,
    Created,
}

/// The authorization state of the active vault, or `None` before it joined a sync space.
pub(crate) async fn read<R: Runtime>(app: &AppHandle<R>) -> Result<Option<AuthorityView>, String> {
    let Some(view) = authorization(app).await? else {
        return Ok(None);
    };
    let vault_id = app.active_vault_id()?;
    let own = super::devices::stable_device_id(app)?;
    let own_is_admin = own_is_admin(app, &vault_id, &view, &own).await?;
    let path = app.sync_files(&vault_id)?.quarantine_path();
    let quarantined = blocking(move || Quarantine::load(&path)).await?;
    let names = super::devices::read(app)?;
    Ok(Some(authority_view(
        &view,
        &names,
        quarantined.devices(),
        own_is_admin,
    )))
}

/// The proposals of this device's outbox, newest first.
pub(crate) async fn outbox<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<OutboxEntryView>, String> {
    let entries = load_outbox(app).await?;
    let names = super::devices::read(app)?;
    Ok(outbox_views(entries.entries(), &names))
}

/// The first late changes no admin decided yet, in removal, table, and key order, with their
/// total.
pub(crate) async fn late_changes<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<LateChangesView, String> {
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let vault_id = app.active_vault_id()?;
    let engine = Engine::vault();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let Some(ctx) = local::space_context(&mut conn, &vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(LateChangesView::default());
    };
    let changes = engine
        .late_changes(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read late changes: {error}"))?;
    drop(conn);
    let outbox = load_outbox(app).await?;
    let names = super::devices::read(app)?;
    Ok(late_views(
        &engine,
        changes,
        outbox.entries(),
        &names,
        MAX_LISTED_LATE_CHANGES,
    ))
}

async fn authorization<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<AuthorizationView>, String> {
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let vault_id = app.active_vault_id()?;
    let engine = Engine::vault();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))?;
    let Some(ctx) = local::space_context(&mut conn, &vault_id)
        .await
        .map_err(|error| format!("read sync space: {error}"))?
    else {
        return Ok(None);
    };
    engine
        .authorization(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))
}

async fn load_outbox<R: Runtime>(app: &AppHandle<R>) -> Result<Outbox, String> {
    let vault_id = app.active_vault_id()?;
    let path = app.sync_files(&vault_id)?.outbox_path();
    blocking(move || Outbox::load(&path)).await
}

/// Whether this desktop's admin key is the one the log names for it.
#[cfg(desktop)]
async fn own_is_admin<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
    view: &AuthorizationView,
    own: &str,
) -> Result<bool, String> {
    let enrolled_admin = view.devices.iter().any(|device| {
        device.device_id == own
            && device.removed_revision.is_none()
            && matches!(device.role, DeviceRole::Admin(_))
    });
    if !enrolled_admin {
        return Ok(false);
    }
    let Some(key) = super::stored_admin_key(app, vault_id).await? else {
        return Ok(false);
    };
    Ok(ganbaru_sync_replica::authority::is_admin(view, own, &key))
}

/// Mobile devices are never admins.
#[cfg(mobile)]
async fn own_is_admin<R: Runtime>(
    _app: &AppHandle<R>,
    _vault_id: &str,
    _view: &AuthorizationView,
    _own: &str,
) -> Result<bool, String> {
    Ok(false)
}

/// The authorization view as Settings shows it.
fn authority_view(
    view: &AuthorizationView,
    names: &DeviceNames,
    quarantined: &BTreeSet<String>,
    own_is_admin: bool,
) -> AuthorityView {
    let mut devices: Vec<AuthorityDeviceView> = view
        .devices
        .iter()
        .map(|device| {
            let mut reference = names.describe(&device.device_id);
            if reference.device_label.is_none() {
                reference.device_label = device.label.clone().filter(|label| !label.is_empty());
            }
            AuthorityDeviceView {
                device: reference,
                kind: match device.kind {
                    DeviceKind::Desktop => DeviceKindView::Desktop,
                    DeviceKind::Mobile => DeviceKindView::Mobile,
                },
                role: match device.role {
                    DeviceRole::Admin(_) => DeviceRoleView::Admin,
                    DeviceRole::Member => DeviceRoleView::Member,
                },
                enrolled_revision: device.enrolled_revision,
                removed_revision: device.removed_revision,
                quarantined: quarantined.contains(&device.device_id),
                anchor: device.device_id == view.anchor_device_id,
            }
        })
        .collect();
    devices.sort_by(|left, right| left.device.device_id.cmp(&right.device.device_id));
    AuthorityView {
        anchor_device_id: view.anchor_device_id.clone(),
        epoch: view.epoch,
        revision: view.revision,
        frozen: view.frozen_at.is_some(),
        own_is_admin,
        devices,
    }
}

/// Outbox entries as Settings shows them, newest first.
fn outbox_views(entries: &[OutboxEntry], names: &DeviceNames) -> Vec<OutboxEntryView> {
    let mut views: Vec<OutboxEntryView> = entries
        .iter()
        .map(|entry| {
            let (change, target) = match &entry.proposal.proposal.entry {
                Entry::Enroll(enroll) => (ProposalChangeView::Enroll, Some(&enroll.device_id)),
                Entry::Remove(remove) => (ProposalChangeView::Remove, Some(&remove.device_id)),
                Entry::SetRole(SetRole::Device { device_id, role }) => (
                    match role {
                        DeviceRole::Admin(_) => ProposalChangeView::MakeAdmin,
                        DeviceRole::Member => ProposalChangeView::MakeMember,
                    },
                    Some(device_id),
                ),
                Entry::SetRole(SetRole::Recovery { .. }) => {
                    (ProposalChangeView::ReplaceRecoveryKit, None)
                }
                Entry::ResolveLate(_) => (ProposalChangeView::ResolveLate, None),
                Entry::Handover(handover) => (
                    ProposalChangeView::Handover,
                    Some(&handover.new_anchor_device_id),
                ),
                Entry::Yield(_) | Entry::Unknown { .. } => (ProposalChangeView::Unknown, None),
            };
            let state = if entry.rejection.is_some() {
                OutboxStateView::Rejected
            } else if entry.held.is_some() {
                OutboxStateView::Held
            } else {
                OutboxStateView::Pending
            };
            OutboxEntryView {
                change,
                target: target.map(|device_id| names.describe(device_id)),
                created_at_ms: u64::try_from(entry.proposal.proposal.created_at_ms).unwrap_or(0),
                state,
                rejection: entry.rejection.map(|rejection| rejection.as_str()),
            }
        })
        .collect();
    views.sort_by_key(|view| std::cmp::Reverse(view.created_at_ms));
    views
}

/// The first `limit` late changes as Settings shows them, marking those a pending proposal of
/// this device already decides, with the total of all of them.
fn late_views(
    engine: &Engine,
    changes: Vec<LateChange>,
    outbox: &[OutboxEntry],
    names: &DeviceNames,
    limit: usize,
) -> LateChangesView {
    let pending: Vec<(u64, &LateRow)> = outbox
        .iter()
        .filter(|entry| entry.pending())
        .filter_map(|entry| match &entry.proposal.proposal.entry {
            Entry::ResolveLate(resolve) => Some(resolve),
            _ => None,
        })
        .flat_map(|resolve| {
            resolve
                .rows
                .iter()
                .map(move |row| (resolve.removal_revision, row))
        })
        .collect();
    let known: Vec<(&'static str, LateChange)> = changes
        .into_iter()
        .filter_map(|late| Some((engine.manifest().table(late.table)?.name, late)))
        .collect();
    let total = known.len();
    let changes = known
        .into_iter()
        .take(limit)
        .map(|(table, late)| {
            let decision_pending = pending.iter().any(|(revision, row)| {
                *revision == late.removal_revision
                    && row.table == late.table
                    && row.row.as_str() == late.row_key
            });
            LateChangeView {
                removal_revision: late.removal_revision,
                device: names.describe(&late.device_id),
                table: table.to_string(),
                row_key: late.row_key,
                kind: match late.kind {
                    LateKind::Values => LateKindView::Values,
                    LateKind::Tombstone => LateKindView::Tombstone,
                    LateKind::Created => LateKindView::Created,
                },
                title: late.presentation.title,
                preview: late.presentation.preview,
                changed_at_ms: late.clock.map(|clock| clock.physical_ms()),
                decision_pending,
            }
        })
        .collect();
    LateChangesView { changes, total }
}

#[cfg(test)]
mod tests;
