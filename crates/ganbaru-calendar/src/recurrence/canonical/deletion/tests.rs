use super::*;

fn date(value: &str) -> NaiveDate {
    parse_date(value).unwrap()
}

fn clock(value: &str) -> ScopeClock {
    ScopeClock {
        epoch_ms: DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

fn source(rule: Option<&str>) -> StoredTemplate<'_> {
    StoredTemplate {
        id: "series",
        start: "2026-05-01T09:00:00Z",
        end: "2026-05-01T10:00:00Z",
        home_zone: "UTC",
        all_day: false,
        rrule: rule,
        repeat_until: None,
        exceptions: &[],
        rdates: &[],
        overrides: Vec::new(),
    }
}

fn moved(identity: &str, start: &str, end: &str) -> StoredOverride {
    StoredOverride {
        recurrence_id: identity.into(),
        start: Some(start.into()),
        end: Some(end.into()),
        cancelled: false,
        this_and_future: false,
    }
}

/// Reparse the planned stored side and compare actual recurrence identities.
/// Flags alone cannot prove COUNT, exceptions, moved overrides or RDATE survived.
fn remaining(template: &Template, plan: &DeletePlan, zone: &str) -> Vec<String> {
    let Some(side) = &plan.source_after else {
        return Vec::new();
    };
    let retained = Template::from_stored(StoredTemplate {
        id: "series",
        start: &side.start_time,
        end: &side.end_time,
        home_zone: zone,
        all_day: template.all_day,
        rrule: side.rrule.as_deref(),
        repeat_until: None,
        exceptions: &side.exceptions,
        rdates: &side.rdates,
        overrides: template
            .overrides
            .values()
            .filter(|(_, value)| side.override_recurrence_ids.contains(&value.recurrence_id))
            .map(|(_, value)| StoredOverride {
                recurrence_id: value.recurrence_id.clone(),
                start: value.start.clone(),
                end: value.end.clone(),
                cancelled: value.cancelled,
                this_and_future: value.this_and_future,
            })
            .collect(),
    })
    .unwrap();
    retained
        .identity_range(
            date("2026-04-01"),
            date("2035-12-31"),
            &mut ExpansionBudget::default(),
        )
        .unwrap()
        .0
        .into_iter()
        .map(|row| row.recurrence_date.to_string())
        .collect()
}

fn archived(plan: &DeletePlan) -> Vec<&str> {
    plan.archive_occurrences
        .iter()
        .map(|row| row.recurrence_date.as_str())
        .collect()
}

#[test]
fn future_all_retains_started_history_and_archives_only_protected_future_members() {
    let template = Template::from_stored(source(Some("FREQ=DAILY;COUNT=6"))).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-04"),
            EditScope::All,
            ScopeEvidence {
                history_dates: BTreeSet::from([date("2026-05-05")]),
                ..ScopeEvidence::default()
            },
            clock("2026-05-02T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(plan.outcome, DeleteOutcome::Mixed);
    assert_eq!(
        remaining(&template, &plan, "UTC"),
        ["2026-05-01", "2026-05-02"]
    );
    assert_eq!(archived(&plan), ["2026-05-05"]);
    assert_eq!(
        plan.valid_until_ms,
        Some(clock("2026-05-03T09:00:00Z").epoch_ms)
    );
}

#[test]
fn already_started_all_archives_started_members_and_leaves_later_mutable_members_live() {
    let template = Template::from_stored(source(Some("FREQ=DAILY;COUNT=5"))).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2026-05-02T09:00:00Z"),
            false,
        )
        .unwrap();
    assert!(plan.history_only);
    assert_eq!(plan.outcome, DeleteOutcome::Archive);
    assert_eq!(archived(&plan), ["2026-05-01", "2026-05-02"]);
    assert_eq!(
        remaining(&template, &plan, "UTC"),
        ["2026-05-03", "2026-05-04", "2026-05-05"]
    );
    assert_eq!(
        plan.valid_until_ms,
        Some(clock("2026-05-03T09:00:00Z").epoch_ms)
    );
}

#[test]
fn started_following_archives_only_scoped_started_members_in_original_identity_order() {
    let mut stored = source(Some("FREQ=DAILY;COUNT=5"));
    stored.overrides.push(moved(
        "2026-05-05",
        "2026-04-01T09:00:00Z",
        "2026-04-01T10:00:00Z",
    ));
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::Following,
            ScopeEvidence::default(),
            clock("2026-05-03T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(archived(&plan), ["2026-05-02", "2026-05-03", "2026-05-05"]);
    assert_eq!(
        remaining(&template, &plan, "UTC"),
        ["2026-05-01", "2026-05-04"]
    );
}

#[test]
fn future_all_drops_moved_future_members_without_losing_later_started_original_identities() {
    let mut stored = source(Some("FREQ=DAILY;COUNT=6"));
    stored.overrides.push(moved(
        "2026-05-02",
        "2026-06-01T09:00:00Z",
        "2026-06-01T10:00:00Z",
    ));
    stored.overrides.push(moved(
        "2026-05-06",
        "2026-04-01T09:00:00Z",
        "2026-04-01T10:00:00Z",
    ));
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-04"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2026-05-03T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(plan.outcome, DeleteOutcome::Delete);
    assert_eq!(
        remaining(&template, &plan, "UTC"),
        ["2026-05-01", "2026-05-03", "2026-05-06"]
    );
    assert!(plan.archive_occurrences.is_empty());
    assert_eq!(
        plan.valid_until_ms,
        Some(clock("2026-05-04T09:00:00Z").epoch_ms)
    );
}

#[test]
fn future_following_caps_at_the_original_identity_and_archives_later_moved_past_or_active_members()
{
    let mut stored = source(Some("FREQ=DAILY;COUNT=8"));
    stored.overrides.push(moved(
        "2026-05-07",
        "2026-04-01T09:00:00Z",
        "2026-04-01T10:00:00Z",
    ));
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-04"),
            EditScope::Following,
            ScopeEvidence {
                active: Some(("run".into(), date("2026-05-06"))),
                ..ScopeEvidence::default()
            },
            clock("2026-05-02T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(plan.effective_scope, EditScope::Following);
    assert_eq!(archived(&plan), ["2026-05-06", "2026-05-07"]);
    assert_eq!(
        remaining(&template, &plan, "UTC"),
        ["2026-05-01", "2026-05-02", "2026-05-03"]
    );
    assert_eq!(plan.active_run_to_stop.as_deref(), Some("run"));
}

#[test]
fn selecting_active_normalizes_to_this_without_enumerating_decades() {
    let mut stored = source(Some("FREQ=DAILY"));
    stored.start = "1900-05-01T09:00:00Z";
    stored.end = "1900-05-01T10:00:00Z";
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence {
                active: Some(("run".into(), date("2026-05-02"))),
                ..ScopeEvidence::default()
            },
            clock("2026-05-02T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(plan.effective_scope, EditScope::This);
    assert_eq!(archived(&plan), ["2026-05-02"]);
    assert_eq!(plan.active_run_to_stop.as_deref(), Some("run"));
    assert_eq!(plan.source_after.unwrap().exceptions, ["2026-05-02"]);
}

#[test]
fn standalone_started_history_or_task_reference_requires_master_archive() {
    let template = Template::from_stored(source(None)).unwrap();
    for (now, history, reference, expected) in [
        ("2026-04-01T00:00:00Z", false, false, DeleteOutcome::Delete),
        ("2026-05-01T09:00:00Z", false, false, DeleteOutcome::Archive),
        ("2026-04-01T00:00:00Z", true, false, DeleteOutcome::Archive),
        ("2026-04-01T00:00:00Z", false, true, DeleteOutcome::Archive),
    ] {
        let evidence = ScopeEvidence {
            history_dates: if history {
                BTreeSet::from([date("2026-05-01")])
            } else {
                BTreeSet::new()
            },
            ..ScopeEvidence::default()
        };
        let plan = template
            .plan_delete(
                date("2026-05-01"),
                EditScope::All,
                evidence,
                clock(now),
                reference,
            )
            .unwrap();
        assert_eq!(plan.outcome, expected);
        assert_eq!(plan.archive_source, expected == DeleteOutcome::Archive);
        assert_eq!(plan.effective_scope, EditScope::This);
        assert!(plan.source_after.is_none());
    }
}

#[test]
fn native_device_date_controls_all_day_history_only_behavior() {
    let mut stored = source(Some("FREQ=DAILY;COUNT=3"));
    stored.all_day = true;
    stored.start = "2026-05-01";
    stored.end = "2026-05-01";
    let template = Template::from_stored(stored).unwrap();
    let mut now = clock("2026-05-02T01:00:00Z");
    assert!(
        template
            .plan_delete(
                date("2026-05-02"),
                EditScope::All,
                ScopeEvidence::default(),
                now,
                false
            )
            .unwrap_err()
            .contains("native device date")
    );
    now.floating_today = Some(date("2026-05-01"));
    let future = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            now,
            false,
        )
        .unwrap();
    assert!(!future.history_only);
    assert_eq!(remaining(&template, &future, "UTC"), ["2026-05-01"]);
    now.floating_today = Some(date("2026-05-02"));
    let started = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            now,
            false,
        )
        .unwrap();
    assert!(started.history_only);
    assert_eq!(remaining(&template, &started, "UTC"), ["2026-05-03"]);
}

#[test]
fn excluded_count_members_and_rdates_survive_history_only_deletion_without_revival() {
    let exceptions = vec!["2026-05-02".into()];
    let rdates = vec!["2026-05-20".into()];
    let mut stored = source(Some("FREQ=DAILY;COUNT=3"));
    stored.exceptions = &exceptions;
    stored.rdates = &rdates;
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2026-05-03T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(archived(&plan), ["2026-05-01", "2026-05-03"]);
    assert_eq!(remaining(&template, &plan, "UTC"), ["2026-05-20"]);
}

#[test]
fn future_all_with_durable_links_preserves_a_master_archive_and_started_prefix() {
    let template = Template::from_stored(source(Some("FREQ=DAILY;COUNT=3"))).unwrap();
    let plan = template
        .plan_delete(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2026-05-01T09:30:00Z"),
            true,
        )
        .unwrap();
    assert!(plan.archive_source);
    assert_eq!(plan.outcome, DeleteOutcome::Archive);
    assert_eq!(remaining(&template, &plan, "UTC"), ["2026-05-01"]);
}

#[test]
fn impossible_future_search_and_oversized_history_fail_instead_of_returning_partial_plans() {
    for (rule, expected) in [
        ("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30", "candidate budget"),
        ("FREQ=DAILY", "10000 occurrences"),
    ] {
        let template = Template::from_stored(source(Some(rule))).unwrap();
        let plan = template.plan_delete(
            date("2026-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            clock(if rule == "FREQ=DAILY" {
                "2060-05-01T09:30:00Z"
            } else {
                "2026-04-01T00:00:00Z"
            }),
            false,
        );
        // The impossible rule has an explicit unstarted anchor, so deleting the
        // entire source needs no future search. Add anchor history to force the
        // meaningful mutable-tail exhaustion check below.
        if rule == "FREQ=DAILY" {
            assert!(plan.unwrap_err().contains(expected));
        } else {
            let error = template
                .plan_delete(
                    date("2026-05-01"),
                    EditScope::All,
                    ScopeEvidence {
                        history_dates: BTreeSet::from([date("2026-05-01")]),
                        ..ScopeEvidence::default()
                    },
                    clock("2026-04-01T00:00:00Z"),
                    false,
                )
                .unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
    }
}

#[test]
fn missing_selected_or_active_identity_cannot_produce_a_deletion_plan() {
    let template = Template::from_stored(source(Some("FREQ=DAILY;COUNT=3"))).unwrap();
    assert!(
        template
            .plan_delete(
                date("2026-05-04"),
                EditScope::All,
                ScopeEvidence::default(),
                clock("2026-04-01T00:00:00Z"),
                false
            )
            .unwrap_err()
            .contains("no longer exists")
    );
    assert!(
        template
            .plan_delete(
                date("2026-05-02"),
                EditScope::All,
                ScopeEvidence {
                    active: Some(("run".into(), date("2026-05-04"))),
                    ..ScopeEvidence::default()
                },
                clock("2026-04-01T00:00:00Z"),
                false
            )
            .unwrap_err()
            .contains("Active Focus occurrence is missing")
    );
}

#[test]
fn a_pre_anchor_rdate_does_not_hide_the_mutable_anchor_decades_later() {
    let rdates = vec!["2026-05-01".into()];
    let mut stored = source(Some("FREQ=YEARLY;COUNT=1"));
    stored.start = "2066-05-01T09:00:00Z";
    stored.end = "2066-05-01T10:00:00Z";
    stored.rdates = &rdates;
    let template = Template::from_stored(stored).unwrap();
    let plan = template
        .plan_delete(
            date("2066-05-01"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2026-05-02T09:30:00Z"),
            false,
        )
        .unwrap();
    assert_eq!(remaining(&template, &plan, "UTC"), ["2026-05-01"]);
    assert_eq!(plan.outcome, DeleteOutcome::Delete);
    assert_eq!(
        plan.valid_until_ms,
        Some(clock("2066-05-01T09:00:00Z").epoch_ms)
    );
}

#[test]
fn standalone_source_history_is_protected_and_an_invalid_live_alias_is_rejected() {
    let template = Template::from_stored(source(None)).unwrap();
    let evidence = ScopeEvidence {
        history_dates: BTreeSet::from([date("2026-05-02")]),
        ..ScopeEvidence::default()
    };
    let plan = template
        .plan_delete(
            date("2026-05-01"),
            EditScope::All,
            evidence,
            clock("2026-04-01T00:00:00Z"),
            false,
        )
        .unwrap();
    assert!(plan.archive_source && plan.selected_has_history);
    let invalid = ScopeEvidence {
        active: Some(("run".into(), date("2026-05-02"))),
        ..ScopeEvidence::default()
    };
    assert!(
        template
            .plan_delete(
                date("2026-05-01"),
                EditScope::All,
                invalid,
                clock("2026-04-01T00:00:00Z"),
                false
            )
            .unwrap_err()
            .contains("Active Focus occurrence is missing")
    );
}

#[test]
fn retained_and_archived_identity_sets_match_scoped_deletion_across_recurrence_families() {
    for rule in [
        "FREQ=DAILY;INTERVAL=2;COUNT=8",
        "FREQ=WEEKLY;INTERVAL=2;COUNT=8",
        "FREQ=WEEKLY;BYDAY=MO,WE;COUNT=8",
        "FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=8",
        "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=8",
        "FREQ=MONTHLY;INTERVAL=2;COUNT=8",
        "FREQ=YEARLY;BYWEEKNO=2;BYDAY=MO;COUNT=8",
        "FREQ=YEARLY;BYYEARDAY=100;COUNT=8",
        "FREQ=YEARLY;BYMONTH=2,5;BYMONTHDAY=-1;COUNT=8",
        "FREQ=YEARLY;INTERVAL=2;COUNT=5",
    ] {
        let original = Template::from_stored(source(Some(rule))).unwrap();
        let seeds = original
            .identity_range(
                date("2026-01-01"),
                date("2035-12-31"),
                &mut ExpansionBudget::default(),
            )
            .unwrap()
            .0;
        let now_ms = seeds[2].start_ms + 1_000;
        let mut stored = source(Some(rule));
        for (identity, delta) in [
            (seeds[0].recurrence_date, 365),
            (seeds.last().unwrap().recurrence_date, -365),
        ] {
            let start = DateTime::from_timestamp_millis(now_ms).unwrap() + Duration::days(delta);
            stored.overrides.push(moved(
                &identity.to_string(),
                &start.to_rfc3339(),
                &(start + Duration::hours(1)).to_rfc3339(),
            ));
        }
        let template = Template::from_stored(stored).unwrap();
        let rows = template
            .identity_range(
                date("2026-01-01"),
                date("2035-12-31"),
                &mut ExpansionBudget::default(),
            )
            .unwrap()
            .0;
        let history = seeds[seeds.len() - 2].recurrence_date;
        for selected in [
            rows[0].recurrence_date,
            rows[1].recurrence_date,
            rows[3].recurrence_date,
        ] {
            let selected_started = rows
                .iter()
                .find(|row| row.recurrence_date == selected)
                .unwrap()
                .start_ms
                <= now_ms;
            for scope in [EditScope::This, EditScope::Following, EditScope::All] {
                let plan = template
                    .plan_delete(
                        selected,
                        scope,
                        ScopeEvidence {
                            history_dates: BTreeSet::from([history]),
                            ..ScopeEvidence::default()
                        },
                        ScopeClock {
                            epoch_ms: now_ms,
                            floating_today: None,
                        },
                        false,
                    )
                    .unwrap();
                let expected_live: Vec<_> = rows
                    .iter()
                    .filter(|row| match scope {
                        EditScope::This => row.recurrence_date != selected,
                        EditScope::Following if selected_started => {
                            row.recurrence_date < selected || row.start_ms > now_ms
                        }
                        EditScope::Following => row.recurrence_date < selected,
                        EditScope::All if selected_started => row.start_ms > now_ms,
                        EditScope::All => row.start_ms <= now_ms,
                    })
                    .map(|row| row.recurrence_date.to_string())
                    .collect();
                let expected_archived: Vec<_> = rows
                    .iter()
                    .filter(|row| {
                        let started = row.start_ms <= now_ms;
                        let protected = started || row.recurrence_date == history;
                        match scope {
                            EditScope::This => row.recurrence_date == selected && protected,
                            EditScope::Following if selected_started => {
                                row.recurrence_date >= selected && started
                            }
                            EditScope::Following => row.recurrence_date >= selected && protected,
                            EditScope::All if selected_started => started,
                            EditScope::All => !started && protected,
                        }
                    })
                    .map(|row| row.recurrence_date.to_string())
                    .collect();
                assert_eq!(
                    remaining(&template, &plan, "UTC"),
                    expected_live,
                    "{rule}: {scope:?}, {selected}"
                );
                assert_eq!(
                    archived(&plan),
                    expected_archived,
                    "{rule}: {scope:?}, {selected}"
                );
            }
        }
    }
}
