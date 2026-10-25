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
    AdminKeyPair, DeviceKind, Digest32, Entry, ProposalRejection, SignedProposal, VersionVector,
};
use ganbaru_sync_replica::authority::{self, ensure_admin_key};
use ganbaru_sync_replica::client::{self, PeerIds, SyncTransport};
use ganbaru_sync_replica::now_ms;
use ganbaru_sync_replica::outbox::Outbox;
use ganbaru_sync_replica::writer::ActiveWriter;
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

    /// Runs every proposal action and answers it, and returns the requests to take the anchor,
    /// which wait for the exchange of the pass.
    pub(super) async fn run_proposal_actions(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        is_hub: bool,
        requests: Vec<AuthorityRequest>,
    ) -> Vec<AuthorityRequest> {
        let mut take_anchor = Vec::new();
        for request in requests {
            if request.action == AuthorityAction::TakeAnchor {
                take_anchor.push(request);
                continue;
            }
            let outcome = self
                .propose_action(manager, vault_id, is_hub, &request.action)
                .await;
            request.answer(outcome);
        }
        take_anchor
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
    /// reported storing in that exchange, so the new anchor refuses removals until it knows
    /// every writer either stored. Proposals the coordinator sequenced after the exchange are
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

    /// The anchor duties of a linked desktop: reports the final rejections of the last
    /// forward and collects what the coordinator holds, sequences it, and sequences this
    /// desktop's own outbox. Returns whether it sealed anything, which the coordinator learns
    /// on the next exchange. The rejections live in memory only: after a restart the
    /// coordinator forwards the proposals again and they are rejected again.
    pub(super) async fn anchor_as_member<T: SyncTransport>(
        &self,
        transport: &T,
        ids: &PeerIds,
        writer: &mut Option<ActiveWriter>,
        settled: &mut Vec<(Digest32, ProposalRejection)>,
    ) -> Result<bool, String> {
        let Some(active) = writer.as_mut() else {
            return Ok(false);
        };
        let proposals = client::forward(transport, ids, settled).await?;
        settled.clear();
        let before = self.stored_head(active).await?;
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await?;
        let mut conn = acquire(&self.pool).await?;
        let now = now_ms()?;
        *settled = authority::sequence_forwarded(
            &self.engine,
            &mut conn,
            &self.ctx,
            &mut active.local(),
            &proposals,
            now,
        )
        .await?;
        if outbox.entries().iter().any(|entry| entry.pending()) {
            authority::sequence_outbox(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                &mut outbox,
                now,
            )
            .await?;
        }
        drop(conn);
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
