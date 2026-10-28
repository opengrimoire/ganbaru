//! Authority work of a desktop pass beyond the coordinator's own anchoring: proposals a
//! person asks for on this desktop, taking the anchor from the coordinator, the anchor duties
//! of a linked desktop that holds it, and the proposals the coordinator holds for such an
//! anchor.
//!
//! The coordinator keeps serving the hub, vault ownership, and pairing after a handover, so
//! members keep dialing it. A linked desktop that anchors collects what the coordinator holds
//! through forward requests and sequences it with its own writer.

use super::{Session, SyncHost, acquire, blocking};
use crate::vault::handoff::pairing::PairingManager;
use ganbaru_handoff::protocol::decode_sync_admin_key;
use ganbaru_sync::AuthorizationView;
use ganbaru_sync_contracts::{
    AdminKeyPair, DeviceKind, Digest32, Entry, LateResolution, LateRow, ProposalRejection,
    ResolveLate, RowKey, SignedProposal, TableId, VersionVector,
};
use ganbaru_sync_replica::authority::{self, ensure_admin_key};
use ganbaru_sync_replica::client::{self, PeerIds, SyncTransport};
use ganbaru_sync_replica::now_ms;
use ganbaru_sync_replica::outbox::Outbox;
use ganbaru_sync_replica::writer::ActiveWriter;
use std::collections::BTreeSet;
use tokio::sync::oneshot;

/// Authority work a person asks this desktop for, run by the next pass. Its debug form never
/// shows a recovery kit code.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AuthorityAction {
    /// Seal a handover that makes this linked desktop the anchor.
    TakeAnchor,
    /// Propose making a linked desktop an admin with the admin key it sent at pairing.
    Promote(String),
    /// Replace the recovery kit and propose its recovery admin key.
    RegenerateRecoveryKit,
    /// Read the recovery kit whose seed waits for the user to save it.
    PendingRecoveryKit,
    /// Forget the recovery seed after the user saved its kit.
    RecoveryKitSaved,
    /// Propose, signed with the recovery admin key the kit code derives, making this enrolled
    /// desktop an admin with its own admin key.
    UseRecoveryKit(String),
    /// Propose removing a device this desktop quarantined.
    ProposeRemoval(String),
    /// Propose removing a device from the vault, refusing what the anchor would reject.
    Remove(String),
    /// Restore or discard one late change, which needs the local writer for a restore.
    ResolveLate(LateDecision),
    /// Seal a handover to this linked desktop without the anchor, after the anchor is lost.
    /// An admin desktop signs with its admin key; any other enrolled desktop signs with the
    /// recovery admin key the kit code derives, and then promotes itself to admin.
    ForceAnchor(Option<String>),
}

/// An admin's decision on one late change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LateDecision {
    /// Revision of the removal that excluded the change.
    pub(crate) removal_revision: u64,
    /// Table of the row.
    pub(crate) table: TableId,
    /// Row key.
    pub(crate) row_key: String,
    /// Whether the late change is applied or dropped.
    pub(crate) resolution: LateResolution,
}

impl std::fmt::Debug for AuthorityAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TakeAnchor => formatter.write_str("TakeAnchor"),
            Self::Promote(device_id) => formatter.debug_tuple("Promote").field(device_id).finish(),
            Self::RegenerateRecoveryKit => formatter.write_str("RegenerateRecoveryKit"),
            Self::PendingRecoveryKit => formatter.write_str("PendingRecoveryKit"),
            Self::RecoveryKitSaved => formatter.write_str("RecoveryKitSaved"),
            Self::UseRecoveryKit(_) => formatter.write_str("UseRecoveryKit(<redacted>)"),
            Self::ProposeRemoval(device_id) => formatter
                .debug_tuple("ProposeRemoval")
                .field(device_id)
                .finish(),
            Self::Remove(device_id) => formatter.debug_tuple("Remove").field(device_id).finish(),
            Self::ResolveLate(decision) => formatter
                .debug_tuple("ResolveLate")
                .field(decision)
                .finish(),
            Self::ForceAnchor(None) => formatter.write_str("ForceAnchor(None)"),
            Self::ForceAnchor(Some(_)) => formatter.write_str("ForceAnchor(<redacted>)"),
        }
    }
}

