//! Read model of the anchor handover contests and the record storage state for Settings: each
//! frozen instance with its candidates, objections, yields, and the actions this device may
//! take, the removals the recovery kit's device review lists, and the storage backstop with its
//! growth alerts. Every read is local; acting on a contest runs through the service.

use std::collections::HashMap;

use ganbaru_sync::{
    CandidateKind, ContestAction, ContestCandidate, ContestCause, ContestEntry, ContestRole,
    ContestView, ContestsView, DeviceNames, DeviceRef, Engine, KitPoint, KitPointPlace,
    LosingRemoval, local,
};
use ganbaru_sync_contracts::Digest32;
use ganbaru_sync_contracts::ids::hex;
use ganbaru_sync_replica::records::{BackstopState, GrowthAlert, RecordExchange, growth_alerts};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use super::host::{AppHost, SyncHost};

/// The contests of the active vault's sync space and its record storage state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContestsDto {
    /// Frozen instances, and an instance a lone forced take opened that this device anchors.
    pub contests: Vec<ContestDto>,
    /// Devices that stored entries which are not effective remove, for the kit device review.
    pub losing_removals: Vec<LosingRemovalDto>,
    pub storage: StorageDto,
}

/// One contested instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ContestDto {
    /// Hash of the entry that opened the instance, which identifies it.
    pub root_hash: String,
    pub epoch: u32,
    /// The instance's anchor.
    pub anchor: DeviceRef,
    /// The entry where the instance froze, `None` for a lone take this device may object to.
    pub frozen_at: Option<EntryDto>,
    pub cause: CauseDto,
    pub candidates: Vec<CandidateDto>,
    pub objections: Vec<ObjectionDto>,
    pub yields: Vec<YieldDto>,
    /// Devices whose operations the contest holds back.
    pub held: Vec<DeviceRef>,
    pub local_role: RoleDto,
    pub actions: Vec<ActionDto>,
}

/// An authorization entry by hash and revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryDto {
    pub hash: String,
    pub revision: u64,
}

/// Why an instance is contested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CauseDto {
    Contested,
    WriterFork,
    Taken,
}

/// How a candidate handover was signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CandidateKindDto {
    Planned,
    Forced,
    SelfHandover,
    Kit,
    InstalledKey,
}

/// A handover out of the contested instance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateDto {
    pub hash: String,
    pub parent: EntryDto,
    pub kind: CandidateKindDto,
    /// The device it makes the anchor.
    pub target: DeviceRef,
    /// The device whose admin key signed it, `None` for a recovery key.
    pub signer: Option<DeviceRef>,
    /// Revision that enrolled the target on the candidate's chain.
    pub target_enrolled_revision: Option<u64>,
    /// The device that enrolled the target, when an admin device did.
    pub target_enroller: Option<DeviceRef>,
    /// Hash of the candidate its signer yielded to.
    pub withdrawn_in_favor_of: Option<String>,
    /// Whether it makes this device the anchor.
    pub local: bool,
}

/// An objection of the instance's anchor to a candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObjectionDto {
    pub hash: String,
    /// Hash of the candidate it objects to.
    pub against: String,
    pub objector: Option<DeviceRef>,
    pub withdrawn: bool,
}

/// A stored yield.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct YieldDto {
    pub hash: String,
    pub author: Option<DeviceRef>,
    /// Hash of the candidate it names.
    pub to: String,
}

/// This device's part in a contest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum RoleDto {
    Contestant { candidate: String },
    Anchor,
    Bystander,
}

/// What this device may do in a contest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ActionDto {
    YieldTo {
        candidate: String,
    },
    WithdrawObjection {
        candidate: String,
    },
    Object {
        candidate: String,
    },
    /// Use the printed recovery kit from one of `points`, the default first.
    UseKit {
        points: Vec<KitPointDto>,
    },
}

/// An entry a kit handover to this desktop may continue from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KitPointDto {
    pub entry: EntryDto,
    pub epoch: u32,
    pub place: KitPlaceDto,
}

/// Where a kit point lies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum KitPlaceDto {
    Head,
    Candidate { target: DeviceRef },
    Continuation { anchor: DeviceRef },
}

/// A removal on a branch that is not effective.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LosingRemovalDto {
    pub device: DeviceRef,
    pub removed_by: Option<DeviceRef>,
    pub revision: u64,
}

/// The record store's storage state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StorageDto {
    /// The storage backstop, once it started; full sync resumes only when the user resumes it.
    pub backstop: Option<BackstopDto>,
    pub alerts: Vec<AlertDto>,
}

