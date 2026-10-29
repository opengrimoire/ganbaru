//! What Settings > Sync shows of contests: each contested epoch instance without a winner and
//! each instance a writer fork froze, with its candidates, objections, yields, held devices,
//! and the actions the local device may take, plus the devices removed on branches that lost.
//!
//! Everything here derives from the stored envelope set through [`Resolution`]. The local device
//! id and admin key only annotate the view: which candidate is local, the local role, and which
//! actions the sealing paths in the `contest` module would accept. No authority rule reads them.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::{AdminPublicKey, DeviceRole, Digest32, Entry, Sequencing};
use sqlx::SqliteConnection;

use super::{AuthState, Outcome, Resolution, Signer, Stored};
use crate::error::SyncResult;
use crate::{Engine, SpaceContext, log};

/// The contests of the stored tree and the devices removed on branches that lost.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContestsView {
    /// One per frozen epoch instance on the way to the selected head.
    pub contests: Vec<ContestView>,
    /// Devices live at the selected head that a stored entry which is not effective removes,
    /// once per device, for the device review of a kit recovery.
    pub losing_removals: Vec<LosingRemoval>,
}

/// A frozen epoch instance: contested without a winner, or forked by a writer of its anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestView {
    /// The instance.
    pub instance: ContestInstance,
    /// The entry past which the instance is frozen, `None` for a take that won it alone.
    pub frozen_at: Option<ContestEntry>,
    /// Why it is frozen.
    pub cause: ContestCause,
    /// Handovers out of the instance, in stored walk order.
    pub candidates: Vec<ContestCandidate>,
    /// Entries of the instance that revoke a candidate's signer key or target.
    pub objections: Vec<ContestObjection>,
    /// Stored yields to the instance's candidates.
    pub yields: Vec<ContestYield>,
    /// Devices held while the instance is frozen, in device id order.
    pub held_device_ids: Vec<String>,
    /// The local device's part in the contest.
    pub local_role: ContestRole,
    /// What the local device may do.
    pub actions: Vec<ContestAction>,
}

/// An epoch instance, named by the entry that opened it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestInstance {
    /// Hash of the handover that opened it, the charter hash for epoch 0.
    pub root_hash: Digest32,
    /// Its epoch.
    pub epoch: u32,
    /// Its anchor.
    pub anchor_device_id: String,
}

/// A stored entry by hash and revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContestEntry {
    /// Entry hash.
    pub hash: Digest32,
    /// Revision.
    pub revision: u64,
}

/// Why an instance is frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestCause {
    /// Two handovers leave it, or an objection stands against its only one.
    Contested,
    /// A writer of its anchor forked and no handover won it.
    WriterFork,
    /// A forced take won it alone and opened the selected head's instance, and its anchor, the
    /// local device, may still object to it.
    Taken,
}

/// How a candidate handover was signed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    /// Signed by the target's admin key with the old anchor's stored vector.
    Planned,
    /// Signed by the target's admin key without the old anchor's stored vector.
    Forced,
    /// The anchor handing over to itself, as a restored anchor does.
    SelfHandover,
    /// Signed by the printed recovery kit.
    Kit,
    /// Signed by a recovery key the anchor installed from Settings.
    InstalledKey,
}

/// A handover out of a contested instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestCandidate {
    /// Entry hash.
    pub hash: Digest32,
    /// The entry it continues from.
    pub parent: ContestEntry,
    /// How it was signed.
    pub kind: CandidateKind,
    /// The device it makes the anchor.
    pub target_device_id: String,
    /// The device whose admin key signed it, `None` for the kit and installed recovery keys.
    pub signer_device_id: Option<String>,
    /// Revision that enrolled the target on the candidate's chain.
    pub target_enrolled_revision: Option<u64>,
    /// The device whose admin key enrolled the target, `None` for the charter's anchor or an
    /// enrollment signed by a recovery key.
    pub target_enroller_device_id: Option<String>,
    /// The candidate its signer yielded to, which withdraws it.
    pub withdrawn_in_favor_of: Option<Digest32>,
    /// Whether it makes the local device the anchor.
    pub local: bool,
}

/// An entry of the instance that objects to a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestObjection {
    /// Entry hash.
    pub hash: Digest32,
    /// The candidate it objects to.
    pub against: Digest32,
    /// The device whose admin key signed it, when one holds that key there.
    pub objector_device_id: Option<String>,
    /// Whether the anchor of the instance yielded to the candidate since.
    pub withdrawn: bool,
}

