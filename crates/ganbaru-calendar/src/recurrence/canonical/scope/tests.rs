use super::*;

fn date(value: &str) -> NaiveDate {
    parse_date(value).unwrap()
}

fn now(value: &str) -> ScopeClock {
    let epoch_ms = DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis();
    ScopeClock {
        epoch_ms,
        floating_today: None,
    }
}

fn source(rule: &str) -> StoredTemplate<'_> {
    StoredTemplate {
        id: "series",
        start: "2026-05-01T09:00:00Z",
        end: "2026-05-01T10:00:00Z",
        home_zone: "UTC",
        all_day: false,
        rrule: Some(rule),
        repeat_until: None,
        exceptions: &[],
        rdates: &[],
        overrides: Vec::new(),
    }
}

#[test]
fn selected_active_run_forces_this_without_enumerating_unrelated_decades() {
    let mut stored = source("FREQ=DAILY");
    stored.start = "1900-05-01T09:00:00Z";
    stored.end = "1900-05-01T10:00:00Z";
    let plan = Template::from_stored(stored)
        .unwrap()
        .plan_scope(
            date("2026-05-10"),
            EditScope::All,
            ScopeEvidence {
                active: Some(("live-run".into(), date("2026-05-10"))),
                ..ScopeEvidence::default()
            },
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.effective_scope, EditScope::This);
    assert!(plan.selected_active && plan.selected_started);
    assert_eq!(plan.active_run_id.as_deref(), Some("live-run"));
    assert!(plan.preserve.is_empty());
}

#[test]
fn all_preserves_later_history_without_freezing_earlier_mutable_occurrences() {
    let plan = Template::from_stored(source("FREQ=DAILY;COUNT=60"))
        .unwrap()
        .plan_scope(
            date("2026-05-15"),
            EditScope::All,
            ScopeEvidence {
                history_dates: BTreeSet::from([date("2026-06-01")]),
                ..ScopeEvidence::default()
            },
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.protected_through.as_deref(), Some("2026-05-10"));
    assert_eq!(plan.first_mutable.unwrap().recurrence_date, "2026-05-11");
    assert_eq!(plan.preserve.len(), 1);
    assert_eq!(plan.preserve[0].recurrence_date, "2026-06-01");
    assert!(!plan.selected_has_history);
}