/// What the storage backstop still stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BackstopDto {
    /// Only kit and contest records draw on the reserve.
    Reserve,
    /// Record exchange stores nothing.
    Stopped,
}

/// A growth alert of the record store.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum AlertDto {
    Device {
        device: DeviceRef,
        bytes: u64,
    },
    Total {
        bytes: u64,
    },
    ForkedWriter {
        writer: String,
        device: Option<DeviceRef>,
    },
}

/// The contests and record storage of the active vault, or `None` before it joined a sync
/// space.
pub(crate) async fn read<R: Runtime>(
    app: &AppHandle<R>,
    records: &RecordExchange,
) -> Result<Option<ContestsDto>, String> {
    let pool = crate::db::connect_active_vault_for_sync(app).await?;
    let vault_id = AppHost::of(app).active_vault_id()?;
    let own = super::devices::stable_device_id(app)?;
    let admin = own_admin_key(app, &vault_id).await?;
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
    let Some(view) = engine
        .contests(&mut conn, &ctx, &own, admin.as_ref())
        .await
        .map_err(|error| format!("read sync contests: {error}"))?
    else {
        return Ok(None);
    };
    let enrollment_labels = engine
        .authorization(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync authorization: {error}"))?
        .map(|authorization| {
            authorization
                .devices
                .into_iter()
                .filter_map(|device| device.label.map(|label| (device.device_id, label)))
                .collect()
        })
        .unwrap_or_default();
    let storage = engine
        .authorization_storage(&mut conn, &ctx)
        .await
        .map_err(|error| format!("read sync record storage: {error}"))?;
    drop(conn);
    let names = Names {
        paired: super::devices::read(app)?,
        enrollment_labels,
    };
    Ok(Some(contests_dto(
        &view,
        records.backstop_state(),
        &growth_alerts(&storage, records.limits()),
        &names,
        cfg!(desktop),
    )))
}

#[cfg(desktop)]
async fn own_admin_key<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<Option<ganbaru_sync_contracts::AdminPublicKey>, String> {
    super::stored_admin_key(app, vault_id).await
}

/// Mobile devices hold no admin key.
#[cfg(mobile)]
async fn own_admin_key<R: Runtime>(
    _app: &AppHandle<R>,
    _vault_id: &str,
) -> Result<Option<ganbaru_sync_contracts::AdminPublicKey>, String> {
    Ok(None)
}

/// Device names from pairing, falling back to the label each enrollment recorded.
struct Names {
    paired: DeviceNames,
    enrollment_labels: HashMap<String, String>,
}

impl Names {
    fn describe(&self, device_id: &str) -> DeviceRef {
        let mut reference = self.paired.describe(device_id);
        if reference.device_label.is_none() {
            reference.device_label = self
                .enrollment_labels
                .get(device_id)
                .filter(|label| !label.is_empty())
                .cloned();
        }
        reference
    }

    fn describe_some(&self, device_id: Option<&String>) -> Option<DeviceRef> {
        device_id.map(|device_id| self.describe(device_id))
    }
}

/// The contests as Settings shows them. Without the kit, as on mobile, the kit action is left
/// out.
fn contests_dto(
    view: &ContestsView,
    backstop: Option<BackstopState>,
    alerts: &[GrowthAlert],
    names: &Names,
    kit_available: bool,
) -> ContestsDto {
    ContestsDto {
        contests: view
            .contests
            .iter()
            .map(|contest| contest_dto(contest, names, kit_available))
            .collect(),
        losing_removals: view
            .losing_removals
            .iter()
            .map(|removal| losing_removal_dto(removal, names))
            .collect(),
        storage: StorageDto {
            backstop: backstop.map(|state| match state {
                BackstopState::Reserve => BackstopDto::Reserve,
                BackstopState::Stopped => BackstopDto::Stopped,
            }),
            alerts: alerts.iter().map(|alert| alert_dto(alert, names)).collect(),
        },
    }
}

fn contest_dto(contest: &ContestView, names: &Names, kit_available: bool) -> ContestDto {
    ContestDto {
        root_hash: hex(&contest.instance.root_hash),
        epoch: contest.instance.epoch,
        anchor: names.describe(&contest.instance.anchor_device_id),
        frozen_at: contest.frozen_at.as_ref().map(entry_dto),
        cause: match contest.cause {
            ContestCause::Contested => CauseDto::Contested,
            ContestCause::WriterFork => CauseDto::WriterFork,
            ContestCause::Taken => CauseDto::Taken,
        },
        candidates: contest
            .candidates
            .iter()
            .map(|candidate| candidate_dto(candidate, names))
            .collect(),
        objections: contest
            .objections
            .iter()
            .map(|objection| ObjectionDto {
                hash: hex(&objection.hash),
                against: hex(&objection.against),
                objector: names.describe_some(objection.objector_device_id.as_ref()),
                withdrawn: objection.withdrawn,
            })
            .collect(),
        yields: contest
            .yields
            .iter()
            .map(|entry| YieldDto {
                hash: hex(&entry.hash),
                author: names.describe_some(entry.author_device_id.as_ref()),
                to: hex(&entry.to),
            })
            .collect(),
        held: contest
            .held_device_ids
            .iter()
            .map(|device_id| names.describe(device_id))
            .collect(),
        local_role: match contest.local_role {
            ContestRole::Contestant { candidate } => RoleDto::Contestant {
                candidate: hex(&candidate),
            },
            ContestRole::Anchor => RoleDto::Anchor,
            ContestRole::Bystander => RoleDto::Bystander,
        },
        actions: contest
            .actions
            .iter()
            .filter(|action| kit_available || !matches!(action, ContestAction::UseKit { .. }))
            .map(|action| action_dto(action, names))
            .collect(),
    }
}

fn entry_dto(entry: &ContestEntry) -> EntryDto {
    EntryDto {
        hash: hex(&entry.hash),
        revision: entry.revision,
    }
}

fn candidate_dto(candidate: &ContestCandidate, names: &Names) -> CandidateDto {
    CandidateDto {
        hash: hex(&candidate.hash),
        parent: entry_dto(&candidate.parent),
        kind: match candidate.kind {
            CandidateKind::Planned => CandidateKindDto::Planned,
            CandidateKind::Forced => CandidateKindDto::Forced,
            CandidateKind::SelfHandover => CandidateKindDto::SelfHandover,
            CandidateKind::Kit => CandidateKindDto::Kit,
            CandidateKind::InstalledKey => CandidateKindDto::InstalledKey,
        },
        target: names.describe(&candidate.target_device_id),
        signer: names.describe_some(candidate.signer_device_id.as_ref()),
        target_enrolled_revision: candidate.target_enrolled_revision,
        target_enroller: names.describe_some(candidate.target_enroller_device_id.as_ref()),
        withdrawn_in_favor_of: candidate.withdrawn_in_favor_of.as_ref().map(digest_hex),
        local: candidate.local,
    }
}

fn digest_hex(digest: &Digest32) -> String {
    hex(digest)
}

fn action_dto(action: &ContestAction, names: &Names) -> ActionDto {
    match action {
        ContestAction::YieldTo { candidate } => ActionDto::YieldTo {
            candidate: hex(candidate),
        },
        ContestAction::WithdrawObjection { candidate } => ActionDto::WithdrawObjection {
            candidate: hex(candidate),
        },
        ContestAction::Object { candidate } => ActionDto::Object {
            candidate: hex(candidate),
        },
        ContestAction::UseKit { points } => ActionDto::UseKit {
            points: points
                .iter()
                .map(|point| kit_point_dto(point, names))
                .collect(),
        },
    }
}

fn kit_point_dto(point: &KitPoint, names: &Names) -> KitPointDto {
    KitPointDto {
        entry: entry_dto(&point.entry),
        epoch: point.epoch,
        place: match &point.place {
            KitPointPlace::Head => KitPlaceDto::Head,
            KitPointPlace::Candidate { target_device_id } => KitPlaceDto::Candidate {
                target: names.describe(target_device_id),
            },
            KitPointPlace::Continuation { anchor_device_id } => KitPlaceDto::Continuation {
                anchor: names.describe(anchor_device_id),
            },
        },
    }
}

fn losing_removal_dto(removal: &LosingRemoval, names: &Names) -> LosingRemovalDto {
    LosingRemovalDto {
        device: names.describe(&removal.device_id),
        removed_by: names.describe_some(removal.removed_by_device_id.as_ref()),
        revision: removal.revision,
    }
}

fn alert_dto(alert: &GrowthAlert, names: &Names) -> AlertDto {
    match alert {
        GrowthAlert::Device { device_id, bytes } => AlertDto::Device {
            device: names.describe(device_id),
            bytes: *bytes,
        },
        GrowthAlert::Total { bytes } => AlertDto::Total { bytes: *bytes },
        GrowthAlert::ForkedWriter { writer, device_id } => AlertDto::ForkedWriter {
            writer: writer.to_hex(),
            device: names.describe_some(device_id.as_ref()),
        },
    }
}

#[cfg(test)]
mod tests;