/// A stored yield to a candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContestYield {
    /// Entry hash.
    pub hash: Digest32,
    /// The device whose admin key signed it, when one holds that key there.
    pub author_device_id: Option<String>,
    /// The candidate it names.
    pub to: Digest32,
}

/// The local device's part in a contest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestRole {
    /// The target of a candidate its own admin key signed.
    Contestant { candidate: Digest32 },
    /// The anchor of the contested instance.
    Anchor,
    /// Neither.
    Bystander,
}

/// What the local device may do in a contest. Each yield and objection is one the sealing path
/// accepts as the stored set stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContestAction {
    /// Give way to another candidate.
    YieldTo { candidate: Digest32 },
    /// As the anchor, withdraw an objection by yielding to the candidate it objects to.
    WithdrawObjection { candidate: Digest32 },
    /// As the anchor, block a candidate by removing its target.
    Object { candidate: Digest32 },
    /// Use the printed recovery kit from one of `points`, the default first.
    UseKit { points: Vec<KitPoint> },
}

/// An entry a kit handover to the local desktop may continue from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitPoint {
    /// The entry.
    pub entry: ContestEntry,
    /// Its epoch.
    pub epoch: u32,
    /// Where it lies.
    pub place: KitPointPlace,
}

/// Where a kit point lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KitPointPlace {
    /// The selected head.
    Head,
    /// Under a candidate out of the frozen instance, continuing that candidate's work.
    Candidate { target_device_id: String },
    /// On a branch of the frozen instance past its freeze point.
    Continuation { anchor_device_id: String },
}

/// A device that a stored entry which is not effective removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LosingRemoval {
    /// The removed device.
    pub device_id: String,
    /// The device whose admin key signed the removal, when one holds that key there.
    pub removed_by_device_id: Option<String>,
    /// Revision of the removal on its branch.
    pub revision: u64,
}

/// The device holding `key` as its admin key in `state`, removed or not.
fn admin_device(state: &AuthState, key: &AdminPublicKey) -> Option<String> {
    state
        .devices
        .iter()
        .find(|(_, device)| device.role == DeviceRole::Admin(*key))
        .map(|(device_id, _)| device_id.clone())
}

