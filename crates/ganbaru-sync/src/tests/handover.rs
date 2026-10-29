//! Anchor handover selection: takes, contests between handovers out of one epoch instance,
//! objections, yields, kit pins and demotions, rotation vetoes, installed recovery keys, and the
//! devices held for data while an instance is frozen. Each case selects the same head whatever
//! order its entries arrive in, and most run twice: once with the attacker's records under a
//! writer whose fork freezes its epoch, once under a fresh writer id.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    AdminKeyPair, AuthorizationRecord, DeviceRole, Digest32, Entry, Handover, Remove, Sequencing,
    SetRole, SignedCharter, SpaceId, WriterId, Yield,
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
        let (mut log, root) = Self::charter();
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

    /// The charter, then the enrollment of [`MEMBER`] only: the anchor is the single admin.
    fn single() -> (Self, At) {
        let (mut log, root) = Self::charter();
        let head = log.append(
            &root,
            &anchor_admin(&person()),
            enroll_entry(&person(), MEMBER, DeviceRole::Member),
        );
        (log, head)
    }

    fn charter() -> (Self, At) {
        let space = SpaceId::from_bytes([41; 16]);
        let charter = test_charter(&person(), space);
        let root = At {
            epoch: 0,
            revision: 0,
            hash: charter.hash(),
            anchor: ANCHOR_DEVICE_ID.to_owned(),
        };
        let log = Self {
            space,
            charter,
            records: Vec::new(),
            writers: Vec::new(),
            writer: writer(0),
            freezes: BTreeMap::new(),
            clock: START_MS,
        };
        (log, root)
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

    /// The take of admin desktop `device`, signed by its own admin key, from `at`.
    fn take(&mut self, at: &At, device: usize) -> At {
        self.handover(at, &device_admin(device), &device_id(device))
    }

    /// The charter anchor's take of its own device from `at`.
    fn self_handover(&mut self, at: &At) -> At {
        self.handover(at, &anchor_admin(&person()), ANCHOR_DEVICE_ID)
    }

    /// A handover the charter's kit key signs to `target` from `at`.
    fn kit(&mut self, at: &At, target: &str) -> At {
        self.handover(at, &recovery_admin(&person()), target)
    }

    /// A yield `author` signs to `handover`, sequenced after `at`.
    fn yield_to(&mut self, at: &At, author: &AdminKeyPair, handover: &At) -> At {
        self.append(
            at,
            author,
            Entry::Yield(Yield {
                handover_hash: handover.hash,
            }),
        )
    }

    /// The attacker's later records: under a fresh writer id with `fresh`, otherwise under the
    /// anchor's own writer, whose fork freezes the anchor's epoch at `fork`.
    fn attacker(&mut self, fresh: bool, fork: &At) {
        if fresh {
            self.writer = writer(9);
        } else {
            self.freezes
                .insert(ANCHOR_DEVICE_ID.to_owned(), vec![fork.revision]);
        }
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
        assert_eq!(selected.anchor, expected.anchor);
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

    /// [`Self::assert_selected`] for a head whose instance is not frozen and holds nobody.
    fn assert_selects(&self, expected: &At) -> SelectedHead {
        let selected = self.assert_selected(expected);
        assert_eq!(selected.frozen_at, None);
        assert!(selected.held.is_empty(), "held {:?}", selected.held);
        selected
    }

    /// [`Self::assert_selected`] for a head its instance is frozen at.
    fn assert_frozen(&self, expected: &At) -> SelectedHead {
        let selected = self.assert_selected(expected);
        assert_eq!(selected.frozen_at, Some(expected.revision));
        selected
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

fn promote(device: usize) -> Entry {
    Entry::SetRole(SetRole::Device {
        device_id: device_id(device),
        role: DeviceRole::Admin(device_admin(device).public_key()),
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

/// Whether `device` is enrolled and live at the selected head.
fn live(selected: &SelectedHead, device: &str) -> bool {
    selected.devices.get(device) == Some(&None)
}

/// Whether `device` is enrolled and removed at the selected head.
fn removed(selected: &SelectedHead, device: &str) -> bool {
    selected.devices.get(device).is_some_and(Option::is_some)
}

/// The shared contest: from `next`, where the anchor enrolls device 4, [`SECOND`] takes
/// (`planned`) and, when asked, removes the anchor; [`FIRST`], standing for a planted admin,
/// takes from `next` and enrolls 6 (`acted`).
struct Contest {
    log: Log,
    next: At,
    planned: At,
    acted: At,
}

fn contest(fresh: bool, removes_anchor: bool) -> Contest {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let next = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let planned = log.take(&next, SECOND);
    if removes_anchor {
        log.append(&planned, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
    }
    log.attacker(fresh, &next);
    let other = log.take(&next, FIRST);
    let acted = log.append(
        &other,
        &device_admin(FIRST),
        enroll_entry(&person(), 6, DeviceRole::Member),
    );
    Contest {
        log,
        next,
        planned,
        acted,
    }
}

/// [`contest`], then [`SECOND`]'s second take from `next` (`forced`), the anchor's
/// self-handover from `forced`, its removal of both admins, its enrollment of admin 7, and 7's
/// enrollment of 8.
struct Takeover {
    contest: Contest,
    forced: At,
}

fn takeover(fresh: bool) -> Takeover {
    let mut contest = contest(fresh, true);
    let log = &mut contest.log;
    let forced = log.take(&contest.next, SECOND);
    let anchor = anchor_admin(&person());
    let mut taken = log.self_handover(&forced);
    for admin in [FIRST, SECOND] {
        taken = log.append(&taken, &anchor, remove(&device_id(admin)));
    }
    let minted = log.append(&taken, &anchor, admin_entry(7));
    log.append(
        &minted,
        &device_admin(7),
        enroll_entry(&person(), 8, DeviceRole::Member),
    );
    Takeover { contest, forced }
}

#[test]
fn an_old_anchor_removed_through_a_freeze_takes_nothing_back_from_the_recovery() {
    for fresh in [false, true] {
        let Takeover { contest, .. } = takeover(fresh);
        let selected = contest.log.assert_frozen(&contest.next);
        for device in [ANCHOR_DEVICE_ID, &device_id(FIRST), &device_id(SECOND)] {
            assert!(live(&selected, device));
            assert!(selected.held.contains(device), "{device} is held");
        }
        assert!(live(&selected, &device_id(MEMBER)));
        assert!(live(&selected, &device_id(4)));
        for absent in [6, 7, 8] {
            assert!(!selected.devices.contains_key(&device_id(absent)));
        }

        // Both admins yield to the second take: epoch 0 resolves to it, and the removal of the
        // anchor SECOND seals under it objects to the anchor's self-handover out of it.
        let Takeover {
            contest: Contest { mut log, acted, .. },
            forced,
        } = takeover(fresh);
        log.yield_to(&acted, &device_admin(FIRST), &forced);
        let objection = log.append(&forced, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
        log.yield_to(&objection, &device_admin(SECOND), &forced);
        let selected = log.assert_frozen(&forced);
        for device in [ANCHOR_DEVICE_ID, &device_id(FIRST), &device_id(SECOND)] {
            assert!(live(&selected, device));
        }
        assert!(selected.held.contains(ANCHOR_DEVICE_ID));
        assert!(selected.held.contains(&device_id(SECOND)));
        for absent in [6, 7, 8] {
            assert!(!selected.devices.contains_key(&device_id(absent)));
        }
    }
}

#[test]
fn a_kit_recovery_stands_after_an_old_anchor_removed_through_a_freeze_forces_again() {
    for fresh in [false, true] {
        let Takeover {
            contest: Contest { mut log, .. },
            forced,
        } = takeover(fresh);
        let kit = log.kit(&forced, &device_id(MEMBER));
        // Rule K demoted the anchor at the kit handover, so its key signs nothing there.
        log.self_handover(&kit);
        let selected = log.assert_selects(&kit);
        for device in [ANCHOR_DEVICE_ID, &device_id(FIRST), &device_id(SECOND)] {
            assert!(live(&selected, device));
        }
        assert!(selected.admins.is_empty(), "admins {:?}", selected.admins);
        assert!(live(&selected, &device_id(MEMBER)));
        assert_eq!(selected.kit_handovers, 1);
        for absent in [7, 8] {
            assert!(!selected.devices.contains_key(&device_id(absent)));
        }
    }
}

#[test]
fn a_target_enrolled_before_the_planned_one_does_not_stand_alone_by_forking_earlier() {
    for fresh in [false, true] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let planted = log.append(&base, &anchor, admin_entry(5));
        let enrolled = log.append(&planted, &anchor, admin_entry(6));
        let planned = log.take(&enrolled, 6);
        let removed_anchor = log.append(&planned, &device_admin(6), remove(ANCHOR_DEVICE_ID));
        let cleared = log.append(&removed_anchor, &device_admin(6), remove(&device_id(5)));
        log.attacker(fresh, &planted);
        let pushed = log.take(&planted, 5);
        log.append(
            &pushed,
            &device_admin(5),
            enroll_entry(&person(), 8, DeviceRole::Member),
        );
        let selected = log.assert_frozen(&planted);
        for device in [ANCHOR_DEVICE_ID, &device_id(5)] {
            assert!(live(&selected, device));
            assert!(selected.held.contains(device), "{device} is held");
        }
        assert!(!selected.devices.contains_key(&device_id(6)));
        assert!(!selected.devices.contains_key(&device_id(8)));

        // The kit continues from the last entry of 6's branch and pins its ancestry.
        let kit = log.kit(&cleared, &device_id(6));
        let selected = log.assert_selects(&kit);
        assert!(removed(&selected, ANCHOR_DEVICE_ID));
        assert!(removed(&selected, &device_id(5)));
        assert!(live(&selected, &device_id(6)));
        assert!(!selected.devices.contains_key(&device_id(8)));
    }
}

#[test]
fn a_removal_in_a_vetoed_subtree_of_a_contender_does_not_narrow_its_freeze() {
    for fresh in [false, true] {
        let Contest {
            mut log,
            next,
            planned,
            ..
        } = contest(fresh, true);
        let anchor = anchor_admin(&person());
        let taken = log.self_handover(&planned);
        log.append(&taken, &anchor, remove(&device_id(MEMBER)));
        let selected = log.assert_frozen(&next);
        assert!(live(&selected, &device_id(MEMBER)));
    }
}

#[test]
fn a_removal_the_old_anchor_seals_under_a_contender_does_not_narrow_its_freeze() {
    for fresh in [false, true] {
        let Contest {
            mut log,
            next,
            acted,
            ..
        } = contest(fresh, false);
        log.append(&acted, &anchor_admin(&person()), remove(&device_id(MEMBER)));
        let selected = log.assert_frozen(&next);
        assert!(live(&selected, &device_id(MEMBER)));
    }
}

#[test]
fn an_uncontested_take_stands_and_two_takes_freeze_until_one_yields() {
    // A planned or forced take by an admin and the anchor's self-handover each stand alone.
    for taker in [Some(SECOND), None] {
        let (mut log, base) = Log::new();
        let taken = match taker {
            Some(device) => log.take(&base, device),
            None => log.self_handover(&base),
        };
        let selected = log.assert_selects(&taken);
        assert!(live(&selected, ANCHOR_DEVICE_ID));
        assert!(selected.admins.contains(ANCHOR_DEVICE_ID));
    }
    // Two of them out of one instance freeze at the common ancestor of their parents.
    let (mut log, base) = Log::new();
    let next = log.append(
        &base,
        &anchor_admin(&person()),
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let taken = log.take(&next, SECOND);
    let restored = log.self_handover(&base);
    log.assert_frozen(&base);
    // The anchor yields to the take.
    log.yield_to(&restored, &anchor_admin(&person()), &taken);
    log.assert_selects(&taken);
}

#[test]
fn a_restored_anchor_racing_a_planned_move_resolves_by_either_yield() {
    let build = || {
        let (mut log, base) = Log::new();
        let point = log.append(
            &base,
            &anchor_admin(&person()),
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let taken = log.take(&point, SECOND);
        let restored = log.self_handover(&base);
        (log, base, taken, restored)
    };
    let (log, base, ..) = build();
    log.assert_frozen(&base);

    let (mut log, _, taken, restored) = build();
    log.yield_to(&restored, &anchor_admin(&person()), &taken);
    log.assert_selects(&taken);

    let (mut log, _, taken, restored) = build();
    log.yield_to(&taken, &device_admin(SECOND), &restored);
    log.assert_selects(&restored);

    // Crossed yields accept both, and the lower hash wins; its branch continues with the yield
    // sealed on it.
    let (mut log, _, taken, restored) = build();
    let on_restored = log.yield_to(&restored, &anchor_admin(&person()), &taken);
    let on_taken = log.yield_to(&taken, &device_admin(SECOND), &restored);
    let lower = if taken.hash < restored.hash {
        on_taken
    } else {
        on_restored
    };
    log.assert_selects(&lower);
}

#[test]
fn a_returning_anchor_objects_to_a_take_or_yields_to_it() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let point = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let taken = log.take(&point, SECOND);
    let ousted = log.append(&taken, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
    let unseen = log.append(&point, &anchor, admin_entry(7));
    let selected = log.assert_selects(&ousted);
    assert!(removed(&selected, ANCHOR_DEVICE_ID));
    assert!(!selected.devices.contains_key(&device_id(7)));

    let objection = log.append(&unseen, &anchor, remove(&device_id(SECOND)));
    let selected = log.assert_frozen(&point);
    assert!(selected.held.contains(ANCHOR_DEVICE_ID));
    assert!(selected.held.contains(&device_id(SECOND)));

    log.yield_to(&objection, &anchor, &taken);
    log.assert_selects(&ousted);
}

#[test]
fn concurrent_forced_takes_freeze_at_their_common_point_until_one_yields() {
    for same_point in [true, false] {
        let (mut log, base) = Log::new();
        let next = log.append(
            &base,
            &anchor_admin(&person()),
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let first = log.take(if same_point { &next } else { &base }, FIRST);
        let second = log.take(&next, SECOND);
        log.assert_frozen(if same_point { &next } else { &base });
        log.yield_to(&first, &device_admin(FIRST), &second);
        log.assert_selects(&second);
    }
}

#[test]
fn a_single_admin_space_restores_by_self_handover_and_recovers_by_kit() {
    let (mut log, head) = Log::single();
    let restored = log.self_handover(&head);
    log.assert_selects(&restored);

    // A member's key is no admin, so its take never enters the tree.
    let (mut log, head) = Log::single();
    log.take(&head, MEMBER);
    log.assert_selects(&head);

    let kit = log.kit(&head, &device_id(MEMBER));
    log.assert_selects(&kit);
}

#[test]
fn a_stale_removed_admins_take_freezes_until_the_kit_continues_the_removal() {
    for fresh in [false, true] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let next = log.append(
            &base,
            &anchor,
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let removal = log.append(&next, &anchor, remove(&device_id(FIRST)));
        log.attacker(fresh, &removal);
        let stale = log.take(&next, FIRST);
        log.append(
            &stale,
            &device_admin(FIRST),
            enroll_entry(&person(), 9, DeviceRole::Member),
        );
        let selected = log.assert_frozen(&next);
        assert!(live(&selected, &device_id(FIRST)));
        assert!(selected.held.contains(&device_id(FIRST)));
        assert!(!selected.devices.contains_key(&device_id(9)));

        // The kit continues from the head of the objection branch.
        let kit = log.kit(&removal, &device_id(MEMBER));
        let selected = log.assert_selects(&kit);
        assert!(removed(&selected, &device_id(FIRST)));
        assert!(!selected.devices.contains_key(&device_id(9)));

        // The kit pins its ancestry against later takes out of the same instance.
        log.take(&next, FIRST);
        log.take(&base, FIRST);
        log.take(&base, SECOND);
        log.assert_selects(&kit);
    }
}

#[test]
fn an_ousted_anchors_objection_freezes_and_holds_both_until_the_kit() {
    for fresh in [false, true] {
        let (mut log, base) = Log::new();
        let point = log.append(
            &base,
            &anchor_admin(&person()),
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let taken = log.take(&point, SECOND);
        let ousted = log.append(&taken, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
        log.attacker(fresh, &point);
        log.append(&point, &anchor_admin(&person()), remove(&device_id(SECOND)));
        let selected = log.assert_frozen(&point);
        assert!(selected.held.contains(ANCHOR_DEVICE_ID));
        assert!(selected.held.contains(&device_id(SECOND)));

        let kit = log.kit(&ousted, &device_id(SECOND));
        let selected = log.assert_selects(&kit);
        assert!(removed(&selected, ANCHOR_DEVICE_ID));
        assert!(live(&selected, &device_id(SECOND)));
    }
}

#[test]
fn a_planted_admins_take_stands_until_the_anchor_objects() {
    let (mut log, head) = Log::new();
    let taken = log.take(&head, FIRST);
    let cleared = log.append(&taken, &device_admin(FIRST), remove(&device_id(SECOND)));
    log.assert_selects(&cleared);

    log.append(&head, &anchor_admin(&person()), remove(&device_id(FIRST)));
    let selected = log.assert_frozen(&head);
    assert!(live(&selected, &device_id(SECOND)));

    let kit = log.kit(&head, &device_id(MEMBER));
    let selected = log.assert_selects(&kit);
    assert!(live(&selected, &device_id(FIRST)));
    assert!(!selected.admins.contains(&device_id(FIRST)));
}

#[test]
fn a_restored_admins_second_take_is_withdrawn_by_its_yield_to_the_first() {
    let (mut log, base) = Log::new();
    let point = log.append(
        &base,
        &anchor_admin(&person()),
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let first = log.take(&point, SECOND);
    let second = log.take(&base, SECOND);
    log.assert_frozen(&base);
    log.yield_to(&second, &device_admin(SECOND), &first);
    log.assert_selects(&first);
    // A yield binds the key for the whole instance, takes it stores later included.
    log.take(&point, SECOND);
    log.assert_selects(&first);
}

#[test]
fn a_take_beside_a_clone_stands_and_the_other_clone_branch_is_not_effective() {
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    log.freezes
        .insert(ANCHOR_DEVICE_ID.to_owned(), vec![base.revision]);
    let one = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 5, DeviceRole::Member),
    );
    log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 6, DeviceRole::Member),
    );
    let selected = log.assert_frozen(&base);
    assert!(!selected.devices.contains_key(&device_id(5)));
    let taken = log.take(&one, SECOND);
    let selected = log.assert_selects(&taken);
    assert!(live(&selected, &device_id(5)));
    assert!(!selected.devices.contains_key(&device_id(6)));
}

#[test]
fn a_removed_old_anchors_fork_below_a_take_freezes_only_once_it_contests() {
    for fresh in [false, true] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let shared = log.append(
            &base,
            &anchor,
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let point = log.append(
            &shared,
            &anchor,
            enroll_entry(&person(), 5, DeviceRole::Member),
        );
        let taken = log.take(&point, SECOND);
        let ousted = log.append(&taken, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
        log.freezes
            .insert(ANCHOR_DEVICE_ID.to_owned(), vec![shared.revision]);
        let selected = log.assert_selects(&ousted);
        assert!(removed(&selected, ANCHOR_DEVICE_ID));

        if fresh {
            log.writer = writer(9);
        }
        log.self_handover(&shared);
        let selected = log.assert_frozen(&shared);
        assert_eq!(selected.anchor, ANCHOR_DEVICE_ID);
        assert!(selected.held.contains(ANCHOR_DEVICE_ID));

        let kit = log.kit(&ousted, &device_id(MEMBER));
        let selected = log.assert_selects(&kit);
        assert!(removed(&selected, ANCHOR_DEVICE_ID));
    }
}

#[test]
fn the_kit_recovers_a_clone_freeze_and_demotes_every_other_admin() {
    let (mut log, base) = Log::new();
    log.freezes
        .insert(ANCHOR_DEVICE_ID.to_owned(), vec![base.revision]);
    log.append(
        &base,
        &anchor_admin(&person()),
        enroll_entry(&person(), 5, DeviceRole::Member),
    );
    log.assert_frozen(&base);
    let kit = log.kit(&base, &device_id(MEMBER));
    let selected = log.assert_selects(&kit);
    assert!(selected.admins.is_empty(), "admins {:?}", selected.admins);
    assert_eq!(selected.kit_handovers, 1);
}

#[test]
fn concurrent_kit_uses_converge_on_the_higher_ranked_one() {
    for same_point in [true, false] {
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let one = log.append(
            &base,
            &anchor,
            enroll_entry(&person(), 5, DeviceRole::Member),
        );
        let two = if same_point {
            one.clone()
        } else {
            log.append(
                &base,
                &anchor,
                enroll_entry(&person(), 6, DeviceRole::Member),
            )
        };
        let to_first = log.kit(&one, &device_id(FIRST));
        let to_member = log.kit(&two, &device_id(MEMBER));
        let selected = log.assert_agreed();
        assert!(selected.head == to_first.hash || selected.head == to_member.hash);
        assert_eq!(selected.frozen_at, None);
        assert_eq!(selected.kit_handovers, 1);
    }
}

#[test]
fn a_kit_handover_that_precedes_its_rotation_pins_and_the_old_key_signs_nothing_after_it() {
    let (mut log, base) = Log::new();
    let leaked = recovery_admin(&person());
    let kit = log.kit(&base, &device_id(MEMBER));
    let rotated = log.append(&kit, &leaked, rotate_recovery(&other_recovery(92)));
    let selected = log.assert_selects(&rotated);
    assert_eq!(selected.kit_handovers, 1);
    log.handover(&rotated, &leaked, &device_id(MEMBER));
    log.assert_selects(&rotated);
}

#[test]
fn a_leaked_old_kit_on_a_side_branch_is_an_objected_candidate_once_the_rotation_is_stored() {
    let leaked = recovery_admin(&person());
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let side = log.kit(&base, &device_id(FIRST));
    let honest = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 5, DeviceRole::Member),
    );
    let selected = log.assert_selects(&side);
    assert_eq!(selected.kit_handovers, 1);

    // The rotation the honest chain stores vetoes the side branch's kit handover, objects to
    // it by revoking its key, and the anchor's removal of its target objects too.
    let rotated = log.append(&honest, &leaked, rotate_recovery(&other_recovery(92)));
    log.append(&rotated, &anchor, remove(&device_id(FIRST)));
    let selected = log.assert_frozen(&base);
    assert_eq!(selected.kit_handovers, 0);
    assert!(selected.held.contains(&device_id(FIRST)));
}

#[test]
fn a_leaked_old_kits_second_rotation_vetoes_both_and_leaves_no_kit_lineage() {
    let leaked = recovery_admin(&person());
    let current = other_recovery(92);
    let stolen = other_recovery(93);
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    let rotated = log.append(&base, &leaked, rotate_recovery(&current));
    let side = log.append(
        &base,
        &anchor,
        enroll_entry(&person(), 5, DeviceRole::Member),
    );
    let second = log.append(&side, &leaked, rotate_recovery(&stolen));
    let by_stolen = log.handover(&second, &stolen, &device_id(FIRST));
    let by_current = log.handover(&rotated, &current, &device_id(MEMBER));
    let selected = log.assert_agreed();
    assert_eq!(selected.head, side.hash);
    assert_ne!(selected.head, by_stolen.hash);
    assert_ne!(selected.head, by_current.hash);
    assert_eq!(selected.kit_handovers, 0);
    assert_eq!(
        selected.recovery_admin_key,
        recovery_admin(&person()).public_key()
    );
}

#[test]
fn a_leaked_kits_conflicting_rotation_turns_its_recovery_into_a_freeze() {
    for fresh in [false, true] {
        let leaked = recovery_admin(&person());
        let (mut log, base) = Log::new();
        let anchor = anchor_admin(&person());
        let next = log.append(
            &base,
            &anchor,
            enroll_entry(&person(), 4, DeviceRole::Member),
        );
        let recovered = log.kit(&next, &device_id(MEMBER));
        let ousted = log.append(&recovered, &leaked, remove(ANCHOR_DEVICE_ID));
        log.append(&ousted, &leaked, rotate_recovery(&other_recovery(92)));
        log.assert_selects(&log_head(&log));
        log.attacker(fresh, &next);
        log.append(&base, &leaked, rotate_recovery(&other_recovery(93)));
        log.self_handover(&next);
        let selected = log.assert_frozen(&next);
        assert_eq!(selected.kit_handovers, 0);
        assert!(selected.held.contains(ANCHOR_DEVICE_ID));
    }
}

/// The head the last stored record reaches.
fn log_head(log: &Log) -> At {
    let record = log.records.last().unwrap();
    At {
        epoch: record.epoch,
        revision: record.revision,
        hash: record.hash(),
        anchor: record.anchor_device_id.clone(),
    }
}

#[test]
fn only_the_anchor_installs_a_recovery_key_and_the_printed_kit_outranks_it() {
    let installed = other_recovery(91);
    let (mut log, base) = Log::new();
    let anchor = anchor_admin(&person());
    log.append(&base, &device_admin(FIRST), rotate_recovery(&installed));
    log.assert_selects(&base);

    let install = log.append(&base, &anchor, rotate_recovery(&installed));
    log.assert_selects(&install);
    let recovered = log.handover(&install, &installed, &device_id(MEMBER));
    let selected = log.assert_selects(&recovered);
    assert!(selected.admins.is_empty(), "admins {:?}", selected.admins);
    assert_eq!(selected.kit_handovers, 0);
    let promoted = log.append(&recovered, &installed, promote(MEMBER));
    let selected = log.assert_selects(&promoted);
    assert!(selected.admins.contains(&device_id(MEMBER)));

    // Beside an admin's take the installed key's handover contests.
    let taken = log.take(&install, SECOND);
    log.assert_frozen(&install);

    // The printed kit beside both pins and resets the installed key.
    let kit = log.kit(&install, &device_id(FIRST));
    let selected = log.assert_selects(&kit);
    assert_eq!(
        selected.recovery_admin_key,
        recovery_admin(&person()).public_key()
    );

    // Once an admin's take wins, the installed key signs nothing in its instance.
    let (mut log, base) = Log::new();
    let install = log.append(&base, &anchor, rotate_recovery(&installed));
    let taken_alone = log.take(&install, SECOND);
    log.append(
        &taken_alone,
        &installed,
        enroll_entry(&person(), 9, DeviceRole::Member),
    );
    let selected = log.assert_selects(&taken_alone);
    assert!(!selected.devices.contains_key(&device_id(9)));
    let _ = taken;
}

#[test]
fn an_ousted_anchors_installed_key_only_contests_its_old_instance() {
    for fresh in [false, true] {
        let installed = other_recovery(91);
        let (mut log, base) = Log::new();
        let install = log.append(&base, &anchor_admin(&person()), rotate_recovery(&installed));
        let taken = log.take(&install, SECOND);
        log.append(&taken, &device_admin(SECOND), remove(ANCHOR_DEVICE_ID));
        log.attacker(fresh, &install);
        let own = log.handover(&install, &installed, ANCHOR_DEVICE_ID);
        log.handover(&own, &installed, ANCHOR_DEVICE_ID);
        let selected = log.assert_frozen(&install);
        assert!(selected.held.contains(ANCHOR_DEVICE_ID));
        assert_eq!(selected.kit_handovers, 0);
    }
}

#[test]
fn a_single_admin_recovers_with_an_installed_key_unless_a_thief_contests() {
    let installed = other_recovery(91);
    let (mut log, head) = Log::single();
    let install = log.append(&head, &anchor_admin(&person()), rotate_recovery(&installed));
    let recovered = log.handover(&install, &installed, &device_id(MEMBER));
    let promoted = log.append(&recovered, &installed, promote(MEMBER));
    let cleared = log.append(&promoted, &installed, remove(ANCHOR_DEVICE_ID));
    let selected = log.assert_selects(&cleared);
    assert!(selected.admins.contains(&device_id(MEMBER)));
    assert!(removed(&selected, ANCHOR_DEVICE_ID));

    // A thief holding the anchor's device contests with its self-handover; the kit wins.
    log.self_handover(&install);
    log.assert_frozen(&install);
    let kit = log.kit(&install, &device_id(MEMBER));
    let cleared = log.append(&kit, &recovery_admin(&person()), remove(ANCHOR_DEVICE_ID));
    let selected = log.assert_selects(&cleared);
    assert!(removed(&selected, ANCHOR_DEVICE_ID));
}

#[test]
fn a_yield_withdraws_only_candidates_of_its_own_instance() {
    let (mut log, base) = Log::new();
    let next = log.append(
        &base,
        &anchor_admin(&person()),
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    let first = log.take(&next, FIRST);
    let second = log.take(&next, SECOND);
    log.yield_to(&first, &device_admin(FIRST), &second);
    log.assert_selects(&second);
    let later = log.take(&second, FIRST);
    log.assert_selects(&later);
}

#[test]
fn a_restored_anchor_beside_a_planted_take_freezes_rather_than_withdrawing() {
    let (mut log, base) = Log::new();
    let next = log.append(
        &base,
        &anchor_admin(&person()),
        enroll_entry(&person(), 4, DeviceRole::Member),
    );
    log.take(&next, FIRST);
    log.self_handover(&base);
    let selected = log.assert_frozen(&base);
    assert_eq!(selected.anchor, ANCHOR_DEVICE_ID);
}
