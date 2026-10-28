//! Handover selection at a fork point: the trust order between a kit-lineage recovery
//! handover, the anchor's continuation, and a device admin handover, and the vetoes the
//! continuation's revocations cast. Each case selects the same head whatever order its entries
//! arrive in.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, DeviceRole, Digest32, Entry, Handover, Remove, Sequencing,
    SetRole, SignedCharter, SpaceId, WriterId,
};

use super::authorization::admin_entry;
use super::replica::{
    ANCHOR_DEVICE_ID, START_MS, anchor_admin, device_admin, device_id, enroll_entry, person,
    proposal, recovery_admin, test_charter,
};
use crate::authorization::{SelectedHead, select_records};

/// A position in a test log: the head an entry continues from.
#[derive(Clone)]
struct At {
    epoch: u32,
    revision: u64,
    hash: Digest32,
    anchor: String,
}

/// Records of a test space, built by hand so a case can store any fork.
struct Log {
    space: SpaceId,
    charter: SignedCharter,
    records: Vec<AuthorizationRecord>,
    /// The writer that carries each record.
    writers: Vec<WriterId>,
    /// The writer that carries the next records.
    writer: WriterId,
    /// Revisions at which forks freeze epochs, by anchor device.
    freezes: BTreeMap<String, Vec<u64>>,
    clock: u64,
}

/// Admin desktops 1 and 2 and member desktop 3.
const FIRST: usize = 1;
const SECOND: usize = 2;
const MEMBER: usize = 3;

impl Log {
    /// The charter, then the enrollment of [`FIRST`] and [`SECOND`] as admins and [`MEMBER`]
    /// as a member. Returns the log and its head.
    fn new() -> (Self, At) {
        let space = SpaceId::from_bytes([41; 16]);
        let charter = test_charter(&person(), space);
        let root = At {
            epoch: 0,
            revision: 0,
            hash: charter.hash(),
            anchor: ANCHOR_DEVICE_ID.to_owned(),
        };
        let mut log = Self {
            space,
            charter,
            records: Vec::new(),
            writers: Vec::new(),
            writer: writer(0),
            freezes: BTreeMap::new(),
            clock: START_MS,
        };
        let anchor = anchor_admin(&person());
        let first = log.append(&root, &anchor, admin_entry(FIRST));
        let second = log.append(&first, &anchor, admin_entry(SECOND));
        let head = log.append(
            &second,
            &anchor,
            enroll_entry(&person(), MEMBER, DeviceRole::Member),
        );
        (log, head)
    }

    fn record(
        &mut self,
        at: &At,
        author: &AdminKeyPair,
        entry: Entry,
        epoch: u32,
        anchor: String,
    ) -> At {
        self.clock += 1;
        let sequencing = match &entry {
            Entry::Remove(_) => Sequencing::Removal {
                cutoffs: Vec::new(),
                discovery_key: [u8::try_from(self.records.len() % 200).unwrap(); 32],
            },
            Entry::Handover(_) => Sequencing::Handover {
                old_anchor_vector: None,
            },
            _ => Sequencing::Empty,
        };
        let record = AuthorizationRecord {
            epoch,
            revision: at.revision + 1,
            previous_entry_hash: at.hash,
            anchor_device_id: anchor.clone(),
            proposal: proposal(self.space, author, self.clock, entry),
            sequencing,
        };
        let next = At {
            epoch,
            revision: record.revision,
            hash: record.hash(),
            anchor,
        };
        self.records.push(record);
        self.writers.push(self.writer);
        next
    }

    /// Drops the last record.
    fn pop(&mut self) {
        self.records.pop();
        self.writers.pop();
    }

    /// An entry `author` signed that the anchor of `at` sequences after it.
    fn append(&mut self, at: &At, author: &AdminKeyPair, entry: Entry) -> At {
        self.record(at, author, entry, at.epoch, at.anchor.clone())
    }

    /// A handover `author` signed of the anchor to `target`, continuing from `at`.
    fn handover(&mut self, at: &At, author: &AdminKeyPair, target: &str) -> At {
        let epoch = at.epoch + 1;
        let entry = Entry::Handover(Handover {
            new_anchor_device_id: target.to_owned(),
            epoch,
            continues_from_revision: at.revision,
            continues_from_hash: at.hash,
        });
        self.record(at, author, entry, epoch, target.to_owned())
    }