/// What an authority action produced. Its debug form never shows a recovery kit code.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum AuthorityAnswer {
    /// The proposal waits in the outbox for the anchor, or the action needed none.
    Proposed,
    /// The authorization revision of the handover this desktop sealed.
    Anchored(u64),
    /// The authorization revision of the forced handover, and its promotion when the kit
    /// signed it, with the device id of the anchor it replaced.
    ForcedAnchor { revision: u64, old_anchor: String },
    /// The code of the regenerated recovery kit, shown once.
    RecoveryKit(String),
    /// The recovery kit waiting to be saved, if any.
    PendingRecoveryKit(Option<PendingRecoveryKit>),
}

/// A recovery kit whose seed the key store holds until the user saves it. Its debug form never
/// shows the code.
#[derive(Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingRecoveryKit {
    /// The grouped kit code with its checksum.
    pub(crate) code: String,
    /// Whether the selected log names the kit's recovery admin key; a regenerated kit is
    /// inactive until the anchor sequences it.
    pub(crate) active: bool,
}

impl std::fmt::Debug for AuthorityAnswer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Proposed => formatter.write_str("Proposed"),
            Self::Anchored(revision) => formatter.debug_tuple("Anchored").field(revision).finish(),
            Self::ForcedAnchor {
                revision,
                old_anchor,
            } => formatter
                .debug_struct("ForcedAnchor")
                .field("revision", revision)
                .field("old_anchor", old_anchor)
                .finish(),
            Self::RecoveryKit(_) => formatter.write_str("RecoveryKit(<redacted>)"),
            Self::PendingRecoveryKit(kit) => formatter
                .debug_tuple("PendingRecoveryKit")
                .field(kit)
                .finish(),
        }
    }
}

impl std::fmt::Debug for PendingRecoveryKit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingRecoveryKit")
            .field("code", &"<redacted>")
            .field("active", &self.active)
            .finish()
    }
}

/// An authority action with where its answer goes, if anyone waits for it.
pub(crate) struct AuthorityRequest {
    pub(crate) action: AuthorityAction,
    pub(crate) reply: Option<oneshot::Sender<Result<AuthorityAnswer, String>>>,
}

impl AuthorityRequest {
    /// Sends the answer, or reports a failure nobody waits for.
    pub(crate) fn answer(self, outcome: Result<AuthorityAnswer, String>) {
        match self.reply {
            Some(reply) => {
                let _ = reply.send(outcome);
            }
            None => {
                if let Err(error) = outcome {
                    eprintln!("sync: {error}");
                }
            }
        }
    }
}

/// A forward request of a linked device, with where the proposals it collects go.
pub(crate) struct ForwardRequest {
    pub(crate) device_id: String,
    pub(crate) settled: Vec<(Digest32, ProposalRejection)>,
    pub(crate) reply: oneshot::Sender<Result<Vec<SignedProposal>, String>>,
}

/// The selected authorization view with this installation's device id and admin key.
struct AdminContext {
    view: AuthorizationView,
    device_id: String,
    admin: AdminKeyPair,
}

impl AdminContext {
    fn is_admin(&self) -> bool {
        authority::is_admin(&self.view, &self.device_id, &self.admin.public_key())
    }

    fn anchors(&self) -> bool {
        self.view.anchor_device_id == self.device_id
    }
}

