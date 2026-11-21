//! The selection rule: the tree of check-valid stored entries from the charter, how each epoch
//! instance resolves its handovers, and the selected chain the effective entry of highest rank
//! heads.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(test)]
use ganbaru_sync_contracts::AuthorizationRecord;
use ganbaru_sync_contracts::{
    AdminPublicKey, DevicePublicKey, DeviceRole, Digest32, Entry, ResolveLate, Seq, SetRole,
    SignedCharter, SignedProposal, SpaceId, WriterId,
};
use sqlx::SqliteConnection;

use crate::error::SyncResult;
use crate::records::{self, Node};

use super::{
    AuthState, AuthorizationView, EpochKeys, Rank, Revocation, Signer, Vetoes, load_charter,
    push_epoch_keys,
};

/// One entry of the selected chain, with the first stored record envelope that carries it in
/// carrier order: below its writer's horizon, then lowest hash.
#[derive(Debug, Clone)]
pub(super) struct SelectedEntry {
    pub(super) revision: u64,
    pub(super) epoch: u32,
    pub(super) entry_hash: Digest32,
    pub(super) proposal_hash: Digest32,
    pub(super) envelope_hash: Digest32,
}

/// The selected chain: its head state and its entries in revision order.
#[derive(Clone)]
pub(crate) struct Selection {
    pub(crate) state: AuthState,
    pub(super) path: Vec<SelectedEntry>,
    /// Late change decisions of the chain with their revisions, in revision order.
    pub(crate) resolutions: Vec<(u64, ResolveLate)>,
    /// Highest revision any stored chain reaches from the charter, selected or not.
    pub(crate) reachable: u64,
    /// Devices held while the head's epoch instance is frozen: removed on a stored branch of the
    /// contest that is not effective. Their operations wait and they are served authorization
    /// records only.
    pub(crate) held: BTreeSet<String>,
    /// Whether the stored tree holds a handover out of the head's epoch instance, so its anchor
    /// stops sequencing there.
    pub(crate) handed_over: bool,
    /// The key epochs of the chain in epoch order.
    pub(crate) key_epochs: Vec<EpochKeys>,
}

impl Selection {
    /// This chain extended by `node`, which its head state admits.
    fn extended(mut self, node: &Node) -> Self {
        self.state.advance(&node.record, node.hash);
        push_epoch_keys(&mut self.key_epochs, &node.record);
        if let Some(carrier) = node.carriers.first() {
            self.path.push(SelectedEntry {
                revision: node.record.revision,
                epoch: node.record.epoch,
                entry_hash: node.hash,
                proposal_hash: node.record.proposal.hash(),
                envelope_hash: carrier.envelope_hash,
            });
        }
        if let Entry::ResolveLate(resolve) = &node.record.proposal.proposal.entry {
            self.resolutions
                .push((node.record.revision, resolve.clone()));
        }
        self
    }

    /// The view of the selected chain with the devices it holds marked.
    pub(crate) fn view(&self) -> AuthorizationView {
        let mut view = self.state.view();
        for device in &mut view.devices {
            device.held = self.held.contains(&device.device_id);
        }
        view
    }

    /// Device id of the selected anchor.
    pub(crate) fn anchor(&self) -> &str {
        &self.state.anchor_device_id
    }
}

/// Work done by the anchor's reversal check. The unit tally counts nothing, so production pays
/// no counting.
trait ReversalTally {
    /// One visit taken from the pending stack.
    fn popped(&mut self) {}
    /// One comparison of a later entry with the proposal.
    fn compared(&mut self) {}
}

impl ReversalTally for () {}

/// Whether `later`, a later entry of the same branch, reverses `earlier`: it sets the role of or
/// removes the device `earlier` set the role of, removes the device `earlier` enrolled or gives
/// another role to a device `earlier` enrolled as admin, or replaces the recovery admin key or
/// recovery key `earlier` set. An enrollment of a member followed by a promotion is no reversal.
fn reverses(later: &Entry, earlier: &Entry) -> bool {
    match (earlier, later) {
        (
            Entry::SetRole(SetRole::Device { device_id, .. }),
            Entry::SetRole(SetRole::Device {
                device_id: later_device,
                ..
            }),
        ) => device_id == later_device,
        (Entry::SetRole(SetRole::Device { device_id, .. }), Entry::Remove(remove)) => {
            *device_id == remove.device_id
        }
        (Entry::Enroll(enroll), Entry::Remove(remove)) => enroll.device_id == remove.device_id,
        (Entry::Enroll(enroll), Entry::SetRole(SetRole::Device { device_id, role })) => {
            matches!(enroll.role, DeviceRole::Admin(_))
                && enroll.device_id == *device_id
                && enroll.role != *role
        }
        (Entry::SetRole(SetRole::Recovery { .. }), Entry::SetRole(SetRole::Recovery { .. }))
        | (Entry::RecoveryKey(_), Entry::RecoveryKey(_)) => true,
        _ => false,
    }
}

/// Readiness revision of a space whose stored chains reach a writer-fork freeze: every
/// revision, kept within the stored integer range.
const UNBOUNDED_REVISION: u64 = i64::MAX.unsigned_abs();

/// The charter and what the record store derives for the walk.
pub(super) struct Stored {
    pub(super) charter: SignedCharter,
    /// Every node, see [`records::load`].
    pub(super) nodes: Vec<Node>,
    /// The device id and key each verified certificate of a writer names.
    pub(super) certificates: BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>,
    /// Lowest sequence of each writer with provable fork evidence.
    provable: BTreeMap<WriterId, Seq>,
    /// Verified record bodies that are not nodes, see [`records::Loaded::others`]. The walk
    /// never reads them; only the record exchange does.
    pub(super) others: Vec<Node>,
    /// Whether a node needs a certificate of a carrier that names its anchor device and key.
    /// Only the selection rule's tests, which have no certificates, leave it unset.
    check_binding: bool,
}