    /// What the records in `order` select.
    fn select(&self, order: &[usize]) -> SelectedHead {
        let records: Vec<(WriterId, AuthorizationRecord)> = order
            .iter()
            .map(|index| (self.writers[*index], self.records[*index].clone()))
            .collect();
        select_records(&self.charter, self.freezes.clone(), &records)
    }

    /// Asserts that every arrival order selects `expected` and derives the same state: the
    /// stored order, its reverse, and seeded random orders. Returns what they select.
    fn assert_selected(&self, expected: &At) -> SelectedHead {
        let selected = self.assert_agreed();
        assert_eq!(selected.head, expected.hash);
        selected
    }

    /// Asserts that every arrival order of [`Self::assert_selected`] selects the same head and
    /// derives the same state, whichever head that is. Returns what they select.
    fn assert_agreed(&self) -> SelectedHead {
        let count = self.records.len();
        let forward: Vec<usize> = (0..count).collect();
        let mut orders = vec![forward.clone(), forward.iter().rev().copied().collect()];
        let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::try_from(count).unwrap();
        for _ in 0..16 {
            let mut order = forward.clone();
            for index in (1..count).rev() {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let other = usize::try_from(state % (u64::try_from(index).unwrap() + 1)).unwrap();
                order.swap(index, other);
            }
            orders.push(order);
        }
        let mut selected: Option<SelectedHead> = None;
        for order in orders {
            let head = self.select(&order);
            let first = selected.get_or_insert_with(|| head.clone());
            assert_eq!(*first, head, "arrival order {order:?}");
        }
        selected.unwrap()
    }

    /// [`Self::assert_selected`] for a head its epoch is not frozen at. Returns the reachable
    /// revision.
    fn assert_selects(&self, expected: &At) -> u64 {
        let selected = self.assert_selected(expected);
        assert_eq!(selected.anchor, expected.anchor);
        assert_eq!(selected.frozen_at, None);
        selected.reachable
    }
}

/// A writer id derived from `seed`.
fn writer(seed: u8) -> WriterId {
    WriterId::from_bytes([seed; 16])
}

fn remove(device: &str) -> Entry {
    Entry::Remove(Remove {
        device_id: device.to_owned(),
    })
}

fn demote(device: &str) -> Entry {
    Entry::SetRole(SetRole::Device {
        device_id: device.to_owned(),
        role: DeviceRole::Member,
    })
}

fn rotate_recovery(key: &AdminKeyPair) -> Entry {
    Entry::SetRole(SetRole::Recovery {
        recovery_admin_key: key.public_key(),
    })
}

/// A recovery key no test charter names.
fn other_recovery(seed: u8) -> AdminKeyPair {
    AdminKeyPair::from_seed(&[seed; 32]).unwrap()
}

/// `admin` hands the anchor to its own device from `at` and then again `count - 1` times.
fn self_handovers(log: &mut Log, at: &At, admin: &AdminKeyPair, device: &str, count: usize) -> At {
    let mut head = at.clone();
    for _ in 0..count {
        head = log.handover(&head, admin, device);
    }
    head
}

#[test]
fn a_stale_admin_the_continuation_removes_or_demotes_loses_its_self_handovers() {
    for revoke in [remove, demote] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let continuation = log.append(&base, &anchor, revoke(&device_id(FIRST)));
        let stale = self_handovers(&mut log, &base, &device_admin(FIRST), &device_id(FIRST), 5);
        assert_eq!(stale.epoch, 5);
        let reachable = log.assert_selects(&continuation);
        assert_eq!(
            reachable, stale.revision,
            "a vetoed chain still counts for readiness"
        );
    }
}

#[test]
fn a_demotion_in_a_later_epoch_after_a_planned_handover_still_vetoes_a_stale_admin() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let next = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let planned = log.handover(&next, &anchor, &device_id(SECOND));
    let demoted = log.append(&planned, &device_admin(SECOND), demote(&device_id(FIRST)));
    self_handovers(&mut log, &base, &device_admin(FIRST), &device_id(FIRST), 5);
    log.assert_selects(&demoted);
}

