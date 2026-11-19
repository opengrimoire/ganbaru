use super::*;
use ganbaru_sync::ForkedWriter;

const MIB: u64 = 1024 * 1024;

fn storage(
    bytes: u64,
    by_device: Vec<(&str, u64)>,
    forked: Vec<ForkedWriter>,
) -> AuthorizationStorage {
    AuthorizationStorage {
        bytes,
        envelopes: 1,
        by_device: by_device
            .into_iter()
            .map(|(device, bytes)| (device.to_string(), bytes))
            .collect(),
        forked_writers: forked,
    }
}

#[test]
fn a_low_budget_preference_stays_between_the_minimum_and_the_default() {
    let limits = RecordLimits::default();
    assert_eq!(limits.low_budget(None), LOW_BUDGET);
    let tiny = RecordBudget {
        bytes: 1,
        envelopes: 1,
    };
    assert_eq!(limits.low_budget(Some(tiny)), LOW_BUDGET_MIN);
    assert_eq!(limits.low_budget(Some(NORMAL_BUDGET)), LOW_BUDGET);
    let between = RecordBudget {
        bytes: 32 * 1024,
        envelopes: 32,
    };
    assert_eq!(limits.low_budget(Some(between)), between);
}

#[test]
fn a_window_spends_until_its_budget_and_carries_overspend_into_later_windows() {
    let start = Instant::now();
    let mut window = Window::new(start);
    window.spend(NORMAL_BUDGET.bytes - 10, 5);
    assert_eq!(
        window.left(NORMAL_BUDGET),
        RecordBudget {
            bytes: 10,
            envelopes: NORMAL_BUDGET.envelopes - 5,
        }
    );
    window.spend(100, NORMAL_BUDGET.envelopes);
    assert_eq!(window.left(NORMAL_BUDGET).bytes, 0);
    assert_eq!(window.left(NORMAL_BUDGET).envelopes, 0);
    let same = window.at(start + RECORD_WINDOW / 2, RECORD_WINDOW, NORMAL_BUDGET);
    assert_eq!(same.left(NORMAL_BUDGET).bytes, 0);
    // The page taken past the budget is debt the next window starts with.
    let next = window.at(start + RECORD_WINDOW, RECORD_WINDOW, NORMAL_BUDGET);
    assert_eq!(
        next.left(NORMAL_BUDGET),
        RecordBudget {
            bytes: NORMAL_BUDGET.bytes - 90,
            envelopes: NORMAL_BUDGET.envelopes - 5,
        }
    );
    let later = window.at(start + RECORD_WINDOW * 2, RECORD_WINDOW, NORMAL_BUDGET);
    assert_eq!(later.left(NORMAL_BUDGET), NORMAL_BUDGET);
}

#[test]
fn the_backstop_keeps_part_of_its_reserve_for_kit_records() {
    let limits = RecordLimits::default();
    let shared = BACKSTOP_RESERVE - BACKSTOP_KIT_RESERVE;
    let mut backstop = Backstop::default();
    assert!(backstop.charge(&limits, RecordClass::Contest, shared - MIB, true));
    // Contest and wanted records cannot reach into the kit part, and their miss leaves it
    // open.
    assert!(!backstop.charge(&limits, RecordClass::Wanted, 2 * MIB, true));
    assert!(!backstop.stopped);
    assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_KIT_RESERVE + MIB, true));
    // A kit-charged envelope the binding does not fix, such as an ancestor, misses without
    // stopping.
    assert!(!backstop.charge(&limits, RecordClass::Kit, 1, false));
    assert!(!backstop.stopped);
    assert!(!backstop.charge(&limits, RecordClass::Kit, 1, true));
    assert!(backstop.stopped);

    // A full shared part still admits kit records up to the whole reserve.
    let mut backstop = Backstop::default();
    assert!(backstop.charge(&limits, RecordClass::Contest, shared, true));
    assert!(!backstop.charge(&limits, RecordClass::Contest, 1, true));
    assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_KIT_RESERVE, true));
    assert!(!backstop.stopped);

    // Kit records may spend the whole reserve.
    let mut backstop = Backstop::default();
    assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_RESERVE, true));
    assert!(!backstop.stopped);
}

#[test]
fn the_backstop_filter_stores_only_kit_contest_and_wanted_records() {
    let state = RecordExchange::default();
    let envelopes = vec![vec![1; 8], vec![2; 8], vec![3; 8], vec![4; 8], vec![5; 8]];
    let classes = RECORD_CLASSES_IN_ORDER;
    assert_eq!(state.backstop_filter(envelopes.clone(), &classes).len(), 5);
    state.lock().backstop = Some(Backstop::default());
    assert_eq!(
        state.backstop_filter(envelopes.clone(), &classes),
        vec![vec![1; 8], vec![2; 8], vec![3; 8]]
    );
    state.lock().backstop = Some(Backstop {
        stopped: true,
        ..Backstop::default()
    });
    assert!(state.backstop_filter(envelopes, &classes).is_empty());
    state.resume();
    assert!(state.lock().backstop.is_none());
}

const RECORD_CLASSES_IN_ORDER: [RecordClass; 5] = [
    RecordClass::Kit,
    RecordClass::Contest,
    RecordClass::Wanted,
    RecordClass::Live,
    RecordClass::Rest,
];

#[test]
fn growth_alerts_name_heavy_devices_a_heavy_store_and_forked_writers() {
    assert!(
        growth_alerts(
            &storage(MIB, vec![("phone", MIB)], vec![]),
            &RecordLimits::default()
        )
        .is_empty()
    );
    let writer = WriterId::from_bytes([7; 16]);
    let alerts = growth_alerts(
        &storage(
            ALERT_TOTAL_BYTES + 1,
            vec![
                ("flooder", ALERT_DEVICE_BYTES + 1),
                ("phone", ALERT_DEVICE_BYTES),
            ],
            vec![ForkedWriter {
                writer,
                device_id: Some("forker".to_string()),
            }],
        ),
        &RecordLimits::default(),
    );
    assert_eq!(
        alerts,
        vec![
            GrowthAlert::Device {
                device_id: "flooder".to_string(),
                bytes: ALERT_DEVICE_BYTES + 1,
            },
            GrowthAlert::Total {
                bytes: ALERT_TOTAL_BYTES + 1,
            },
            GrowthAlert::ForkedWriter {
                writer,
                device_id: Some("forker".to_string()),
            },
        ]
    );
}

#[test]
fn a_session_with_a_new_peer_lets_every_wanted_entry_be_asked_of_it_again() {
    let state = RecordExchange::default();
    let waiting = Backoff {
        next: 100,
        wait: 64,
    };
    for peer in ["hub", "laptop"] {
        state
            .lock()
            .backoff
            .insert(peer.to_string(), BTreeMap::from([(vec![1], waiting)]));
    }
    state
        .lock()
        .pulled
        .insert("hub".to_string(), RecordCursor::default());
    state.start_session("hub");
    assert_eq!(state.backed_off("hub"), BTreeSet::from([vec![1]]));
    state.start_session("laptop");
    assert!(state.backed_off("laptop").is_empty());
    assert_eq!(state.backed_off("hub"), BTreeSet::from([vec![1]]));
    state.disconnected("hub");
    state.start_session("hub");
    assert!(state.backed_off("hub").is_empty());
}