#[test]
fn moved_future_occurrence_starts_the_mutable_side_before_later_started_identities() {
    let mut stored = source("FREQ=DAILY;COUNT=30");
    stored.overrides.push(StoredOverride {
        recurrence_id: "2026-05-02".into(),
        start: Some("2026-06-01T09:00:00Z".into()),
        end: Some("2026-06-01T10:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let plan = Template::from_stored(stored)
        .unwrap()
        .plan_scope(
            date("2026-05-15"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.protected_through.as_deref(), Some("2026-05-01"));
    assert_eq!(plan.first_mutable.unwrap().recurrence_date, "2026-05-02");
    assert_eq!(plan.preserve.len(), 8);
    assert_eq!(plan.preserve.first().unwrap().recurrence_date, "2026-05-03");
    assert_eq!(plan.preserve.last().unwrap().recurrence_date, "2026-05-10");
}

#[test]
fn protected_boundary_uses_the_home_date_and_exact_start_instant() {
    let mut stored = source("FREQ=DAILY;COUNT=3");
    stored.home_zone = "America/Monterrey";
    stored.start = "2026-05-02T02:00:00Z";
    stored.end = "2026-05-02T03:00:00Z";
    let template = Template::from_stored(stored).unwrap();
    let before = template
        .plan_scope(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-02T01:59:59.999Z"),
        )
        .unwrap();
    assert!(before.protected_through.is_none());
    assert_eq!(before.first_mutable.unwrap().recurrence_date, "2026-05-01");
    let started = template
        .plan_scope(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-02T02:00:00Z"),
        )
        .unwrap();
    assert_eq!(started.protected_through.as_deref(), Some("2026-05-01"));
    assert_eq!(started.first_mutable.unwrap().recurrence_date, "2026-05-02");
}

#[test]
fn floating_all_day_protection_uses_the_native_device_date_instead_of_utc_midnight() {
    let mut stored = source("FREQ=DAILY;COUNT=3");
    stored.all_day = true;
    stored.start = "2026-05-01";
    stored.end = "2026-05-01";
    let template = Template::from_stored(stored).unwrap();
    let mut clock = now("2026-05-02T01:00:00Z");
    let unavailable = template.plan_scope(
        date("2026-05-02"),
        EditScope::All,
        ScopeEvidence::default(),
        clock,
    );
    assert!(unavailable.unwrap_err().contains("native device date"));
    clock.floating_today = Some(date("2026-05-01"));
    let plan = template
        .plan_scope(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            clock,
        )
        .unwrap();
    assert!(!plan.selected_started);
    assert_eq!(plan.protected_through.as_deref(), Some("2026-05-01"));
    assert_eq!(plan.first_mutable.unwrap().recurrence_date, "2026-05-02");
}

#[test]
fn following_preserves_started_history_later_active_and_moved_past_occurrences() {
    let mut stored = source("FREQ=DAILY;COUNT=30");
    stored.overrides.push(StoredOverride {
        recurrence_id: "2026-05-15".into(),
        start: Some("2026-04-01T09:00:00Z".into()),
        end: Some("2026-04-01T10:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let plan = Template::from_stored(stored)
        .unwrap()
        .plan_scope(
            date("2026-05-08"),
            EditScope::Following,
            ScopeEvidence {
                active: Some(("live-run".into(), date("2026-05-12"))),
                history_dates: BTreeSet::from([date("2026-05-20")]),
            },
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.effective_scope, EditScope::Following);
    assert_eq!(
        plan.preserve
            .iter()
            .map(|item| item.recurrence_date.as_str())
            .collect::<Vec<_>>(),
        [
            "2026-05-08",
            "2026-05-09",
            "2026-05-10",
            "2026-05-12",
            "2026-05-15",
            "2026-05-20"
        ]
    );
    assert_eq!(
        plan.preserve[4].start_ms,
        now("2026-04-01T09:00:00Z").epoch_ms
    );
}

#[test]
fn exhausted_count_is_proven_without_searching_an_arbitrary_future_horizon() {
    let plan = Template::from_stored(source("FREQ=DAILY;COUNT=3"))
        .unwrap()
        .plan_scope(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.protected_through.as_deref(), Some("2026-05-03"));
    assert!(plan.first_mutable.is_none());
}

#[test]
fn independent_rdate_remains_mutable_after_rrule_exhaustion_and_exclusions() {
    let mut stored = source("FREQ=DAILY;COUNT=3");
    let rdates = vec!["2026-05-20".into(), "2026-06-20".into()];
    let exceptions = vec!["2026-05-20".into()];
    stored.rdates = &rdates;
    stored.exceptions = &exceptions;
    let plan = Template::from_stored(stored)
        .unwrap()
        .plan_scope(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.first_mutable.unwrap().recurrence_date, "2026-06-20");
}

#[test]
fn sparse_recurrence_is_not_mistaken_for_exhaustion() {
    let plan = Template::from_stored(source("FREQ=YEARLY;INTERVAL=40;COUNT=2"))
        .unwrap()
        .plan_scope(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert_eq!(plan.first_mutable.unwrap().recurrence_date, "2066-05-01");
}

#[test]
fn rdate_before_dtstart_does_not_hide_a_later_explicit_anchor() {
    for rule in ["FREQ=YEARLY;COUNT=1", "FREQ=YEARLY;UNTIL=20260501"] {
        let mut stored = source(rule);
        stored.start = "2066-05-01T09:00:00Z";
        stored.end = "2066-05-01T10:00:00Z";
        let rdates = vec!["2026-05-01".into()];
        stored.rdates = &rdates;
        let plan = Template::from_stored(stored)
            .unwrap()
            .plan_scope(
                date("2026-05-01"),
                EditScope::All,
                ScopeEvidence::default(),
                now("2026-05-10T09:30:00Z"),
            )
            .unwrap();
        assert_eq!(
            plan.first_mutable.unwrap().recurrence_date,
            "2066-05-01",
            "{rule}"
        );
    }
}

#[test]
fn impossible_future_rule_has_a_budget_error_instead_of_an_empty_plan() {
    let plan = Template::from_stored(source("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30"))
        .unwrap()
        .plan_scope(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        );
    assert!(plan.unwrap_err().contains("candidate budget"));
}

#[test]
fn until_proves_exhaustion_even_when_no_subsequent_date_matches() {
    let plan = Template::from_stored(source("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30;UNTIL=20260601"))
        .unwrap()
        .plan_scope(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            now("2026-05-10T09:30:00Z"),
        )
        .unwrap();
    assert!(plan.first_mutable.is_none());
}

#[test]
fn missing_active_identity_prevents_a_plan_that_could_orphan_execution() {
    let template = Template::from_stored(source("FREQ=DAILY;COUNT=3")).unwrap();
    let plan = template.plan_scope(
        date("2026-05-02"),
        EditScope::Following,
        ScopeEvidence {
            active: Some(("live-run".into(), date("2026-05-20"))),
            ..ScopeEvidence::default()
        },
        now("2026-05-01T08:00:00Z"),
    );
    assert!(
        plan.unwrap_err()
            .contains("Active Focus occurrence is missing")
    );
}