#[test]
fn a_handover_to_a_device_the_continuation_removes_is_vetoed() {
    let (mut log, base) = Log::new();
    let continuation = log.append(&base, &anchor_admin(&person()), remove(&device_id(FIRST)));
    log.handover(&base, &device_admin(SECOND), &device_id(FIRST));
    log.assert_selects(&continuation);
}

#[test]
fn sibling_admin_handovers_do_not_veto_each_other() {
    let (mut log, base) = Log::new();
    let first = log.handover(&base, &device_admin(FIRST), &device_id(FIRST));
    let second = log.handover(&base, &device_admin(SECOND), &device_id(SECOND));
    let first = log.append(&first, &device_admin(FIRST), remove(&device_id(SECOND)));
    let second = log.append(&second, &device_admin(SECOND), remove(&device_id(FIRST)));
    // Equal epochs: the lower handover hash opens the selected epoch.
    let (low, high) = (
        &log.records[log.records.len() - 4],
        &log.records[log.records.len() - 3],
    );
    let expected = if low.hash() < high.hash() {
        first
    } else {
        second
    };
    log.assert_selects(&expected);
}

#[test]
fn a_stolen_anchors_admin_signed_rotation_does_not_veto_a_kit_handover() {
    let (mut log, base) = Log::new();
    let stolen = anchor_admin(&person());
    let rotated = log.append(&base, &stolen, rotate_recovery(&other_recovery(90)));
    log.append(
        &rotated,
        &stolen,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let kit = log.handover(&base, &recovery_admin(&person()), &device_id(MEMBER));
    log.assert_selects(&kit);
}

#[test]
fn a_stolen_anchor_vetoes_an_admin_forced_handover_by_removing_the_forcer() {
    let (mut log, base) = Log::new();
    let continuation = log.append(&base, &anchor_admin(&person()), remove(&device_id(SECOND)));
    log.handover(&base, &device_admin(SECOND), &device_id(SECOND));
    log.assert_selects(&continuation);
}

#[test]
fn a_stolen_anchor_removing_the_kit_target_does_not_veto_the_kit_handover() {
    let (mut log, base) = Log::new();
    log.append(&base, &anchor_admin(&person()), remove(&device_id(MEMBER)));
    let kit = log.handover(&base, &recovery_admin(&person()), &device_id(MEMBER));
    log.assert_selects(&kit);
}

#[test]
fn a_stolen_anchors_self_handovers_from_below_the_kit_fork_point_lose_on_recovery_count() {
    let (mut log, base) = Log::new();
    let stolen = anchor_admin(&person());
    let next = log.append(
        &base,
        &stolen,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let kit = log.handover(&next, &recovery_admin(&person()), &device_id(MEMBER));
    self_handovers(&mut log, &base, &stolen, ANCHOR_DEVICE_ID, 4);
    log.assert_selects(&kit);
}

#[test]
fn a_stolen_anchors_handover_inside_the_kit_epoch_is_vetoed_once_its_removal_is_stored() {
    let (mut log, base) = Log::new();
    let stolen = anchor_admin(&person());
    let recovery = recovery_admin(&person());
    let kit = log.handover(&base, &recovery, &device_id(MEMBER));
    let taken = log.handover(&kit, &stolen, ANCHOR_DEVICE_ID);
    log.assert_selects(&taken);
    let removed = log.append(&kit, &recovery, remove(ANCHOR_DEVICE_ID));
    log.assert_selects(&removed);
}

#[test]
fn a_recovery_key_an_admin_installed_carries_no_kit_precedence() {
    let (mut log, base) = Log::new();
    let stolen = anchor_admin(&person());
    let installed = other_recovery(91);
    let rotated = log.append(&base, &stolen, rotate_recovery(&installed));
    let continuation = log.append(&rotated, &stolen, remove(&device_id(MEMBER)));
    log.handover(&rotated, &installed, &device_id(MEMBER));
    log.assert_selects(&continuation);
}

#[test]
fn a_forced_handover_after_a_loss_beats_unseen_continuation_unless_it_removes_the_forcer() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let lost = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let later = log.append(
        &lost,
        &anchor,
        enroll_entry(&person(), 5, DeviceRole::Member),
    );
    let forced = log.handover(&base, &device_admin(SECOND), &device_id(SECOND));
    log.assert_selects(&forced);
    let removal = log.append(&later, &anchor, remove(&device_id(SECOND)));
    log.assert_selects(&removal);
}

#[test]
fn a_restored_anchors_self_handover_beats_its_own_lost_continuation() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let lost = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    log.append(&lost, &anchor, remove(&device_id(FIRST)));
    let restored = log.handover(&base, &anchor, ANCHOR_DEVICE_ID);
    log.assert_selects(&restored);
}