impl Stored {
    /// The key epochs of the stored chain from the charter to the node `head`, in epoch order.
    pub(super) fn key_epochs(&self, head: &Digest32) -> Vec<EpochKeys> {
        let by_hash: BTreeMap<&Digest32, &Node> =
            self.nodes.iter().map(|node| (&node.hash, node)).collect();
        let mut chain = Vec::new();
        let mut at = by_hash.get(head).copied();
        while let Some(node) = at {
            chain.push(&node.record);
            at = by_hash.get(&node.record.previous_entry_hash).copied();
        }
        let mut key_epochs = Vec::new();
        for record in chain.into_iter().rev() {
            push_epoch_keys(&mut key_epochs, record);
        }
        key_epochs
    }

    /// What `space` stores, or `None` without a charter.
    pub(super) async fn load(
        conn: &mut SqliteConnection,
        space: SpaceId,
    ) -> SyncResult<Option<Self>> {
        let Some(charter) = load_charter(conn, space).await? else {
            return Ok(None);
        };
        let loaded = records::load(conn, space, &charter.hash()).await?;
        Ok(Some(Self {
            charter,
            nodes: loaded.nodes,
            certificates: loaded.certificates,
            provable: loaded.provable,
            others: loaded.others,
            check_binding: true,
        }))
    }

    /// The carriers of `node` bound at `state`, its parent: those with a certificate that names
    /// the node's anchor device and the key that device holds at `state`.
    fn bound(&self, node: &Node, state: &AuthState) -> Vec<(WriterId, Seq)> {
        let anchor = &node.record.anchor_device_id;
        let key = state.devices.get(anchor).map(|device| device.device_key);
        node.carriers
            .iter()
            .filter(|carrier| {
                !self.check_binding
                    || self.certificates.get(&carrier.writer).is_some_and(|names| {
                        names
                            .iter()
                            .any(|(device, named)| device == anchor && Some(*named) == key)
                    })
            })
            .map(|carrier| (carrier.writer, carrier.seq))
            .collect()
    }
}

/// One check-valid entry of the tree, or the charter at index 0.
pub(super) struct Visit {
    /// The stored entry, `None` for the charter.
    pub(super) node: Option<usize>,
    /// Index of the parent visit, `None` for the charter.
    pub(super) parent: Option<usize>,
    /// Index of the visit that opened the entry's epoch instance: the charter, or the entry
    /// itself when it is a handover.
    pub(super) instance: usize,
    /// Entry hash, the charter hash for the charter.
    pub(super) hash: Digest32,
    /// Rank of the state after the entry.
    pub(super) rank: Rank,
    /// Revision of the entry, 0 for the charter.
    pub(super) revision: u64,
    /// Key that signed the entry's proposal, `None` for the charter.
    pub(super) author: Option<AdminPublicKey>,
    /// Signer kind and target device when the entry is a handover.
    pub(super) handover: Option<(Signer, String)>,
    /// Admin key of the anchor at the parent, if the anchor is a live admin there.
    pub(super) parent_anchor_key: Option<AdminPublicKey>,
    /// Revocations the entry carries.
    revocations: Vec<Revocation>,
    /// Hash of the handover a yield names.
    pub(super) yields_to: Option<Digest32>,
    /// Device a removal removes.
    pub(super) removes: Option<String>,
    /// Kit key of the state after the entry.
    pub(super) kit_key: AdminPublicKey,
    /// Writers and sequences of the envelopes that carry the entry bound to its anchor device
    /// at the parent, empty for the charter.
    pub(super) bound: Vec<(WriterId, Seq)>,
}

/// The tree of check-valid stored entries from the charter, see [`Resolution`].
pub(super) struct Walk<'a> {
    pub(super) stored: &'a Stored,
    pub(super) vetoes: Arc<Vetoes>,
    pub(super) visits: Vec<Visit>,
    /// Child visits of each visit.
    pub(super) children: Vec<Vec<usize>>,
    /// Device ids and keys any check-valid chain enrolls.
    enrolled: EnrolledSet,
    /// Tour positions of each visit, see [`tour_spans`].
    spans: Vec<(usize, usize)>,
}

/// Enter and leave positions of each node in a depth-first tour from node 0, so that one node is
/// an ancestor-or-self of another exactly when its span contains the other's. Ancestry checks
/// run once per visit, and a span keeps each one constant however long the chain grows.
pub(crate) fn tour_spans(children: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let mut spans = vec![(0, 0); children.len()];
    let mut position = 0;
    let mut stack = vec![(0, false)];
    while let Some((node, left)) = stack.pop() {
        position += 1;
        if left {
            spans[node].1 = position;
            continue;
        }
        spans[node].0 = position;
        stack.push((node, true));
        stack.extend(children[node].iter().map(|child| (*child, false)));
    }
    spans
}