impl<H: SyncHost> Session<'_, H> {
    /// This installation's admin key, created once.
    pub(super) async fn admin_key(&self) -> Result<AdminKeyPair, String> {
        let keys = self.keys.clone();
        let vault_id = self.files.vault_id.clone();
        blocking(move || ensure_admin_key(keys.as_ref(), &vault_id)).await
    }

    async fn admin_context(&self, manager: &PairingManager) -> Result<AdminContext, String> {
        let view = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
                .ok_or_else(|| "this device has not joined the vault's sync space".to_string())?
        };
        Ok(AdminContext {
            view,
            device_id: manager.device_id()?,
            admin: self.admin_key().await?,
        })
    }

    /// Signs `entry` with the admin key and places it in the outbox, where the anchor's pass,
    /// the next exchange, or the anchor's forward request picks it up.
    async fn queue_proposal(&self, admin: &AdminContext, entry: Entry) -> Result<(), String> {
        let proposal = authority::propose(
            self.ctx.space,
            admin.view.charter,
            &admin.admin,
            entry,
            now_ms()?,
        )?;
        self.place_proposal(proposal).await
    }

    /// Places a signed proposal in the outbox.
    async fn place_proposal(&self, proposal: SignedProposal) -> Result<(), String> {
        let path = self.files.outbox_path();
        blocking(move || {
            let mut outbox = Outbox::load(&path)?;
            outbox.add(proposal).map(|_| ())
        })
        .await
    }

    /// Runs every proposal action and answers it. Returns the requests to take the anchor, which
    /// wait for the exchange of the pass, and the names of the tables restored late changes
    /// wrote.
    pub(super) async fn run_proposal_actions(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        is_hub: bool,
        writer: &mut Option<ActiveWriter>,
        requests: Vec<AuthorityRequest>,
    ) -> (Vec<AuthorityRequest>, BTreeSet<String>) {
        let mut take_anchor = Vec::new();
        let mut restored = BTreeSet::new();
        for request in requests {
            let outcome = match &request.action {
                AuthorityAction::TakeAnchor => {
                    take_anchor.push(request);
                    continue;
                }
                AuthorityAction::ResolveLate(decision) => {
                    self.resolve_late(manager, writer, decision, &mut restored)
                        .await
                }
                AuthorityAction::ForceAnchor(kit) => {
                    self.force_anchor(manager, is_hub, writer, kit.as_deref())
                        .await
                }
                action => self.propose_action(manager, vault_id, is_hub, action).await,
            };
            request.answer(outcome);
        }
        (take_anchor, restored)
    }

    /// Restores or discards one late change: a restore first seals the late values with the
    /// local writer, then both propose the decision so every replica records it once the anchor
    /// sequences it. Adds the table a restore wrote to `restored`.
    async fn resolve_late(
        &self,
        manager: &PairingManager,
        writer: &mut Option<ActiveWriter>,
        decision: &LateDecision,
        restored: &mut BTreeSet<String>,
    ) -> Result<AuthorityAnswer, String> {
        let admin = self.admin_context(manager).await?;
        if !admin.is_admin() {
            return Err("only an admin desktop can decide late changes".to_string());
        }
        let row = RowKey::new(decision.row_key.clone())
            .map_err(|_| "the late change names an invalid row".to_string())?;
        if decision.resolution == LateResolution::Restore {
            let Some(active) = writer.as_mut() else {
                return Err("the sync writer is not ready".to_string());
            };
            let mut conn = acquire(&self.pool).await?;
            let late = self
                .engine
                .late_changes(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read late changes: {error}"))?
                .into_iter()
                .find(|late| {
                    late.removal_revision == decision.removal_revision
                        && late.table == decision.table
                        && late.row_key == decision.row_key
                })
                .ok_or_else(|| "the late change is already decided".to_string())?;
            let report = self
                .engine
                .restore_late_change(&mut conn, &self.ctx, &late, &mut active.local(), now_ms()?)
                .await
                .map_err(|error| format!("restore the late change: {error}"))?;
            drop(conn);
            if report.sealed_ops > 0 {
                self.record_commit(active).await?;
                if let Some(table) = self.engine.manifest().table(decision.table) {
                    restored.insert(table.name.to_string());
                }
            }
        }
        let entry = Entry::ResolveLate(ResolveLate {
            removal_revision: decision.removal_revision,
            rows: vec![LateRow {
                table: decision.table,
                row,
                resolution: decision.resolution,
            }],
        });
        self.queue_proposal(&admin, entry).await?;
        Ok(AuthorityAnswer::Proposed)
    }

    /// Runs one proposal action. `vault_id` names the vault whose linked devices the
    /// coordinator holds; a promotion needs the admin key a linked desktop sent at pairing, so
    /// only the coordinator promotes.
    pub(super) async fn propose_action(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        is_hub: bool,
        action: &AuthorityAction,
    ) -> Result<AuthorityAnswer, String> {
        let admin = self.admin_context(manager).await?;
        match action {
            AuthorityAction::PendingRecoveryKit => {
                return self.pending_recovery_kit(&admin).await;
            }
            AuthorityAction::RecoveryKitSaved => {
                let keys = self.keys.clone();
                let vault_id = self.files.vault_id.clone();
                let view = admin.view.clone();
                blocking(move || authority::forget_recovery_seed(keys.as_ref(), &vault_id, &view))
                    .await?;
                return Ok(AuthorityAnswer::Proposed);
            }
            AuthorityAction::UseRecoveryKit(code) => {
                return self.use_recovery_kit(&admin, code).await;
            }
            // A quarantine on a device that is not an admin stays local.
            AuthorityAction::ProposeRemoval(_) if !admin.is_admin() => {
                return Ok(AuthorityAnswer::Proposed);
            }
            _ => {}
        }
        if !admin.is_admin() {
            return Err("only an admin desktop can change who belongs to the vault".to_string());
        }
        match action {
            AuthorityAction::TakeAnchor => {
                Err("taking the anchor runs after an exchange with the coordinator".to_string())
            }
            AuthorityAction::PendingRecoveryKit
            | AuthorityAction::RecoveryKitSaved
            | AuthorityAction::UseRecoveryKit(_) => {
                Err("the recovery kit action ran on the admin path".to_string())
            }
            AuthorityAction::ResolveLate(_) => {
                Err("deciding a late change runs with the sync writer".to_string())
            }
            AuthorityAction::ForceAnchor(_) => {
                Err("forcing the anchor runs with the sync writer".to_string())
            }
            AuthorityAction::Remove(device_id) => {
                if *device_id == admin.device_id {
                    return Err("this device cannot remove itself".to_string());
                }
                if *device_id == admin.view.anchor_device_id {
                    return Err("the anchor cannot be removed before a handover".to_string());
                }
                let live = admin.view.devices.iter().any(|device| {
                    device.device_id == *device_id && device.removed_revision.is_none()
                });
                if !live {
                    return Err("the device is not enrolled in the vault's sync space".to_string());
                }
                self.queue_proposal(&admin, authority::remove_entry(device_id))
                    .await?;
                Ok(AuthorityAnswer::Proposed)
            }
            AuthorityAction::Promote(device_id) => {
                if !is_hub {
                    return Err(
                        "only the coordinating computer holds the admin keys desktops sent"
                            .to_string(),
                    );
                }
                let peer = manager
                    .linked_peers_for(vault_id)?
                    .into_iter()
                    .find(|peer| peer.device_id == *device_id)
                    .ok_or_else(|| "the device is not linked to this vault".to_string())?;
                let key = peer
                    .sync_admin_key
                    .as_deref()
                    .ok_or_else(|| "only a linked desktop can become an admin".to_string())?;
                let key = decode_sync_admin_key(key)?;
                self.queue_proposal(&admin, authority::promote_entry(device_id, key))
                    .await?;
                Ok(AuthorityAnswer::Proposed)
            }
            AuthorityAction::RegenerateRecoveryKit => {
                let keys = self.keys.clone();
                let vault_id = self.files.vault_id.clone();
                let path = self.files.outbox_path();
                let space = self.ctx.space;
                let now = now_ms()?;
                let seed = blocking(move || {
                    let mut outbox = Outbox::load(&path)?;
                    authority::regenerate_recovery_kit(
                        keys.as_ref(),
                        &vault_id,
                        &mut outbox,
                        &admin.view,
                        space,
                        &admin.admin,
                        now,
                    )
                })
                .await?;
                Ok(AuthorityAnswer::RecoveryKit(seed.kit_code()))
            }
            AuthorityAction::ProposeRemoval(device_id) => {
                let live = admin.view.devices.iter().any(|device| {
                    device.device_id == *device_id && device.removed_revision.is_none()
                });
                if !live || *device_id == admin.view.anchor_device_id {
                    // The anchor cannot be removed before a handover, and an unknown or removed
                    // device needs no removal; the quarantine alone keeps it out.
                    return Ok(AuthorityAnswer::Proposed);
                }
                self.queue_proposal(&admin, authority::remove_entry(device_id))
                    .await?;
                Ok(AuthorityAnswer::Proposed)
            }
        }
    }

    /// The recovery kit whose seed the key store still holds, and whether the selected log
    /// names its recovery admin key.
    async fn pending_recovery_kit(&self, admin: &AdminContext) -> Result<AuthorityAnswer, String> {
        let keys = self.keys.clone();
        let vault_id = self.files.vault_id.clone();
        let Some(seed) =
            blocking(move || authority::pending_recovery_seed(keys.as_ref(), &vault_id)).await?
        else {
            return Ok(AuthorityAnswer::PendingRecoveryKit(None));
        };
        let active = authority::recovery_kit_active(&admin.view, &seed)?;
        Ok(AuthorityAnswer::PendingRecoveryKit(Some(
            PendingRecoveryKit {
                code: seed.kit_code(),
                active,
            },
        )))
    }

    /// Uses the recovery kit on this enrolled desktop: proposes, signed with the recovery admin
    /// key, making it an admin with its own admin key, so it can take the anchor once the
    /// anchor sequences the proposal. The derived key is dropped at once.
    async fn use_recovery_kit(
        &self,
        admin: &AdminContext,
        code: &str,
    ) -> Result<AuthorityAnswer, String> {
        if admin.is_admin() {
            return Err("this desktop is already an admin".to_string());
        }
        let device = admin
            .view
            .devices
            .iter()
            .find(|device| device.device_id == admin.device_id && device.removed_revision.is_none())
            .ok_or_else(|| "this desktop is not enrolled in the vault's sync space".to_string())?;
        if device.kind != DeviceKind::Desktop {
            return Err("only a desktop can become an admin".to_string());
        }
        let proposal = authority::recovery_promotion(
            &admin.view,
            self.ctx.space,
            code,
            &admin.device_id,
            admin.admin.public_key(),
            now_ms()?,
        )?;
        self.place_proposal(proposal).await?;
        Ok(AuthorityAnswer::Proposed)
    }

    /// Seals a handover that makes this linked desktop the anchor. Runs only after an exchange
    /// that left this device current with the coordinator. The old anchor vector is the
    /// per-writer maximum of this device's stored vector and `hub_stored`, what the coordinator
    /// reported storing in that exchange without the writers of devices this desktop
    /// quarantined, so the new anchor refuses removals until it knows every writer either
    /// stored. Proposals the coordinator sequenced after the exchange are
    /// superseded by the handover and return from the outboxes that hold them.
    pub(super) async fn take_anchor(
        &self,
        manager: &PairingManager,
        writer: &mut Option<ActiveWriter>,
        hub_stored: &VersionVector,
    ) -> Result<AuthorityAnswer, String> {
        let admin = self.admin_context(manager).await?;
        if admin.anchors() {
            return Err("this desktop already anchors the vault".to_string());
        }
        if !admin.is_admin() {
            return Err("only an admin desktop can take the anchor".to_string());
        }
        let Some(active) = writer.as_mut() else {
            return Err("the sync writer is not ready".to_string());
        };
        let mut conn = acquire(&self.pool).await?;
        let mut stored = self
            .engine
            .stored_vector(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync stored vector: {error}"))?;
        stored.merge(hub_stored);
        let revision = authority::seal_handover_here(
            &self.engine,
            &mut conn,
            &self.ctx,
            &admin.admin,
            &mut active.local(),
            Some(stored),
            now_ms()?,
        )
        .await?
        .map_err(|rejection| format!("take the anchor: {rejection}"))?;
        drop(conn);
        self.record_commit(active).await?;
        eprintln!("sync: this desktop took the anchor at authorization revision {revision}");
        Ok(AuthorityAnswer::Anchored(revision))
    }

    /// Forces the anchor to this linked desktop after the anchor is lost, without its stored
    /// vector, so operations only the lost anchor stored can become provisional again. An admin
    /// desktop signs with its admin key. Any other enrolled desktop needs `kit`: the recovery
    /// admin key it derives signs the handover and then this desktop's promotion to admin, and
    /// is dropped at once. The coordinator does not force the anchor: while it anchors nothing
    /// is lost, and otherwise its members reach it, so a planned handover applies.
    async fn force_anchor(
        &self,
        manager: &PairingManager,
        is_hub: bool,
        writer: &mut Option<ActiveWriter>,
        kit: Option<&str>,
    ) -> Result<AuthorityAnswer, String> {
        if is_hub {
            return Err("the coordinator takes the anchor through a planned handover".to_string());
        }
        let admin = self.admin_context(manager).await?;
        if admin.anchors() {
            return Err("this desktop already anchors the vault".to_string());
        }
        let device = admin
            .view
            .devices
            .iter()
            .find(|device| device.device_id == admin.device_id && device.removed_revision.is_none())
            .ok_or_else(|| "this desktop is not enrolled in the vault's sync space".to_string())?;
        if device.kind != DeviceKind::Desktop {
            return Err("only a desktop can anchor the vault".to_string());
        }
        let recovery;
        let (signer, promote) = if admin.is_admin() {
            (&admin.admin, None)
        } else {
            let code = kit.ok_or_else(|| {
                "only an admin desktop or the recovery kit can take the anchor".to_string()
            })?;
            recovery = authority::recovery_admin(&admin.view, code)?;
            (&recovery, Some(admin.admin.public_key()))
        };
        let Some(active) = writer.as_mut() else {
            return Err("the sync writer is not ready".to_string());
        };
        let mut conn = acquire(&self.pool).await?;
        let revision = authority::force_handover_here(
            &self.engine,
            &mut conn,
            &self.ctx,
            signer,
            promote,
            &mut active.local(),
            now_ms()?,
        )
        .await?
        .map_err(|rejection| format!("take the anchor: {rejection}"))?;
        drop(conn);
        self.record_commit(active).await?;
        eprintln!(
            "sync: this desktop forced the anchor at authorization revision {revision} without the lost anchor"
        );
        Ok(AuthorityAnswer::ForcedAnchor {
            revision,
            old_anchor: admin.view.anchor_device_id.clone(),
        })
    }

    /// The anchor duties of a linked desktop: reports the final rejections of the last
    /// forward and collects what the coordinator holds, sequences it, and sequences this
    /// desktop's own outbox. Returns whether it sealed anything, which the coordinator learns
    /// on the next exchange. The rejections live in memory only: after a restart the
    /// coordinator forwards the proposals again and they are rejected again. Removal cutoffs
    /// cover at least `covered`, the coordinator's last reported stored vector.
    pub(super) async fn anchor_as_member<T: SyncTransport>(
        &self,
        transport: &T,
        ids: &PeerIds,
        writer: &mut Option<ActiveWriter>,
        settled: &mut Vec<(Digest32, ProposalRejection)>,
        covered: &VersionVector,
    ) -> Result<bool, String> {
        let Some(active) = writer.as_mut() else {
            return Ok(false);
        };
        let proposals = client::forward(transport, ids, settled).await?;
        settled.clear();
        let before = self.stored_head(active).await?;
        {
            let mut conn = acquire(&self.pool).await?;
            *settled = authority::sequence_forwarded(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                &proposals,
                covered,
                now_ms()?,
            )
            .await?;
        }
        self.sequence_own_outbox(active, covered).await?;
        let after = self.record_commit(active).await?;
        Ok(after > before)
    }

    /// Sequences this anchoring desktop's own waiting proposals, also while the coordinator is
    /// away, as after a forced handover that proposes removing the lost anchor. Returns whether
    /// it sealed anything. Removal cutoffs cover at least `covered`.
    pub(super) async fn sequence_own_outbox(
        &self,
        active: &mut ActiveWriter,
        covered: &VersionVector,
    ) -> Result<bool, String> {
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await?;
        if !outbox.entries().iter().any(|entry| entry.pending()) {
            return Ok(false);
        }
        let before = self.stored_head(active).await?;
        {
            let mut conn = acquire(&self.pool).await?;
            authority::sequence_outbox(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                &mut outbox,
                covered,
                now_ms()?,
            )
            .await?;
        }
        let after = self.record_commit(active).await?;
        Ok(after > before)
    }

    /// On a coordinator that does not anchor, answers each forward request with the proposals
    /// it holds for the anchor; any other requester receives none.
    pub(super) async fn answer_forwards(&self, requests: Vec<ForwardRequest>) {
        if requests.is_empty() {
            return;
        }
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await;
        for request in requests {
            let outcome = match outbox.as_mut() {
                Ok(outbox) => match acquire(&self.pool).await {
                    Ok(mut conn) => {
                        authority::collect_forwards(
                            &self.engine,
                            &mut conn,
                            &self.ctx,
                            outbox,
                            &request.device_id,
                            &request.settled,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error.clone()),
            };
            let _ = request.reply.send(outcome);
        }
    }

    /// The stored head of the active writer.
    async fn stored_head(&self, active: &ActiveWriter) -> Result<u64, String> {
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .stored_vector(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync stored vector: {error}"))?
            .get(&active.id()))
    }

    /// Records the head the writer sealed through sequencing, so its record matches the log,
    /// and returns it.
    pub(super) async fn record_commit(&self, active: &mut ActiveWriter) -> Result<u64, String> {
        let seq = self.stored_head(active).await?;
        active.committed(seq)?;
        Ok(seq)
    }
}