#[test]
fn a_leaked_old_kit_is_vetoed_before_its_rotation_and_refused_after_it() {
    let (mut log, base) = Log::new();
    let leaked = recovery_admin(&person());
    let current = other_recovery(92);
    let rotated = log.append(&base, &leaked, rotate_recovery(&current));
    log.handover(&base, &leaked, &device_id(MEMBER));
    log.assert_selects(&rotated);
    // Past the rotation the old key is no admin, so its handover never enters the tree.
    log.handover(&rotated, &leaked, &device_id(MEMBER));
    log.assert_selects(&rotated);
    // The rotated key is of kit lineage: it overrules the anchor's continuation.
    log.append(
        &rotated,
        &anchor_admin(&person()),
        remove(&device_id(MEMBER)),
    );
    let kit = log.handover(&rotated, &current, &device_id(MEMBER));
    log.assert_selects(&kit);
}

#[test]
fn entries_of_a_frozen_epoch_past_its_freeze_are_no_evidence() {
    let (mut log, base) = Log::new();
    log.freezes
        .insert(ANCHOR_DEVICE_ID.to_owned(), vec![base.revision]);
    log.append(&base, &anchor_admin(&person()), remove(&device_id(FIRST)));
    let forced = log.handover(&base, &device_admin(FIRST), &device_id(FIRST));
    log.assert_selects(&forced);
}

/// The enrollment of a fourth device after `base`, then the anchor's planned handover to
/// [`SECOND`] from it. Returns the enrollment and the handover.
fn planned_handover(log: &mut Log, base: &At) -> (At, At) {
    let anchor = anchor_admin(&person());
    let next = log.append(
        base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let planned = log.handover(&next, &anchor, &device_id(SECOND));
    (next, planned)
}

#[test]
fn a_stale_admin_forking_beside_a_planned_handover_that_revokes_it_is_vetoed() {
    for revoke in [remove, demote] {
        let (mut log, base) = Log::new();
        let (next, planned) = planned_handover(&mut log, &base);
        let revoked = log.append(&planned, &device_admin(SECOND), revoke(&device_id(FIRST)));
        let stale = self_handovers(&mut log, &next, &device_admin(FIRST), &device_id(FIRST), 5);
        assert!(stale.epoch > revoked.epoch);
        log.assert_selects(&revoked);
    }
}

#[test]
fn a_stale_admin_forking_beside_a_restored_anchors_self_handover_that_removes_it_is_vetoed() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let restored = log.handover(&base, &anchor, ANCHOR_DEVICE_ID);
    let removed = log.append(&restored, &anchor, remove(&device_id(FIRST)));
    self_handovers(&mut log, &base, &device_admin(FIRST), &device_id(FIRST), 4);
    log.assert_selects(&removed);
}

#[test]
fn a_leaked_old_kit_forking_before_a_planned_handover_that_preceded_its_rotation_is_vetoed() {
    let (mut log, base) = Log::new();
    let leaked = recovery_admin(&person());
    let (next, planned) = planned_handover(&mut log, &base);
    let rotated = log.append(&planned, &leaked, rotate_recovery(&other_recovery(93)));
    let kit = log.handover(&next, &leaked, &device_id(MEMBER));
    log.append(&kit, &leaked, remove(&device_id(SECOND)));
    log.assert_selects(&rotated);
}

#[test]
fn admins_forcing_handovers_after_an_anchor_loss_rank_rather_than_veto_each_other() {
    let (mut log, base) = Log::new();
    let first = log.handover(&base, &device_admin(FIRST), &device_id(FIRST));
    log.append(&first, &device_admin(FIRST), remove(&device_id(SECOND)));
    let second = self_handovers(
        &mut log,
        &base,
        &device_admin(SECOND),
        &device_id(SECOND),
        2,
    );
    let second = log.append(&second, &device_admin(SECOND), remove(&device_id(FIRST)));
    log.assert_selects(&second);
}