impl<'a> Walk<'a> {
    pub(super) fn new(stored: &'a Stored, vetoes: Arc<Vetoes>) -> Self {
        let mut by_parent: BTreeMap<Digest32, Vec<usize>> = BTreeMap::new();
        for (index, node) in stored.nodes.iter().enumerate() {
            by_parent
                .entry(node.record.previous_entry_hash)
                .or_default()
                .push(index);
        }
        let root = AuthState::from_charter(&stored.charter, Arc::clone(&vetoes));
        let mut walk = Self {
            stored,
            vetoes,
            visits: vec![Visit {
                node: None,
                parent: None,
                instance: 0,
                hash: root.entry_hash,
                rank: root.rank(),
                revision: 0,
                author: None,
                handover: None,
                parent_anchor_key: None,
                revocations: Vec::new(),
                yields_to: None,
                removes: None,
                kit_key: root.kit_key,
                bound: Vec::new(),
            }],
            children: vec![Vec::new()],
            enrolled: BTreeSet::new(),
            spans: Vec::new(),
        };
        // Entries name their parent by hash and duplicates are dropped, so the walk visits each
        // entry at most once, and a visit always comes after its parent. Only forks copy the
        // state, so a chain without forks is walked in linear time.
        let mut stack = vec![(root, 0)];
        while let Some((state, visit)) = stack.pop() {
            // A child is valid when its rules hold at the parent and a certificate of one of its
            // carriers binds it to the key its anchor device holds there.
            let mut valid: Vec<(usize, Vec<(WriterId, Seq)>)> = by_parent
                .get(&state.entry_hash)
                .into_iter()
                .flatten()
                .copied()
                .filter(|index| state.check(&stored.nodes[*index].record).is_ok())
                .map(|index| (index, stored.bound(&stored.nodes[index], &state)))
                .filter(|(_, bound)| !bound.is_empty())
                .collect();
            let Some((last, bound)) = valid.pop() else {
                continue;
            };
            for (index, bound) in valid {
                stack.push(walk.visit(visit, index, bound, state.clone()));
            }
            stack.push(walk.visit(visit, last, bound, state));
        }
        walk.spans = tour_spans(&walk.children);
        walk
    }

    /// Records entry `index`, which `state` admits, as a child of `parent`, and returns the
    /// state after it with its visit.
    fn visit(
        &mut self,
        parent: usize,
        index: usize,
        bound: Vec<(WriterId, Seq)>,
        mut state: AuthState,
    ) -> (AuthState, usize) {
        let stored = self.stored;
        let node = &stored.nodes[index];
        let record = &node.record;
        let entry = &record.proposal.proposal.entry;
        if let Entry::Enroll(enroll) = entry {
            self.enrolled
                .insert((enroll.device_id.clone(), enroll.device_key));
        }
        let handover = match entry {
            Entry::Handover(handover) => state
                .signer(record)
                .map(|signer| (signer, handover.new_anchor_device_id.clone())),
            _ => None,
        };
        let yields_to = match entry {
            Entry::Yield(give_way) => Some(give_way.handover_hash),
            _ => None,
        };
        let removes = match entry {
            Entry::Remove(remove) => Some(remove.device_id.clone()),
            _ => None,
        };
        let parent_anchor_key = state.anchor_admin_key();
        let revocations = state.revocations(record);
        state.advance(record, node.hash);
        let visit = self.visits.len();
        let instance = if handover.is_some() {
            visit
        } else {
            self.visits[parent].instance
        };
        self.visits.push(Visit {
            node: Some(index),
            parent: Some(parent),
            instance,
            hash: node.hash,
            rank: state.rank(),
            revision: state.revision,
            author: Some(record.proposal.proposal.author),
            handover,
            parent_anchor_key,
            revocations,
            yields_to,
            removes,
            kit_key: state.kit_key,
            bound,
        });
        self.children.push(Vec::new());
        self.children[parent].push(visit);
        (state, visit)
    }

    pub(super) fn parent(&self, visit: usize) -> usize {
        self.visits[visit].parent.unwrap_or(0)
    }

    pub(super) fn signer(&self, visit: usize) -> Option<Signer> {
        self.visits[visit]
            .handover
            .as_ref()
            .map(|(signer, _)| *signer)
    }

    /// Whether `ancestor` is `visit` or one of its ancestors.
    pub(super) fn is_ancestor(&self, ancestor: usize, visit: usize) -> bool {
        let (outer, inner) = (self.spans[ancestor], self.spans[visit]);
        outer.0 <= inner.0 && inner.1 <= outer.1
    }

    /// The deepest common ancestor-or-self of `a` and `b`.
    fn common_ancestor(&self, mut a: usize, mut b: usize) -> usize {
        while a != b {
            let (here, there) = (self.visits[a].revision, self.visits[b].revision);
            if here >= there {
                a = self.parent(a);
            }
            if there >= here {
                b = self.parent(b);
            }
        }
        a
    }

    /// The rotation vetoes this walk's rotations cast, see [`Resolution`].
    fn vetoes(&self) -> Vetoes {
        let mut rotations: BTreeMap<AdminPublicKey, Vec<usize>> = BTreeMap::new();
        for (index, visit) in self.visits.iter().enumerate() {
            for revocation in &visit.revocations {
                if let Revocation::KitRotation(key) = revocation {
                    rotations.entry(*key).or_default().push(index);
                }
            }
        }
        let conflicted: BTreeSet<AdminPublicKey> = rotations
            .iter()
            .filter(|(_, members)| {
                members.iter().any(|a| {
                    members
                        .iter()
                        .any(|b| !self.is_ancestor(*a, *b) && !self.is_ancestor(*b, *a))
                })
            })
            .map(|(key, _)| *key)
            .collect();
        let kit_handovers = (0..self.visits.len())
            .filter(|index| self.signer(*index) == Some(Signer::Kit))
            .filter(|index| {
                self.visits[*index].author.is_some_and(|key| {
                    conflicted.contains(&key)
                        || rotations.get(&key).is_some_and(|members| {
                            members
                                .iter()
                                .any(|rotation| !self.is_ancestor(*index, *rotation))
                        })
                })
            })
            .map(|index| self.visits[index].hash)
            .collect();
        Vetoes {
            conflicted,
            kit_handovers,
        }
    }