impl Resolution<'_> {
    fn entry_of(&self, visit: usize) -> ContestEntry {
        ContestEntry {
            hash: self.walk.visits[visit].hash,
            revision: self.walk.visits[visit].revision,
        }
    }

    /// The device that signed visit `visit`, read at its parent.
    fn author_device(&self, visit: usize) -> Option<String> {
        let author = self.walk.visits[visit].author?;
        admin_device(&self.walk.selection(self.walk.parent(visit)).state, &author)
    }

    /// The revision that enrolled `device` on the chain through `visit`, with the device whose
    /// admin key signed that enrollment.
    fn enrollment(&self, visit: usize, device: &str) -> (Option<u64>, Option<String>) {
        let mut at = Some(visit);
        while let Some(index) = at {
            let current = &self.walk.visits[index];
            if let Some(node) = current.node
                && let Entry::Enroll(enroll) =
                    &self.walk.stored.nodes[node].record.proposal.proposal.entry
                && enroll.device_id == device
            {
                return (Some(current.revision), self.author_device(index));
            }
            at = current.parent;
        }
        // Only the charter's anchor is enrolled without an entry.
        (
            (self.walk.stored.charter.charter.anchor_device_id == device).then_some(0),
            None,
        )
    }

    fn candidate_view(&self, candidate: usize, instance: usize, local: &str) -> ContestCandidate {
        let visit = &self.walk.visits[candidate];
        let parent = self.walk.parent(candidate);
        let target = visit
            .handover
            .as_ref()
            .map(|(_, target)| target.clone())
            .unwrap_or_default();
        let anchor = self.instance_anchor(instance);
        let kind = match self.walk.signer(candidate) {
            Some(Signer::Kit) => CandidateKind::Kit,
            Some(Signer::Recovery) => CandidateKind::InstalledKey,
            _ if target == anchor => CandidateKind::SelfHandover,
            _ => {
                let planned = visit.node.is_some_and(|node| {
                    matches!(
                        self.walk.stored.nodes[node].record.sequencing,
                        Sequencing::Handover {
                            old_anchor_vector: Some(_)
                        }
                    )
                });
                if planned {
                    CandidateKind::Planned
                } else {
                    CandidateKind::Forced
                }
            }
        };
        let signer_device_id = match kind {
            CandidateKind::Kit | CandidateKind::InstalledKey => None,
            _ => self.author_device(candidate),
        };
        let (target_enrolled_revision, target_enroller_device_id) =
            self.enrollment(parent, &target);
        let siblings = self
            .candidates
            .get(&instance)
            .map_or(&[][..], Vec::as_slice);
        let withdrawn_in_favor_of = visit.author.and_then(|author| {
            self.walk
                .yields()
                .into_iter()
                .filter(|(key, named)| *key == author && *named != visit.hash)
                .map(|(_, named)| named)
                .find(|named| {
                    siblings
                        .iter()
                        .any(|other| self.walk.visits[*other].hash == *named)
                })
        });
        ContestCandidate {
            hash: visit.hash,
            parent: self.entry_of(parent),
            kind,
            local: target == local,
            target_device_id: target,
            signer_device_id,
            target_enrolled_revision,
            target_enroller_device_id,
            withdrawn_in_favor_of,
        }
    }

    /// Devices removed after `point` on stored entries that are not effective.
    pub(super) fn held_under(&self, point: usize) -> BTreeSet<String> {
        let mut held = BTreeSet::new();
        let mut stack = vec![point];
        while let Some(at) = stack.pop() {
            if !self.effective[at]
                && let Some(device) = &self.walk.visits[at].removes
            {
                held.insert(device.clone());
            }
            stack.extend(self.walk.children[at].iter().copied());
        }
        held
    }

    /// The kit points of `signer` for the local desktop `target`, labeled.
    fn labeled_kit_points(&self, signer: &AdminPublicKey, target: &str) -> Vec<KitPoint> {
        let head_instance = self.walk.visits[self.best].instance;
        let candidates = self
            .candidates
            .get(&head_instance)
            .map_or(&[][..], Vec::as_slice);
        self.kit_points(signer, target)
            .into_iter()
            .map(|point| {
                let place = if point == self.best {
                    KitPointPlace::Head
                } else if let Some(candidate) = candidates
                    .iter()
                    .find(|candidate| self.walk.is_ancestor(**candidate, point))
                {
                    KitPointPlace::Candidate {
                        target_device_id: self.walk.visits[*candidate]
                            .handover
                            .as_ref()
                            .map(|(_, target)| target.clone())
                            .unwrap_or_default(),
                    }
                } else {
                    KitPointPlace::Continuation {
                        anchor_device_id: self.instance_anchor(head_instance),
                    }
                };
                KitPoint {
                    entry: self.entry_of(point),
                    epoch: self.walk.selection(point).state.epoch,
                    place,
                }
            })
            .collect()
    }

    fn contest_view(
        &self,
        root: usize,
        point: Option<usize>,
        local: &str,
        admin: Option<&AdminPublicKey>,
        kit_points: &[KitPoint],
    ) -> ContestView {
        let candidates: Vec<usize> = self.candidates.get(&root).cloned().unwrap_or_default();
        let candidate_views: Vec<ContestCandidate> = candidates
            .iter()
            .map(|candidate| self.candidate_view(*candidate, root, local))
            .collect();
        let yields_stored = self.walk.yields();
        let mut objections = Vec::new();
        for candidate in &candidates {
            let against = self.walk.visits[*candidate].hash;
            let anchor_key = self.walk.visits[*candidate].parent_anchor_key;
            let withdrawn = anchor_key.is_some_and(|key| {
                yields_stored
                    .iter()
                    .any(|(author, named)| *author == key && *named == against)
            });
            for objection in self.walk.objections(*candidate) {
                objections.push(ContestObjection {
                    hash: self.walk.visits[objection].hash,
                    against,
                    objector_device_id: self.author_device(objection),
                    withdrawn,
                });
            }
        }
        let hashes: BTreeSet<Digest32> = candidates
            .iter()
            .map(|candidate| self.walk.visits[*candidate].hash)
            .collect();
        let yields: Vec<ContestYield> = (1..self.walk.visits.len())
            .filter(|visit| {
                self.walk.visits[*visit]
                    .yields_to
                    .is_some_and(|to| hashes.contains(&to))
            })
            .map(|visit| ContestYield {
                hash: self.walk.visits[visit].hash,
                author_device_id: self.author_device(visit),
                to: self.walk.visits[visit].yields_to.unwrap_or_default(),
            })
            .collect();
        let anchor = self.instance_anchor(root);
        let own = admin.and_then(|key| {
            candidates.iter().copied().find(|candidate| {
                let visit = &self.walk.visits[*candidate];
                visit.author == Some(*key)
                    && visit
                        .handover
                        .as_ref()
                        .is_some_and(|(_, target)| target == local)
            })
        });
        let local_role = match own {
            Some(candidate) => ContestRole::Contestant {
                candidate: self.walk.visits[candidate].hash,
            },
            None if anchor == local => ContestRole::Anchor,
            None => ContestRole::Bystander,
        };
        let mut actions = Vec::new();
        if let Some(key) = admin {
            let yielded_here = yields_stored
                .iter()
                .any(|(author, named)| author == key && hashes.contains(named));
            for (candidate, view) in candidates.iter().zip(&candidate_views) {
                let hash = self.walk.visits[*candidate].hash;
                match local_role {
                    ContestRole::Contestant { .. }
                        if !yielded_here
                            && view.withdrawn_in_favor_of.is_none()
                            && self.yield_point(key, local, &hash).is_ok() =>
                    {
                        actions.push(ContestAction::YieldTo { candidate: hash });
                    }
                    ContestRole::Anchor => {
                        let objected = objections
                            .iter()
                            .any(|objection| objection.against == hash && !objection.withdrawn);
                        if objected {
                            if self.yield_point(key, local, &hash).is_ok() {
                                actions.push(ContestAction::WithdrawObjection { candidate: hash });
                            }
                        } else if view.withdrawn_in_favor_of.is_none()
                            && self.objection_point(key, local, &hash).is_ok()
                        {
                            actions.push(ContestAction::Object { candidate: hash });
                        }
                    }
                    _ => {}
                }
            }
        }
        actions.push(ContestAction::UseKit {
            points: kit_points.to_vec(),
        });
        ContestView {
            instance: ContestInstance {
                root_hash: self.walk.visits[root].hash,
                epoch: self.walk.selection(root).state.epoch,
                anchor_device_id: anchor,
            },
            frozen_at: point.map(|point| self.entry_of(point)),
            cause: match point {
                None => ContestCause::Taken,
                Some(_) if candidates.is_empty() => ContestCause::WriterFork,
                Some(_) => ContestCause::Contested,
            },
            candidates: candidate_views,
            objections,
            yields,
            held_device_ids: point
                .map(|point| self.held_under(point).into_iter().collect())
                .unwrap_or_default(),
            local_role,
            actions,
        }
    }

    /// Whether `winner`, a forced take that won its instance alone, is one the local device
    /// `local`, the instance's anchor, may object to with `admin`.
    fn objectable_take(&self, winner: usize, local: &str, admin: Option<&AdminPublicKey>) -> bool {
        let Some(key) = admin else {
            return false;
        };
        let instance = self.walk.visits[self.walk.parent(winner)].instance;
        self.instance_anchor(instance) == local
            && self.candidate_view(winner, instance, local).kind == CandidateKind::Forced
            && self
                .objection_point(key, local, &self.walk.visits[winner].hash)
                .is_ok()
    }

    /// The contests on the way to the selected head and the losing removals, for the local
    /// device `local` holding `admin`.
    fn contests(&self, local: &str, admin: Option<&AdminPublicKey>) -> ContestsView {
        let head = self.walk.selection(self.best).state;
        let kit_points = self.labeled_kit_points(&head.kit_key, local);
        let head_instance = self.walk.visits[self.best].instance;
        let contests = self
            .outcomes
            .iter()
            .filter(|(root, _)| self.effective[**root])
            .filter_map(|(root, outcome)| match outcome {
                Outcome::FrozenAt(point) => {
                    Some(self.contest_view(*root, Some(*point), local, admin, &kit_points))
                }
                Outcome::Won(winner)
                    if *winner == head_instance && self.objectable_take(*winner, local, admin) =>
                {
                    Some(self.contest_view(*root, None, local, admin, &kit_points))
                }
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut losing_removals = Vec::new();
        for visit in 1..self.walk.visits.len() {
            if self.effective[visit] {
                continue;
            }
            let Some(device) = &self.walk.visits[visit].removes else {
                continue;
            };
            if head.live_device(device).is_err() || !seen.insert(device.clone()) {
                continue;
            }
            losing_removals.push(LosingRemoval {
                device_id: device.clone(),
                removed_by_device_id: self.author_device(visit),
                revision: self.walk.visits[visit].revision,
            });
        }
        ContestsView {
            contests,
            losing_removals,
        }
    }
}

impl Engine {
    /// The contests of the stored tree as the local device `device_id`, holding `admin` when it
    /// has an admin key, sees them, with the devices removed on branches that lost. `None`
    /// without a charter.
    pub async fn contests(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        device_id: &str,
        admin: Option<&AdminPublicKey>,
    ) -> SyncResult<Option<ContestsView>> {
        log::ensure_space(conn, ctx.space).await?;
        let Some(stored) = Stored::load(conn, ctx.space).await? else {
            return Ok(None);
        };
        Ok(Some(Resolution::new(&stored).contests(device_id, admin)))
    }
}