#[test]
fn a_leaked_old_kit_is_vetoed_by_its_rotation_inside_a_forced_handover_beside_it() {
    let (mut log, base) = Log::new();
    let leaked = recovery_admin(&person());
    let forced = log.handover(&base, &device_admin(SECOND), &device_id(SECOND));
    let rotated = log.append(&forced, &leaked, rotate_recovery(&other_recovery(94)));
    let kit = log.handover(&base, &leaked, &device_id(MEMBER));
    log.append(&kit, &leaked, remove(&device_id(SECOND)));
    log.assert_selects(&rotated);
}

/// A planned handover and the head its target reaches.
struct Handed {
    log: Log,
    /// The head of [`Log::new`].
    base: At,
    /// The enrollment the handover continues from.
    next: At,
    planned: At,
    reached: At,
}

/// The anchor's planned handover to [`SECOND`] and, with `active`, the removal of the old
/// anchor that [`SECOND`] seals under it. With `fresh` the old anchor's later records come from a
/// writer of its device it never used; otherwise its fork at the enrollment freezes its epoch
/// there.
fn handed(fresh: bool, active: bool) -> Handed {
    let (mut log, base) = Log::new();
    let (next, planned) = planned_handover(&mut log, &base);
    let reached = if active {
        log.append(&planned, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID))
    } else {
        planned.clone()
    };
    if fresh {
        log.writer = writer(9);
    } else {
        log.freezes
            .insert(ANCHOR_DEVICE_ID.to_owned(), vec![next.revision]);
    }
    Handed {
        log,
        base,
        next,
        planned,
        reached,
    }
}

#[test]
fn an_anchors_revocation_at_its_own_handover_point_is_no_evidence_against_it() {
    for fresh in [false, true] {
        for active in [true, false] {
            let Handed {
                mut log,
                next,
                reached,
                ..
            } = handed(fresh, active);
            let anchor = anchor_admin(&person());
            log.append(&next, &anchor, remove(&device_id(SECOND)));
            log.append(&next, &anchor, demote(&device_id(SECOND)));
            log.assert_selects(&reached);
        }
    }
}

#[test]
fn an_anchors_harmless_entry_at_its_handover_point_carries_no_later_revocation() {
    for fresh in [false, true] {
        for active in [true, false] {
            let Handed {
                mut log,
                next,
                reached,
                ..
            } = handed(fresh, active);
            let anchor = anchor_admin(&person());
            let harmless = log.append(
                &next,
                &anchor,
                enroll_entry(&person(), 5, DeviceRole::Member),
            );
            log.append(&harmless, &anchor, remove(&device_id(SECOND)));
            log.assert_selects(&reached);
        }
    }
}

#[test]
fn an_admin_the_anchor_mints_after_its_handover_point_vetoes_nothing() {
    for fresh in [false, true] {
        let Handed {
            mut log,
            next,
            reached,
            ..
        } = handed(fresh, true);
        let minted = log.append(&next, &anchor_admin(&person()), admin_entry(5));
        log.append(&minted, &device_admin(5), remove(&device_id(SECOND)));
        log.handover(&minted, &device_admin(5), &device_id(5));
        log.assert_selects(&reached);
    }
}

#[test]
fn an_anchors_self_handovers_at_its_handover_point_lose_whatever_their_rank() {
    for fresh in [false, true] {
        for below_plain in [false, true] {
            let Handed {
                mut log,
                next,
                reached,
                ..
            } = handed(fresh, true);
            let anchor = anchor_admin(&person());
            let from = if below_plain {
                log.append(
                    &next,
                    &anchor,
                    enroll_entry(&person(), 5, DeviceRole::Member),
                )
            } else {
                next.clone()
            };
            let taken = self_handovers(&mut log, &from, &anchor, ANCHOR_DEVICE_ID, 3);
            assert!(taken.epoch > reached.epoch);
            log.append(&taken, &anchor, remove(&device_id(SECOND)));
            log.assert_selects(&reached);
        }
    }
}

/// A second planned handover from `next` to [`FIRST`] whose hash ranks it above `planned`.
fn better_ranked_handover(log: &mut Log, next: &At, planned: &At) -> At {
    loop {
        let other = log.handover(next, &anchor_admin(&person()), &device_id(FIRST));
        if other.hash < planned.hash {
            return other;
        }
        log.pop();
    }
}