    /// The winner of each instance a pin source passes through: the handover out of it on the
    /// ancestry of the highest ranked kit handover no rotation vetoed.
    fn pinned(&self) -> BTreeMap<usize, usize> {
        let mut sources: Vec<usize> = (0..self.visits.len())
            .filter(|index| {
                self.signer(*index) == Some(Signer::Kit)
                    && !self
                        .vetoes
                        .kit_handovers
                        .contains(&self.visits[*index].hash)
            })
            .collect();
        sources.sort_by(|a, b| self.visits[*b].rank.cmp(&self.visits[*a].rank));
        let mut winners = BTreeMap::new();
        for source in sources {
            let mut at = source;
            while let Some(parent) = self.visits[at].parent {
                if self.visits[at].handover.is_some() {
                    winners.entry(self.visits[parent].instance).or_insert(at);
                }
                at = parent;
            }
        }
        winners
    }

    /// Entries of the parent's instance that revoke the signer key or the target of
    /// `handover`, after its parent and not through it.
    pub(super) fn objections(&self, handover: usize) -> Vec<usize> {
        let visit = &self.visits[handover];
        let Some((_, target)) = &visit.handover else {
            return Vec::new();
        };
        let revokes = |revocation: &Revocation| match revocation {
            Revocation::Device(device) => device == target,
            Revocation::Key(key) => Some(*key) == visit.author,
            Revocation::KitRotation(_) => false,
        };
        let mut found = Vec::new();
        let mut stack: Vec<usize> = self.children[self.parent(handover)]
            .iter()
            .copied()
            .filter(|child| *child != handover)
            .collect();
        while let Some(at) = stack.pop() {
            // A handover opens another instance, whose entries object to nothing here.
            if self.visits[at].handover.is_some() {
                continue;
            }
            if self.visits[at].revocations.iter().any(revokes) {
                found.push(at);
            }
            stack.extend(self.children[at].iter().copied());
        }
        found
    }

    /// The author key and named handover hash of every stored yield.
    pub(super) fn yields(&self) -> Vec<(AdminPublicKey, Digest32)> {
        self.visits
            .iter()
            .filter_map(|visit| visit.author.zip(visit.yields_to))
            .collect()
    }

    /// The freeze node of each writer fork of every instance, by instance root.
    ///
    /// A writer with provable fork evidence that carries a bound entry of an instance below its
    /// first provable sequence forks the instance: its freeze node is the visit of the instance
    /// it carries at the highest such sequence, the common ancestor of several at one sequence.
    /// Every bound entry of an instance is carried by a writer of the instance's anchor, since
    /// its certificate names the anchor device. A writer without such an entry forks nothing.
    fn freeze_nodes(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut highest: BTreeMap<(usize, WriterId), (Seq, usize)> = BTreeMap::new();
        for (index, visit) in self.visits.iter().enumerate() {
            for (writer, seq) in &visit.bound {
                let Some(first) = self.stored.provable.get(writer) else {
                    continue;
                };
                if seq >= first {
                    continue;
                }
                highest
                    .entry((visit.instance, *writer))
                    .and_modify(|(at, node)| {
                        if seq > at {
                            *at = *seq;
                            *node = index;
                        } else if seq == at {
                            *node = self.common_ancestor(*node, index);
                        }
                    })
                    .or_insert((*seq, index));
            }
        }
        let mut nodes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for ((instance, _), (_, node)) in highest {
            nodes.entry(instance).or_default().push(node);
        }
        nodes
    }

    /// The chain from the charter through `visit`.
    pub(super) fn selection(&self, visit: usize) -> Selection {
        let mut path = Vec::new();
        let mut at = Some(visit);
        while let Some(index) = at {
            path.push(index);
            at = self.visits[index].parent;
        }
        path.reverse();
        let mut selection = Selection {
            state: AuthState::from_charter(&self.stored.charter, Arc::clone(&self.vetoes)),
            path: Vec::new(),
            resolutions: Vec::new(),
            reachable: 0,
            held: BTreeSet::new(),
            handed_over: false,
            key_epochs: Vec::new(),
        };
        for node in path.iter().filter_map(|index| self.visits[*index].node) {
            selection = selection.extended(&self.stored.nodes[node]);
        }
        selection
    }
}

/// How an epoch instance resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    /// No handover leaves the instance and no writer fork freezes it.
    Open,
    /// The handover out of the instance that wins it.
    Won(usize),
    /// Contested without a winner, or forked by a writer without a candidate: only the visit
    /// and its ancestors are effective.
    FrozenAt(usize),
}

/// The authority the stored tree derives, all of it before the head is selected.
///
/// Two walks build the tree of check-valid entries from the charter. The first collects every
/// rotation away from a kit key that the key signed; two rotations away from one key, neither an
/// ancestor of the other, refuse each other, and a kit handover that is not an ancestor of every
/// rotation away from its key, or whose key is in such a conflict, keeps its validity and rule K
/// but neither pins nor counts in rank. The second walk applies those vetoes.
///
/// Each epoch instance, identified by the entry that opened it, has the handovers out of it as
/// candidates. An entry of the instance after a candidate's parent, not through the candidate,
/// that revokes its signer key or its target objects to it. The instance is contested when it
/// has two candidates or an objected one, and resolves in this order: the candidate on the
/// ancestry of the highest ranked unvetoed kit handover wins; a lone unobjected candidate wins;
/// a candidate every other contestant and, for objections, the instance's anchor yielded to wins,
/// the lowest hash among several; otherwise a contested instance freezes at the common ancestor
/// of the candidates' parents, lowered to the common ancestor with the freeze node of each writer
/// fork of the instance, and an uncontested one with a writer fork freezes at the common ancestor
/// of its freeze nodes.
///
/// An entry is effective when its parent is, it is not a handover that lost or left a frozen
/// instance, it is not off the path to a freeze point, and in an instance whose winner has a
/// writer fork, it is an ancestor of the winner or of every freeze node. The effective entry of
/// highest rank is the head. Nothing reads the selected chain afterwards to change any of this:
/// the head reports the state its ancestry derives, the freeze point, and the devices held for
/// data. The result depends only on the stored envelope set, not on arrival order, the peer that
/// delivered an envelope, or the operation slots a replica kept.
pub(super) struct Resolution<'a> {
    pub(super) walk: Walk<'a>,
    /// Handovers out of each instance, by instance root.
    pub(super) candidates: BTreeMap<usize, Vec<usize>>,
    /// The winner a pin source names for each instance it passes through, by instance root.
    pub(super) pinned: BTreeMap<usize, usize>,
    /// Outcome of each instance, by instance root.
    pub(super) outcomes: BTreeMap<usize, Outcome>,
    /// Whether each visit is effective.
    pub(super) effective: Vec<bool>,
    /// The effective visit of highest rank.
    pub(super) best: usize,
    /// Highest revision the walk without vetoes reaches from the charter, or every revision once
    /// a writer fork freezes an instance.
    reachable: u64,
}

