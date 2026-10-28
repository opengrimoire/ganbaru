//! Handover selection at a fork point: the trust order between a kit-lineage recovery
//! handover, the anchor's continuation, and a device admin handover, and the vetoes the
//! continuation's revocations cast. Each case selects the same head whatever order its entries
//! arrive in.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, DeviceRole, Digest32, Entry, Handover, Remove, Sequencing,
    SetRole, SignedCharter, SpaceId,
};

use super::authorization::admin_entry;
use super::replica::{
    ANCHOR_DEVICE_ID, START_MS, anchor_admin, device_admin, device_id, enroll_entry, person,
    proposal, recovery_admin, test_charter,
};
use crate::authorization::select_records;

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
        next
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

    /// Selected head hash and reachable revision of the records in `order`.
    fn select(&self, order: &[usize]) -> (Digest32, u64) {
        let records: Vec<AuthorizationRecord> = order
            .iter()
            .map(|index| self.records[*index].clone())
            .collect();
        select_records(&self.charter, self.freezes.clone(), &records)
    }

    /// Asserts that every arrival order selects `expected`: the stored order, its reverse, and
    /// seeded random orders. Returns the reachable revision.
    fn assert_selects(&self, expected: &At) -> u64 {
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
        let mut reachable = None;
        for order in orders {
            let (head, reached) = self.select(&order);
            assert_eq!(head, expected.hash, "arrival order {order:?}");
            assert_eq!(*reachable.get_or_insert(reached), reached);
        }
        reachable.unwrap()
    }
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