#[test]
fn an_anchors_better_ranked_second_handover_yields_to_the_target_that_acted() {
    for fresh in [false, true] {
        let Handed {
            mut log,
            next,
            planned,
            reached,
            ..
        } = handed(fresh, true);
        better_ranked_handover(&mut log, &next, &planned);
        log.assert_selects(&reached);

        // A better ranked handover whose target acted wins over an idle one as well.
        let Handed {
            mut log,
            next,
            planned,
            ..
        } = handed(fresh, false);
        let other = better_ranked_handover(&mut log, &next, &planned);
        let acted = log.append(&other, &device_admin(FIRST), remove(ANCHOR_DEVICE_ID));
        log.assert_selects(&acted);
    }
}

#[test]
fn an_anchors_equivocation_without_a_lone_active_target_freezes_its_point() {
    for fresh in [false, true] {
        for active in [false, true] {
            let Handed {
                mut log,
                next,
                planned,
                ..
            } = handed(fresh, active);
            let other = better_ranked_handover(&mut log, &next, &planned);
            if active {
                log.append(
                    &other,
                    &device_admin(FIRST),
                    enroll_entry(&person(), 6, DeviceRole::Member),
                );
            }
            // Neither handover opens an epoch, and the point stays frozen. The old anchor stays
            // named there unless the removal its target sealed survives and vacates the slot.
            let frozen = log.assert_selected(&next);
            let anchor = if active { "" } else { ANCHOR_DEVICE_ID };
            assert_eq!(frozen.anchor, anchor);
            assert_eq!(frozen.frozen_at, Some(next.revision));
            // An admin's forced handover at the point recovers the space.
            let forced = log.handover(&next, &device_admin(SECOND), &device_id(SECOND));
            log.assert_selects(&forced);
        }
    }
}

#[test]
fn an_anchors_self_handovers_from_above_its_handover_point_lose_to_it() {
    for fresh in [false, true] {
        for active in [true, false] {
            let Handed {
                mut log,
                base,
                reached,
                ..
            } = handed(fresh, active);
            let anchor = anchor_admin(&person());
            let taken = self_handovers(&mut log, &base, &anchor, ANCHOR_DEVICE_ID, 3);
            log.append(&taken, &anchor, remove(&device_id(SECOND)));
            log.assert_selects(&reached);
        }
    }
    // Without a handover to another device the anchor's self-handovers are its continuation,
    // which vetoes a forced handover by the admin it removes.
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let next = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let taken = self_handovers(&mut log, &base, &anchor, ANCHOR_DEVICE_ID, 3);
    let removal = log.append(&taken, &anchor, remove(&device_id(SECOND)));
    log.handover(&next, &device_admin(SECOND), &device_id(SECOND));
    log.assert_selects(&removal);
}

/// The anchor's equivocation at `next`: its planned handover to [`SECOND`] and a better ranked
/// one to [`FIRST`]. With `active` each target seals an entry under its handover: [`SECOND`]
/// the removal of the old anchor in [`handed`], and [`FIRST`] the enrollment of device 6.
fn equivocation(fresh: bool, active: bool) -> Handed {
    let mut handed = handed(fresh, active);
    let other = better_ranked_handover(&mut handed.log, &handed.next, &handed.planned);
    if active {
        handed.log.append(
            &other,
            &device_admin(FIRST),
            enroll_entry(&person(), 6, DeviceRole::Member),
        );
    }
    handed
}

#[test]
fn a_target_the_anchor_enrolls_off_its_handover_point_carries_no_authority() {
    for fresh in [false, true] {
        for active in [true, false] {
            let Handed {
                mut log,
                next,
                reached,
                ..
            } = handed(fresh, active);
            let anchor = anchor_admin(&person());
            let minted = log.append(&next, &anchor, admin_entry(5));
            let taken = log.handover(&minted, &anchor, &device_id(5));
            log.append(&taken, &device_admin(5), remove(&device_id(SECOND)));
            log.assert_selects(&reached);
            let selected = log.assert_selected(&reached);
            assert!(!selected.devices.contains_key(&device_id(5)));
            let removed = active.then_some(reached.revision);
            assert_eq!(selected.devices[ANCHOR_DEVICE_ID], removed);
        }
    }
}