impl<'a> Resolution<'a> {
    pub(super) fn new(stored: &'a Stored) -> Self {
        let first = Walk::new(stored, Arc::default());
        // Vetoes only refuse entries, so the walk without them reaches every revision an
        // operation can be sealed against, superseded chains included.
        let walked = first
            .visits
            .iter()
            .map(|visit| visit.revision)
            .max()
            .unwrap_or(0);
        let vetoes = first.vetoes();
        let walk = if vetoes == Vetoes::default() {
            first
        } else {
            Walk::new(stored, Arc::new(vetoes))
        };
        let mut candidates: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for index in 1..walk.visits.len() {
            if walk.visits[index].handover.is_some() {
                let instance = walk.visits[walk.parent(index)].instance;
                candidates.entry(instance).or_default().push(index);
            }
        }
        let pinned = walk.pinned();
        let yields = walk.yields();
        let forks = walk.freeze_nodes();
        let mut outcomes = BTreeMap::new();
        for root in 0..walk.visits.len() {
            if walk.visits[root].instance != root {
                continue;
            }
            let own = candidates.get(&root).map_or(&[][..], Vec::as_slice);
            let freezes = forks.get(&root).map_or(&[][..], Vec::as_slice);
            outcomes.insert(
                root,
                Self::outcome(&walk, root, own, freezes, &pinned, &yields),
            );
        }
        // Copies of a forked anchor writer may carry entries past its freeze node that no
        // replica walks. An author that applied one before the fork was found must not wait for
        // it, so a writer fork makes every revision reachable. Each operation's device is still
        // checked against the selected chain when it applies.
        let reachable = if forks.is_empty() {
            walked
        } else {
            UNBOUNDED_REVISION
        };
        let mut effective = vec![true; walk.visits.len()];
        for index in 1..walk.visits.len() {
            effective[index] =
                effective[walk.parent(index)] && Self::admitted(&walk, index, &outcomes, &forks);
        }
        let best = (0..walk.visits.len())
            .filter(|index| effective[*index])
            .max_by(|a, b| walk.visits[*a].rank.cmp(&walk.visits[*b].rank))
            .unwrap_or(0);
        Self {
            walk,
            candidates,
            pinned,
            outcomes,
            effective,
            best,
            reachable,
        }
    }

    /// How the instance opened at `root` resolves, see [`Resolution`].
    fn outcome(
        walk: &Walk<'_>,
        root: usize,
        candidates: &[usize],
        freezes: &[usize],
        pinned: &BTreeMap<usize, usize>,
        yields: &[(AdminPublicKey, Digest32)],
    ) -> Outcome {
        if let Some(winner) = pinned.get(&root) {
            return Outcome::Won(*winner);
        }
        let Some(first) = candidates.first() else {
            return freezes
                .iter()
                .copied()
                .reduce(|a, b| walk.common_ancestor(a, b))
                .map_or(Outcome::Open, Outcome::FrozenAt);
        };
        let objections: Vec<Vec<usize>> = candidates
            .iter()
            .map(|candidate| walk.objections(*candidate))
            .collect();
        if candidates.len() == 1 && objections.iter().all(Vec::is_empty) {
            return Outcome::Won(*first);
        }
        let accepted = candidates
            .iter()
            .zip(&objections)
            .filter(|(candidate, objected)| {
                let hash = walk.visits[**candidate].hash;
                let gave_way = |key: Option<AdminPublicKey>| {
                    key.is_some_and(|key| {
                        yields
                            .iter()
                            .any(|(author, named)| *author == key && *named == hash)
                    })
                };
                candidates
                    .iter()
                    .all(|other| other == *candidate || gave_way(walk.visits[*other].author))
                    && (objected.is_empty() || gave_way(walk.visits[**candidate].parent_anchor_key))
            })
            .map(|(candidate, _)| *candidate)
            .min_by_key(|candidate| walk.visits[*candidate].hash);
        if let Some(winner) = accepted {
            return Outcome::Won(winner);
        }
        let point = candidates
            .iter()
            .map(|candidate| walk.parent(*candidate))
            .chain(freezes.iter().copied())
            .reduce(|a, b| walk.common_ancestor(a, b))
            .unwrap_or(root);
        Outcome::FrozenAt(point)
    }

    /// Whether visit `index`, whose parent is effective, is effective itself.
    fn admitted(
        walk: &Walk<'_>,
        index: usize,
        outcomes: &BTreeMap<usize, Outcome>,
        forks: &BTreeMap<usize, Vec<usize>>,
    ) -> bool {
        let visit = &walk.visits[index];
        if visit.handover.is_some() {
            let out_of = walk.visits[walk.parent(index)].instance;
            if outcomes.get(&out_of) != Some(&Outcome::Won(index)) {
                return false;
            }
        }
        match outcomes.get(&visit.instance) {
            Some(Outcome::FrozenAt(point)) => walk.is_ancestor(index, *point),
            Some(Outcome::Won(winner)) => {
                walk.is_ancestor(index, *winner)
                    || forks.get(&visit.instance).is_none_or(|freezes| {
                        freezes
                            .iter()
                            .all(|freeze| walk.is_ancestor(index, *freeze))
                    })
            }
            Some(Outcome::Open) | None => true,
        }
    }

    /// Revision past which the head's instance is frozen, if it is.
    fn frozen_at(&self) -> Option<u64> {
        match self.outcomes.get(&self.walk.visits[self.best].instance) {
            Some(Outcome::FrozenAt(point)) => Some(self.walk.visits[*point].revision),
            _ => None,
        }
    }

    /// Devices a stored entry that is not effective removes after the freeze point of the
    /// head's instance, while it is frozen.
    fn held(&self) -> BTreeSet<String> {
        let instance = self.walk.visits[self.best].instance;
        match self.outcomes.get(&instance) {
            Some(Outcome::FrozenAt(point)) => self.held_under(*point),
            _ => BTreeSet::new(),
        }
    }

    /// The selected chain, with the freeze, the hold, and the handovers of the head's instance.
    fn selected(&self) -> Selection {
        let mut selection = self.walk.selection(self.best);
        selection.state.frozen_at = self.frozen_at();
        selection.held = self.held();
        selection.handed_over = self
            .candidates
            .get(&self.walk.visits[self.best].instance)
            .is_some_and(|candidates| !candidates.is_empty());
        selection.reachable = self.reachable;
        selection
    }

    /// Whether a visit off the selected path proposes `proposal` and a later visit of the same
    /// branch reverses it. Only check-valid visits count, so a record outside the tree reverses
    /// nothing. One pass over the subtrees off the path, each visit once.
    fn reversed_off_path(&self, proposal: &SignedProposal) -> bool {
        self.reversal_walk(proposal, &mut ())
    }

    /// [`Self::reversed_off_path`], reporting its work to `tally`.
    fn reversal_walk(&self, proposal: &SignedProposal, tally: &mut impl ReversalTally) -> bool {
        let target = proposal.hash();
        let entry = &proposal.proposal.entry;
        let visits = &self.walk.visits;
        let mut on_path = vec![false; visits.len()];
        let mut at = Some(self.best);
        while let Some(index) = at {
            on_path[index] = true;
            at = visits[index].parent;
        }
        // Each pending visit carries whether an off-path ancestor proposes `proposal`.
        let mut stack: Vec<(usize, bool)> = (0..visits.len())
            .filter(|index| on_path[*index])
            .flat_map(|index| self.walk.children[index].iter().copied())
            .filter(|child| !on_path[*child])
            .map(|child| (child, false))
            .collect();
        while let Some((at, below)) = stack.pop() {
            tally.popped();
            let Some(node) = visits[at].node else {
                continue;
            };
            let later = &self.walk.stored.nodes[node].record.proposal;
            if below {
                tally.compared();
                if reverses(&later.proposal.entry, entry) {
                    return true;
                }
            }
            let below = below || later.hash() == target;
            stack.extend(self.walk.children[at].iter().map(|child| (*child, below)));
        }
        false
    }

    /// The highest ranked visit in the subtree of `start` where `signer` is the kit key and
    /// `target` a live desktop, staying in `start`'s instance when `within` is set.
    fn kit_point_under(
        &self,
        start: usize,
        within: bool,
        signer: &AdminPublicKey,
        target: &str,
    ) -> Option<usize> {
        let instance = self.walk.visits[start].instance;
        let mut subtree = Vec::new();
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            if within && self.walk.visits[at].instance != instance {
                continue;
            }
            if self.walk.visits[at].kit_key == *signer {
                subtree.push(at);
            }
            stack.extend(self.walk.children[at].iter().copied());
        }
        subtree.sort_by(|a, b| self.walk.visits[*b].rank.cmp(&self.walk.visits[*a].rank));
        subtree
            .into_iter()
            .find(|visit| self.admits_kit_target(*visit, target))
    }

    fn admits_kit_target(&self, visit: usize, target: &str) -> bool {
        let state = self.walk.selection(visit).state;
        state.epoch < u32::MAX
            && state
                .live_device(target)
                .is_ok_and(|device| device.kind.may_be_admin())
    }

    /// Visits a kit handover `signer` signs for the desktop `target` may continue from, the
    /// selected head first: when the head's instance is frozen, also the best entry under each
    /// handover out of it and on each branch of its own continuation past the freeze point.
    pub(super) fn kit_points(&self, signer: &AdminPublicKey, target: &str) -> Vec<usize> {
        let mut points = Vec::new();
        if self.walk.visits[self.best].kit_key == *signer
            && self.admits_kit_target(self.best, target)
        {
            points.push(self.best);
        }
        let instance = self.walk.visits[self.best].instance;
        let continuations: Vec<usize> = match self.outcomes.get(&instance) {
            Some(Outcome::FrozenAt(point)) => self.walk.children[*point].clone(),
            _ => return points,
        };
        let branches = self
            .candidates
            .get(&instance)
            .into_iter()
            .flatten()
            .map(|candidate| (*candidate, false))
            .chain(
                continuations
                    .into_iter()
                    .filter(|visit| self.walk.visits[*visit].handover.is_none())
                    .map(|visit| (visit, true)),
            );
        for (start, within) in branches {
            if let Some(point) = self.kit_point_under(start, within, signer, target)
                && !points.contains(&point)
            {
                points.push(point);
            }
        }
        points
    }
}

/// Selects the chain from the charter through the stored entries, or `None` without a charter.
/// See [`Resolution`] for the rule.
pub(crate) async fn select(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<Selection>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    Ok(Some(Resolution::new(&stored).selected()))
}