#[test]
fn a_device_the_anchor_promotes_off_its_handover_point_carries_no_authority() {
    for fresh in [false, true] {
        for active in [true, false] {
            let Handed {
                mut log,
                next,
                reached,
                ..
            } = handed(fresh, active);
            let anchor = anchor_admin(&person());
            let promoted = log.append(
                &next,
                &anchor,
                Entry::SetRole(SetRole::Device {
                    device_id: device_id(MEMBER),
                    role: DeviceRole::Admin(device_admin(MEMBER).public_key()),
                }),
            );
            let taken = log.handover(&promoted, &anchor, &device_id(MEMBER));
            log.append(&taken, &device_admin(MEMBER), remove(&device_id(SECOND)));
            log.assert_selects(&reached);
        }
    }
}

#[test]
fn an_anchors_self_handover_from_an_earlier_point_does_not_move_its_freeze() {
    for fresh in [false, true] {
        for active in [false, true] {
            let Handed {
                mut log,
                base,
                next,
                ..
            } = equivocation(fresh, active);
            let anchor = anchor_admin(&person());
            let taken = self_handovers(&mut log, &base, &anchor, ANCHOR_DEVICE_ID, 3);
            log.append(&taken, &anchor, remove(&device_id(SECOND)));
            let frozen = log.assert_selected(&next);
            assert_eq!(frozen.frozen_at, Some(next.revision));
        }
    }
}

#[test]
fn an_admins_forced_recovery_sticks_after_the_anchor_tries_to_walk_its_freeze_back() {
    for fresh in [false, true] {
        for active in [false, true] {
            let Handed {
                mut log,
                base,
                next,
                reached,
                ..
            } = equivocation(fresh, active);
            let forced = log.handover(&next, &device_admin(SECOND), &device_id(SECOND));
            let anchor = anchor_admin(&person());
            let taken = self_handovers(&mut log, &base, &anchor, ANCHOR_DEVICE_ID, 4);
            log.append(&taken, &anchor, remove(&device_id(SECOND)));
            log.assert_selects(&forced);
            let selected = log.assert_selected(&forced);
            let removed = active.then_some(reached.revision);
            assert_eq!(selected.devices[ANCHOR_DEVICE_ID], removed);
            assert!(!selected.devices.contains_key(&device_id(6)));
        }
    }
}

#[test]
fn a_removal_of_the_old_anchor_survives_its_freeze_and_vacates_the_slot() {
    for fresh in [false, true] {
        let Handed {
            log, next, reached, ..
        } = equivocation(fresh, true);
        let frozen = log.assert_selected(&next);
        assert_eq!(frozen.frozen_at, Some(next.revision));
        assert_eq!(frozen.anchor, "");
        assert_eq!(
            frozen.devices[ANCHOR_DEVICE_ID],
            Some(reached.revision),
            "the removal the target sealed narrows the frozen point"
        );
        assert!(
            !frozen.devices.contains_key(&device_id(6)),
            "the enrollment the other target sealed does not widen it"
        );
    }
}

/// The takeover the round 4 review describes: [`equivocation`] with both targets acting freezes
/// the old anchor's point and vacates the slot, [`SECOND`] forces a recovery handover at that
/// point, and the old anchor forces a handover to itself under the recovery, removes both admins,
/// and enrolls device 7 as an admin that enrolls member 8.
struct Takeover {
    log: Log,
    /// The recovery head [`SECOND`] forces.
    forced: At,
}

fn takeover(fresh: bool) -> Takeover {
    let Handed { mut log, next, .. } = equivocation(fresh, true);
    let forced = log.handover(&next, &device_admin(SECOND), &device_id(SECOND));
    let anchor = anchor_admin(&person());
    let mut taken = log.handover(&forced, &anchor, ANCHOR_DEVICE_ID);
    for admin in [FIRST, SECOND] {
        taken = log.append(&taken, &anchor, remove(&device_id(admin)));
    }
    let minted = log.append(&taken, &anchor, admin_entry(7));
    log.append(
        &minted,
        &device_admin(7),
        enroll_entry(&person(), 8, DeviceRole::Member),
    );
    Takeover { log, forced }
}