/// [`select`] for an anchor about to sequence `proposal`, with whether a stored branch off the
/// selected chain proposed it and a later entry of that branch reversed it, `false` without a
/// proposal to check.
pub(super) async fn select_for_sequencing(
    conn: &mut SqliteConnection,
    space: SpaceId,
    proposal: Option<&SignedProposal>,
) -> SyncResult<Option<(Selection, bool)>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    let resolution = Resolution::new(&stored);
    let reversed = proposal.is_some_and(|proposal| resolution.reversed_off_path(proposal));
    Ok(Some((resolution.selected(), reversed)))
}

/// Whether any check-valid stored chain enrolls `device_id` with `device_key`, selected or not.
/// A replica that selected one branch still stores the writers of another, whose entries may
/// contest its selection. This admits storage only, never authority: selection and enrollment
/// read the selected chain. The walk is cached per stored entry set and freezes, so a batch of
/// geneses walks the tree once.
pub(crate) async fn enrolled_on_any_chain(
    conn: &mut SqliteConnection,
    cache: &EnrolledCache,
    space: SpaceId,
    device_id: &str,
    device_key: &DevicePublicKey,
) -> SyncResult<bool> {
    let key = EnrolledKey::load(conn, space).await?;
    let wanted = (device_id.to_string(), *device_key);
    if let Some(hit) = cache.lookup(&key) {
        return Ok(hit.contains(&wanted));
    }
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(false);
    };
    let enrolled = Arc::new(Walk::new(&stored, Arc::default()).enrolled);
    cache.store(key, Arc::clone(&enrolled));
    Ok(enrolled.contains(&wanted))
}

/// What the enrollments of every stored chain depend on: the space, its charter, and the
/// envelopes of its record store with whether each is verified. The charter keeps a reset space
/// that a new charter founds, or another vault with the same space, from reading a stale answer.
#[derive(Debug, PartialEq, Eq)]
struct EnrolledKey {
    space: SpaceId,
    charter: Option<Digest32>,
    envelopes: Vec<(Vec<u8>, i64)>,
}

impl EnrolledKey {
    async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        let envelopes = sqlx::query_as(
            "SELECT envelope_hash, verified FROM sync_auth_envelopes
             WHERE space_id = ? ORDER BY envelope_hash, role",
        )
        .bind(space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
        Ok(Self {
            space,
            charter: load_charter(conn, space)
                .await?
                .map(|charter| charter.hash()),
            envelopes,
        })
    }
}

/// The device ids and keys any check-valid chain enrolls, kept for the last stored entry set an
/// engine walked. The key is the stored content, so a rolled back transaction cannot leave a
/// stale answer behind.
#[derive(Debug, Default)]
pub(crate) struct EnrolledCache(Mutex<Option<(EnrolledKey, Arc<EnrolledSet>)>>);

type EnrolledSet = BTreeSet<(String, DevicePublicKey)>;

impl EnrolledCache {
    fn lookup(&self, key: &EnrolledKey) -> Option<Arc<EnrolledSet>> {
        let slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref()
            .filter(|(cached, _)| cached == key)
            .map(|(_, enrolled)| Arc::clone(enrolled))
    }

    fn store(&self, key: EnrolledKey, enrolled: Arc<EnrolledSet>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((key, enrolled));
    }
}

/// The state after the stored entry at `revision` with `entry_hash` when it is one of the
/// points a kit handover `author` signs for `target` may continue from, see
/// [`Resolution::kit_points`].
pub(super) async fn kit_point(
    conn: &mut SqliteConnection,
    space: SpaceId,
    author: &AdminPublicKey,
    target: &str,
    revision: u64,
    entry_hash: &Digest32,
) -> SyncResult<Option<AuthState>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    let resolution = Resolution::new(&stored);
    Ok(resolution
        .kit_points(author, target)
        .into_iter()
        .find(|visit| {
            let visit = &resolution.walk.visits[*visit];
            visit.hash == *entry_hash && visit.revision == revision
        })
        .map(|visit| resolution.walk.selection(visit).state))
}

/// Where a handover that `signer` signs for the desktop `target` continues from: the first kit
/// point, see [`Resolution::kit_points`], or else the selected head. `None` without a charter.
pub(super) async fn handover_point(
    conn: &mut SqliteConnection,
    space: SpaceId,
    signer: &AdminPublicKey,
    target: &str,
) -> SyncResult<Option<HandoverPoint>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    let resolution = Resolution::new(&stored);
    if let Some(point) = resolution.kit_points(signer, target).first() {
        let state = resolution.walk.selection(*point).state;
        return Ok(Some(HandoverPoint::of(&state, true)));
    }
    let state = resolution.selected().state;
    let by_kit = state.kit_key == *signer;
    Ok(Some(HandoverPoint::of(&state, by_kit)))
}

/// Where a handover that `signer` signs for the desktop `target` continues from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverPoint {
    /// Epoch of the entry, which the handover follows with the next one.
    pub epoch: u32,
    /// Revision of the entry.
    pub revision: u64,
    /// Hash of the entry.
    pub entry_hash: Digest32,
    /// Anchor of the entry's epoch.
    pub anchor_device_id: String,
    /// Whether `signer` is the kit key there.
    pub by_kit: bool,
    /// The authorization state after the entry, derived from verified stored records only, so a
    /// handover can check what it continues from, such as its key epoch and the encryption keys
    /// published there, rather than the selected head.
    pub view: AuthorizationView,
}

impl HandoverPoint {
    pub(super) fn of(state: &AuthState, by_kit: bool) -> Self {
        Self {
            epoch: state.epoch,
            revision: state.revision,
            entry_hash: state.entry_hash,
            anchor_device_id: state.anchor_device_id.clone(),
            by_kit,
            view: state.view(),
        }
    }
}