#[test]
#[ignore = "milestone 11 step 9 handover redesign: a narrowed removal is invisible to the walk, so the old anchor forces past the recovery"]
fn an_old_anchor_removed_through_a_freeze_takes_nothing_back_from_the_recovery() {
    for fresh in [false, true] {
        let Takeover { log, forced } = takeover(fresh);
        log.assert_selects(&forced);
        let selected = log.assert_selected(&forced);
        assert!(selected.devices[ANCHOR_DEVICE_ID].is_some());
        assert_eq!(selected.devices[&device_id(FIRST)], None);
        assert!(!selected.devices.contains_key(&device_id(7)));
        assert!(!selected.devices.contains_key(&device_id(8)));
    }
}

#[test]
#[ignore = "milestone 11 step 9 handover redesign: the old anchor forces again inside the kit epoch and outranks it"]
fn a_kit_recovery_stands_after_an_old_anchor_removed_through_a_freeze_forces_again() {
    for fresh in [false, true] {
        let Takeover { mut log, forced } = takeover(fresh);
        let kit = log.handover(&forced, &recovery_admin(&person()), &device_id(MEMBER));
        log.handover(&kit, &anchor_admin(&person()), ANCHOR_DEVICE_ID);
        log.assert_selects(&kit);
        let selected = log.assert_selected(&kit);
        assert!(!selected.devices.contains_key(&device_id(7)));
    }
}

#[test]
#[ignore = "milestone 11 step 9 handover redesign: an earlier fork to a pre-planted target moves the cut and leaves its handover alone"]
fn a_target_enrolled_before_the_planned_one_does_not_stand_alone_by_forking_earlier() {
    for fresh in [false, true] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let planted = log.append(&base, &anchor, admin_entry(5));
        let enrolled = log.append(&planted, &anchor, admin_entry(6));
        let planned = log.handover(&enrolled, &anchor, &device_id(6));
        let removed = log.append(&planned, &device_admin(6), remove(ANCHOR_DEVICE_ID));
        log.append(&removed, &device_admin(6), remove(&device_id(5)));
        if fresh {
            log.writer = writer(9);
        } else {
            log.freezes
                .insert(ANCHOR_DEVICE_ID.to_owned(), vec![planted.revision]);
        }
        let pushed = log.handover(&planted, &anchor, &device_id(5));
        let acted = log.append(
            &pushed,
            &device_admin(5),
            enroll_entry(&person(), 8, DeviceRole::Member),
        );
        let selected = log.assert_agreed();
        assert_ne!(selected.head, pushed.hash);
        assert_ne!(selected.head, acted.hash);
        assert!(selected.devices[ANCHOR_DEVICE_ID].is_some());
        assert!(selected.devices[&device_id(5)].is_some());
    }
}

#[test]
#[ignore = "milestone 11 step 9 handover redesign: narrowing collects removals from vetoed subtrees of a contender"]
fn a_removal_in_a_vetoed_subtree_of_a_contender_does_not_narrow_its_freeze() {
    for fresh in [false, true] {
        let Handed {
            mut log,
            next,
            planned,
            ..
        } = equivocation(fresh, true);
        let anchor = anchor_admin(&person());
        let vetoed = log.handover(&planned, &anchor, ANCHOR_DEVICE_ID);
        log.append(&vetoed, &anchor, remove(&device_id(MEMBER)));
        let frozen = log.assert_selected(&next);
        assert_eq!(frozen.frozen_at, Some(next.revision));
        assert_eq!(frozen.devices[&device_id(MEMBER)], None);
    }
}

#[test]
#[ignore = "milestone 11 step 9 handover redesign: narrowing collects removals whoever sealed them"]
fn a_removal_the_old_anchor_seals_under_a_contender_does_not_narrow_its_freeze() {
    for fresh in [false, true] {
        let Handed {
            mut log,
            next,
            planned,
            ..
        } = handed(fresh, true);
        let other = better_ranked_handover(&mut log, &next, &planned);
        let acted = log.append(
            &other,
            &device_admin(FIRST),
            enroll_entry(&person(), 6, DeviceRole::Member),
        );
        log.append(&acted, &anchor_admin(&person()), remove(&device_id(MEMBER)));
        let frozen = log.assert_selected(&next);
        assert_eq!(frozen.frozen_at, Some(next.revision));
        assert_eq!(frozen.devices[&device_id(MEMBER)], None);
    }
}