/// What the selection rule derives from stored entries, for its tests.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedHead {
    /// Hash of the selected head.
    pub(crate) head: Digest32,
    /// Highest revision any stored chain reaches.
    pub(crate) reachable: u64,
    /// Revision past which the selected epoch is frozen.
    pub(crate) frozen_at: Option<u64>,
    /// Anchor of the selected epoch.
    pub(crate) anchor: String,
    /// Revision that removed each enrolled device, by device id, `None` for a live one.
    pub(crate) devices: BTreeMap<String, Option<u64>>,
    /// Live devices with an admin role.
    pub(crate) admins: BTreeSet<String>,
    /// Devices held for data while the head's instance is frozen.
    pub(crate) held: BTreeSet<String>,
    /// Unvetoed kit handovers on the selected chain.
    pub(crate) kit_handovers: u32,
    /// The recovery key the head names.
    pub(crate) recovery_admin_key: AdminPublicKey,
}

/// What the stored `records` derive from `charter`, each carried by its writer at its sequence,
/// for tests of the selection rule. `forks` names the first provable fork sequence of forked
/// writers. With `certificates`, the device id and key each certificate of a writer names, a
/// carrier is bound as in the record store; without, every carrier counts as bound. A record
/// of a forked writer at or past its fork sequence is a node only as an ancestor of another,
/// as in a record store whose horizon equals the provable sequence.
#[cfg(test)]
pub(crate) fn select_records(
    charter: &SignedCharter,
    forks: BTreeMap<WriterId, Seq>,
    certificates: Option<BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>>,
    records: &[(WriterId, Seq, AuthorizationRecord)],
) -> SelectedHead {
    let stored = stored_records(charter, forks, certificates, records);
    let selection = Resolution::new(&stored).selected();
    let state = &selection.state;
    SelectedHead {
        head: state.entry_hash,
        reachable: selection.reachable,
        frozen_at: state.frozen_at,
        anchor: state.anchor_device_id.clone(),
        devices: state
            .devices
            .iter()
            .map(|(id, device)| (id.clone(), device.removed))
            .collect(),
        admins: state
            .devices
            .iter()
            .filter(|(_, device)| {
                device.removed.is_none() && matches!(device.role, DeviceRole::Admin(_))
            })
            .map(|(id, _)| id.clone())
            .collect(),
        held: selection.held.clone(),
        kit_handovers: state.kit_handovers,
        recovery_admin_key: state.recovery_admin_key,
    }
}

/// How the anchor's reversal check answers for `proposal` over the stored `records`, carried as
/// in [`select_records`] with every carrier bound and no fork.
#[cfg(test)]
pub(crate) struct ReversalCheck {
    /// Whether a stored branch off the selected chain proposed it and later reversed it.
    pub(crate) reversed: bool,
    /// Visits in the walked tree.
    pub(crate) visits: usize,
    /// Visits the check took from its pending stack.
    pub(crate) popped: usize,
    /// Later entries the check compared with the proposal.
    pub(crate) compared: usize,
}

#[cfg(test)]
impl ReversalTally for ReversalCheck {
    fn popped(&mut self) {
        self.popped += 1;
    }

    fn compared(&mut self) {
        self.compared += 1;
    }
}

/// The anchor's reversal check of `proposal` over `records`, see [`ReversalCheck`].
#[cfg(test)]
pub(crate) fn reversal_in_records(
    charter: &SignedCharter,
    records: &[(WriterId, Seq, AuthorizationRecord)],
    proposal: &SignedProposal,
) -> ReversalCheck {
    let stored = stored_records(charter, BTreeMap::new(), None, records);
    let resolution = Resolution::new(&stored);
    let mut check = ReversalCheck {
        reversed: false,
        visits: resolution.walk.visits.len(),
        popped: 0,
        compared: 0,
    };
    check.reversed = resolution.reversal_walk(proposal, &mut check);
    check
}

/// The stored tree of `records`, see [`select_records`].
#[cfg(test)]
fn stored_records(
    charter: &SignedCharter,
    forks: BTreeMap<WriterId, Seq>,
    certificates: Option<BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>>,
    records: &[(WriterId, Seq, AuthorizationRecord)],
) -> Stored {
    let mut bodies: BTreeMap<Digest32, Node> = BTreeMap::new();
    for (writer, seq, record) in records {
        let hash = record.hash();
        bodies
            .entry(hash)
            .or_insert_with(|| Node {
                hash,
                record: record.clone(),
                carriers: Vec::new(),
            })
            .carriers
            .push(records::Carrier {
                writer: *writer,
                seq: *seq,
                envelope_hash: ganbaru_sync_contracts::signing::domain_digest(
                    b"test carrier",
                    &[writer.as_bytes(), &seq.to_be_bytes()],
                ),
            });
    }
    let charter_hash = charter.hash();
    let anchors: Vec<Digest32> = bodies
        .values()
        .filter(|node| {
            node.carriers.iter().any(|carrier| {
                forks
                    .get(&carrier.writer)
                    .is_none_or(|fork| carrier.seq < *fork)
            })
        })
        .map(|node| node.hash)
        .collect();
    let mut included = BTreeSet::new();
    for hash in anchors {
        let mut path = Vec::new();
        let mut at = hash;
        while at != charter_hash && !included.contains(&at) {
            let Some(node) = bodies.get(&at) else {
                path.clear();
                break;
            };
            path.push(at);
            at = node.record.previous_entry_hash;
        }
        included.extend(path);
    }
    let mut nodes: Vec<Node> = bodies
        .into_values()
        .filter(|node| included.contains(&node.hash))
        .collect();
    nodes.sort_by_key(|node| (node.record.revision, node.hash));
    Stored {
        charter: charter.clone(),
        nodes,
        check_binding: certificates.is_some(),
        certificates: certificates.unwrap_or_default(),
        provable: forks,
        others: Vec::new(),
    }
}
