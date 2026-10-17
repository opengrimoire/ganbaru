use super::*;

fn date(value: &str) -> NaiveDate {
    parse_date(value).unwrap()
}

fn clock(value: &str) -> ScopeClock {
    ScopeClock {
        epoch_ms: DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis(),
        floating_today: Some(date(&value[..10])),
    }
}

fn source() -> StoredTemplate<'static> {
    StoredTemplate {
        id: "series",
        start: "2024-01-01T09:00:00Z",
        end: "2024-01-01T10:00:00Z",
        home_zone: "UTC",
        all_day: false,
        rrule: Some("FREQ=DAILY;COUNT=5"),
        repeat_until: None,
        exceptions: &[],
        rdates: &[],
        overrides: Vec::new(),
    }
}

struct Input {
    timing: TimingIntent,
    recurrence: RecurrenceIntent,
    evidence: ScopeEvidence,
    clock: ScopeClock,
    metadata_changed: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            timing: TimingIntent::default(),
            recurrence: RecurrenceIntent::Unchanged,
            evidence: ScopeEvidence::default(),
            clock: clock("2023-01-01T00:00:00Z"),
            metadata_changed: true,
        }
    }
}

fn plan(
    stored: StoredTemplate<'_>,
    selected: &str,
    scope: EditScope,
    mut input: Input,
) -> EditGeometryPlan {
    let source_zone = stored.home_zone;
    Template::from_stored(stored)
        .unwrap()
        .prepare_edit_geometry(
            date(selected),
            scope,
            input.evidence,
            input.clock,
            GeometryDraft {
                timing: &input.timing,
                recurrence: &mut input.recurrence,
                source_zone,
                metadata_changed: input.metadata_changed,
            },
        )
        .unwrap()
        .2
}

fn expand(series: &PlannedSeries) -> Vec<Occurrence> {
    expand_templates(
        &[series.template("result").unwrap()],
        &Window::new("2023-01-01", "2025-12-31", &TimeZone::UTC).unwrap(),
    )
    .unwrap()
    .pop()
    .unwrap()
}

fn dates(series: &PlannedSeries) -> Vec<String> {
    expand(series)
        .into_iter()
        .map(|row| row.recurrence_date.to_string())
        .collect()
}

#[test]
fn this_detaches_one_occurrence_and_a_new_rule_is_an_independent_series() {
    let result = plan(source(), "2024-01-03", EditScope::This, Input::default());
    assert_eq!(result.kind, EditKind::Detach);
    assert_eq!(
        dates(result.before.as_ref().unwrap()),
        ["2024-01-01", "2024-01-02", "2024-01-04", "2024-01-05"]
    );
    let edited = result.edited.as_ref().unwrap();
    assert_eq!(dates(edited), ["2024-01-03"]);
    assert_eq!(
        edited.metadata_occurrence_date.as_deref(),
        Some("2024-01-03")
    );
    let result = plan(
        source(),
        "2024-01-03",
        EditScope::This,
        Input {
            recurrence: RecurrenceIntent::Set("FREQ=DAILY;COUNT=2".into()),
            ..Input::default()
        },
    );
    assert_eq!(
        dates(result.edited.as_ref().unwrap()),
        ["2024-01-03", "2024-01-04"]
    );
}

#[test]
fn following_retains_count_consumption_and_future_exclusions() {
    let exclusions = vec!["2024-01-04".into()];
    let mut stored = source();
    stored.exceptions = &exclusions;
    let result = plan(stored, "2024-01-03", EditScope::Following, Input::default());
    assert_eq!(result.kind, EditKind::Split);
    assert_eq!(
        dates(result.before.as_ref().unwrap()),
        ["2024-01-01", "2024-01-02"]
    );
    assert_eq!(
        dates(result.edited.as_ref().unwrap()),
        ["2024-01-03", "2024-01-05"]
    );
    assert_eq!(
        rule::parse(result.edited.unwrap().fields.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(3)
    );
}

#[test]
fn future_only_all_updates_in_place_and_collapse_uses_the_selected_survivor() {
    let result = plan(source(), "2024-01-04", EditScope::All, Input::default());
    assert_eq!(result.kind, EditKind::Update);
    assert!(result.before.is_none());
    assert_eq!(dates(result.edited.as_ref().unwrap()).len(), 5);
    assert_eq!(
        result.edited.unwrap().fields.start_time,
        "2024-01-01T09:00:00.000Z"
    );
    let result = plan(
        source(),
        "2024-01-04",
        EditScope::All,
        Input {
            recurrence: RecurrenceIntent::Clear,
            ..Input::default()
        },
    );
    assert_eq!(result.kind, EditKind::Update);
    assert_eq!(dates(result.edited.as_ref().unwrap()), ["2024-01-04"]);
}

#[test]
fn protected_prefix_and_isolated_history_are_preserved_with_an_exact_active_target() {
    let result = plan(
        source(),
        "2024-01-03",
        EditScope::All,
        Input {
            clock: clock("2024-01-02T12:00:00Z"),
            evidence: ScopeEvidence {
                history_dates: BTreeSet::from([date("2024-01-04")]),
                active: Some(("run-5".into(), date("2024-01-05"))),
            },
            ..Input::default()
        },
    );
    assert_eq!(
        dates(result.before.as_ref().unwrap()),
        ["2024-01-01", "2024-01-02"]
    );
    assert_eq!(dates(result.edited.as_ref().unwrap()), ["2024-01-03"]);
    assert_eq!(
        result
            .materialize
            .iter()
            .map(|row| row.recurrence_date.as_str())
            .collect::<Vec<_>>(),
        ["2024-01-04", "2024-01-05"]
    );
    let transfer = result.active_transfer.unwrap();
    assert_eq!(transfer.run_id, "run-5");
    assert_eq!(transfer.original_recurrence_date, "2024-01-05");
    assert_eq!(
        serde_json::to_value(&transfer).unwrap()["target"]["recurrenceDate"],
        "2024-01-05"
    );
    assert!(
        matches!(transfer.target, ActiveTarget::Preserved { recurrence_date } if recurrence_date == "2024-01-05")
    );
}

#[test]
fn active_selected_series_detaches_but_an_active_standalone_retains_its_identity() {
    let input = || Input {
        clock: clock("2024-01-03T09:30:00Z"),
        evidence: ScopeEvidence {
            active: Some(("run".into(), date("2024-01-03"))),
            ..ScopeEvidence::default()
        },
        ..Input::default()
    };
    let result = plan(source(), "2024-01-03", EditScope::All, input());
    assert_eq!(result.kind, EditKind::Detach);
    assert!(matches!(
        result.active_transfer.unwrap().target,
        ActiveTarget::Edited
    ));
    let mut stored = source();
    stored.start = "2024-01-03T09:00:00Z";
    stored.end = "2024-01-03T10:00:00Z";
    stored.rrule = None;
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::All,
        Input {
            recurrence: RecurrenceIntent::Set("FREQ=DAILY;COUNT=2".into()),
            ..input()
        },
    );
    assert_eq!(result.kind, EditKind::Update);
    assert!(result.active_transfer.is_none());
    assert_eq!(
        dates(result.edited.as_ref().unwrap()),
        ["2024-01-03", "2024-01-04"]
    );
}

#[test]
fn a_protected_noop_produces_no_mutation_or_transfer_targets() {
    let result = plan(
        source(),
        "2024-01-03",
        EditScope::This,
        Input {
            metadata_changed: false,
            clock: clock("2024-01-10T00:00:00Z"),
            ..Input::default()
        },
    );
    assert_eq!(result.kind, EditKind::Unchanged);
    assert!(
        result.before.is_none()
            && result.edited.is_none()
            && result.materialize.is_empty()
            && result.active_transfer.is_none()
    );
}

#[test]
fn following_collapse_keeps_one_survivor_and_all_collapse_preserves_older_history() {
    let result = plan(
        source(),
        "2024-01-03",
        EditScope::Following,
        Input {
            recurrence: RecurrenceIntent::Clear,
            ..Input::default()
        },
    );
    assert_eq!(
        dates(result.before.as_ref().unwrap()),
        ["2024-01-01", "2024-01-02"]
    );
    assert_eq!(dates(result.edited.as_ref().unwrap()), ["2024-01-03"]);
    let result = plan(
        source(),
        "2024-01-04",
        EditScope::All,
        Input {
            recurrence: RecurrenceIntent::Clear,
            clock: clock("2024-01-02T12:00:00Z"),
            ..Input::default()
        },
    );
    assert_eq!(
        dates(result.before.as_ref().unwrap()),
        ["2024-01-01", "2024-01-02"]
    );
    assert_eq!(dates(result.edited.as_ref().unwrap()), ["2024-01-04"]);
}

#[test]
fn unchanged_advanced_sets_round_trip_through_the_concrete_plan() {
    for (rule, selected, additions) in [
        (
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO;COUNT=5",
            "2024-01-02",
            vec!["2024-01-02".into(), "2024-03-04".into()],
        ),
        (
            "FREQ=YEARLY;BYWEEKNO=1;BYDAY=MO;COUNT=3",
            "2024-12-30",
            vec![],
        ),
        (
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=5",
            "2024-02-29",
            vec![],
        ),
    ] {
        let mut stored = source();
        stored.rrule = Some(rule);
        stored.rdates = &additions;
        let template = Template::from_stored(stored).unwrap();
        let (_, _, result) = template
            .prepare_edit_geometry(
                date(selected),
                EditScope::Following,
                ScopeEvidence::default(),
                clock("2023-01-01T00:00:00Z"),
                GeometryDraft {
                    timing: &TimingIntent::default(),
                    recurrence: &mut RecurrenceIntent::Unchanged,
                    source_zone: "UTC",
                    metadata_changed: true,
                },
            )
            .unwrap();
        let window = Window::new("2023-01-01", "2025-12-31", &TimeZone::UTC).unwrap();
        let expected = expand_templates(&[template], &window)
            .unwrap()
            .pop()
            .unwrap();
        let mut actual = expand(result.before.as_ref().unwrap());
        actual.extend(expand(result.edited.as_ref().unwrap()));
        let geometry = |rows: Vec<Occurrence>| {
            let mut values = rows
                .into_iter()
                .map(|row| (row.recurrence_date, row.start_ms, row.end_ms))
                .collect::<Vec<_>>();
            values.sort();
            values
        };
        assert_eq!(geometry(actual), geometry(expected), "{rule}");
    }
}

#[test]
fn changing_only_the_end_preserves_the_full_multiday_span() {
    let mut stored = source();
    stored.start = "2024-01-01T23:00:00Z";
    stored.end = "2024-01-03T01:00:00Z";
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::All,
        Input {
            timing: TimingIntent {
                end_time: Some("2024-01-05T02:00:00Z".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    let edited = result.edited.unwrap();
    assert_eq!(edited.fields.start_time, "2024-01-01T23:00:00.000Z");
    assert_eq!(edited.fields.end_time, "2024-01-03T02:00:00.000Z");
    assert!(
        expand(&edited)
            .iter()
            .all(|row| row.end_ms - row.start_ms == 27 * 60 * 60 * 1000)
    );
}

#[test]
fn explicit_later_fold_is_retained_as_the_following_anchor() {
    let mut stored = source();
    stored.start = "2024-11-01T01:30:00-04:00";
    stored.end = "2024-11-01T02:30:00-04:00";
    stored.home_zone = "America/New_York";
    let result = plan(
        stored,
        "2024-11-03",
        EditScope::Following,
        Input {
            timing: TimingIntent {
                start_time: Some("2024-11-03T01:30:00-05:00".into()),
                end_time: Some("2024-11-03T02:30:00-05:00".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    let edited = result.edited.unwrap();
    assert_eq!(edited.fields.start_time, "2024-11-03T06:30:00.000Z");
    assert_eq!(
        instant(expand(&edited)[0].start_ms).unwrap(),
        "2024-11-03T06:30:00.000Z"
    );
}

#[test]
fn floating_conversion_preserves_independent_dates_and_override_civil_dates() {
    let additions = vec!["2024-01-10".into()];
    let mut stored = source();
    stored.rdates = &additions;
    stored.overrides.push(StoredOverride {
        recurrence_id: "2024-01-04".into(),
        start: Some("2024-01-06T15:00:00Z".into()),
        end: Some("2024-01-06T16:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::All,
        Input {
            timing: TimingIntent {
                start_time: Some("2024-01-03".into()),
                end_time: Some("2024-01-03".into()),
                all_day: Some(true),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    let edited = result.edited.unwrap();
    assert!(edited.all_day);
    assert_eq!(edited.fields.rdates, ["2024-01-10"]);
    assert_eq!(
        edited.overrides[0].start_time.as_deref(),
        Some("2024-01-06")
    );
    assert_eq!(edited.overrides[0].source_recurrence_id, "2024-01-04");
    assert_eq!(
        expand(&edited)
            .iter()
            .find(|row| row.recurrence_date == date("2024-01-04"))
            .unwrap()
            .start_date,
        date("2024-01-06")
    );
}

#[test]
fn changing_the_rule_preserves_exclusions_and_copies_complete_override_references() {
    let exclusions = vec!["2024-01-04".into()];
    let mut stored = source();
    stored.exceptions = &exclusions;
    stored.overrides.push(StoredOverride {
        recurrence_id: "2024-01-05T09:00:00Z".into(),
        start: Some("2024-01-06T14:00:00Z".into()),
        end: Some("2024-01-06T15:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::Following,
        Input {
            recurrence: RecurrenceIntent::Set("FREQ=DAILY;COUNT=3".into()),
            ..Input::default()
        },
    );
    let edited = result.edited.unwrap();
    assert_eq!(dates(&edited), ["2024-01-03", "2024-01-05"]);
    assert_eq!(
        edited.overrides[0].source_recurrence_id,
        "2024-01-05T09:00:00Z"
    );
    assert_eq!(edited.overrides[0].recurrence_id, "2024-01-05");
}

#[test]
fn moving_an_off_pattern_addition_reanchors_without_restarting_or_losing_count() {
    let additions = vec!["2024-01-02".into(), "2024-03-04".into()];
    let mut stored = source();
    stored.rrule = Some("FREQ=WEEKLY;BYDAY=MO;COUNT=3");
    stored.rdates = &additions;
    let result = plan(
        stored,
        "2024-01-02",
        EditScope::Following,
        Input {
            timing: TimingIntent {
                start_time: Some("2024-01-03T09:00:00Z".into()),
                end_time: Some("2024-01-03T10:00:00Z".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    assert_eq!(dates(result.before.as_ref().unwrap()), ["2024-01-01"]);
    let edited = result.edited.unwrap();
    assert_eq!(
        dates(&edited),
        ["2024-01-03", "2024-01-08", "2024-01-15", "2024-03-04"]
    );
    assert_eq!(
        rule::parse(edited.fields.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(3)
    );
}

#[test]
fn moving_an_addition_only_series_replaces_the_selected_date_and_keeps_later_additions() {
    let additions = vec!["2024-01-03".into(), "2024-01-05".into()];
    let mut stored = source();
    stored.rrule = None;
    stored.rdates = &additions;
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::Following,
        Input {
            timing: TimingIntent {
                start_time: Some("2024-01-04T09:00:00Z".into()),
                end_time: Some("2024-01-04T10:00:00Z".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    assert_eq!(dates(result.before.as_ref().unwrap()), ["2024-01-01"]);
    assert_eq!(
        dates(result.edited.as_ref().unwrap()),
        ["2024-01-04", "2024-01-05"]
    );
}

#[test]
fn repeating_the_home_zone_does_not_erase_an_explicit_later_fold_addition() {
    let additions = vec!["2024-11-03T01:30:00-05:00".into()];
    let mut stored = source();
    stored.start = "2024-10-27T01:30:00-04:00";
    stored.end = "2024-10-27T02:30:00-04:00";
    stored.home_zone = "America/New_York";
    stored.rrule = Some("FREQ=DAILY;COUNT=2");
    stored.rdates = &additions;
    let result = plan(
        stored,
        "2024-11-03",
        EditScope::Following,
        Input {
            timing: TimingIntent {
                timezone: Some("America/New_York".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    let rows = expand(result.edited.as_ref().unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(
        instant(rows[0].start_ms).unwrap(),
        "2024-11-03T06:30:00.000Z"
    );
}

#[test]
fn a_reanchored_moved_override_keeps_its_metadata_reference_and_requested_geometry() {
    let mut stored = source();
    stored.overrides.push(StoredOverride {
        recurrence_id: "2024-01-03".into(),
        start: Some("2024-01-10T14:00:00Z".into()),
        end: Some("2024-01-10T16:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let result = plan(
        stored,
        "2024-01-03",
        EditScope::Following,
        Input {
            timing: TimingIntent {
                start_time: Some("2024-01-11T15:00:00Z".into()),
                end_time: Some("2024-01-11T17:00:00Z".into()),
                ..TimingIntent::default()
            },
            ..Input::default()
        },
    );
    let edited = result.edited.unwrap();
    assert_eq!(edited.overrides[0].source_recurrence_id, "2024-01-03");
    assert_eq!(edited.overrides[0].recurrence_id, "2024-01-11");
    let rows = expand(&edited);
    assert_eq!(
        instant(rows[0].start_ms).unwrap(),
        "2024-01-11T15:00:00.000Z"
    );
    assert_eq!(instant(rows[0].end_ms).unwrap(), "2024-01-11T17:00:00.000Z");
    assert_eq!(rows.len(), 3);
}

#[test]
fn floating_conversion_keeps_the_last_admitted_home_date_of_a_timed_until() {
    for (rule, last) in [
        ("FREQ=DAILY;UNTIL=20240105T010000Z", "2024-01-04"),
        ("FREQ=DAILY;UNTIL=20240104T120000Z", "2024-01-03"),
        ("FREQ=DAILY;UNTIL=20240104T080000", "2024-01-03"),
    ] {
        let mut stored = source();
        stored.home_zone = "America/New_York";
        stored.start = "2024-01-01T09:00:00-05:00";
        stored.end = "2024-01-01T10:00:00-05:00";
        stored.rrule = Some(rule);
        let result = plan(
            stored,
            "2024-01-02",
            EditScope::All,
            Input {
                timing: TimingIntent {
                    start_time: Some("2024-01-02".into()),
                    end_time: Some("2024-01-02".into()),
                    all_day: Some(true),
                    ..TimingIntent::default()
                },
                ..Input::default()
            },
        );
        let actual = dates(result.edited.as_ref().unwrap());
        assert_eq!(actual.last().unwrap(), last, "{rule}");
    }
}

#[test]
fn new_rules_cannot_activate_malformed_override_geometry() {
    let mut stored = source();
    stored.rrule = Some("FREQ=WEEKLY;BYDAY=MO;COUNT=3");
    stored.overrides.push(StoredOverride {
        recurrence_id: "2024-01-02".into(),
        start: Some("invalid time".into()),
        end: None,
        cancelled: false,
        this_and_future: false,
    });
    let template = Template::from_stored(stored).unwrap();
    let error = template
        .prepare_edit_geometry(
            date("2024-01-01"),
            EditScope::All,
            ScopeEvidence::default(),
            clock("2023-01-01T00:00:00Z"),
            GeometryDraft {
                timing: &TimingIntent::default(),
                recurrence: &mut RecurrenceIntent::Set("FREQ=DAILY;COUNT=3".into()),
                source_zone: "UTC",
                metadata_changed: false,
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("invalid Calendar civil datetime"), "{error}");
}

#[test]
fn collapse_requires_a_mutable_survivor_and_exhausted_history_cannot_create_new_work() {
    for (recurrence, expected) in [
        (RecurrenceIntent::Clear, "mutable Calendar occurrence"),
        (RecurrenceIntent::Unchanged, "no mutable occurrences"),
    ] {
        let template = Template::from_stored(source()).unwrap();
        let error = template
            .prepare_edit_geometry(
                date("2024-01-03"),
                EditScope::All,
                ScopeEvidence::default(),
                clock("2024-01-10T00:00:00Z"),
                GeometryDraft {
                    timing: &TimingIntent::default(),
                    recurrence: &mut { recurrence },
                    source_zone: "UTC",
                    metadata_changed: true,
                },
            )
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn requesting_a_series_scope_cannot_bypass_standalone_history_protection() {
    for scope in [EditScope::Following, EditScope::All] {
        let mut stored = source();
        stored.rrule = None;
        let template = Template::from_stored(stored).unwrap();
        let error = template
            .prepare_edit_geometry(
                date("2024-01-01"),
                scope,
                ScopeEvidence {
                    history_dates: BTreeSet::from([date("2024-01-01")]),
                    ..ScopeEvidence::default()
                },
                clock("2023-01-01T00:00:00Z"),
                GeometryDraft {
                    timing: &TimingIntent::default(),
                    recurrence: &mut RecurrenceIntent::Unchanged,
                    source_zone: "UTC",
                    metadata_changed: true,
                },
            )
            .err()
            .unwrap();
        assert!(error.contains("Recorded Calendar history"), "{error}");
        let result = template
            .prepare_edit_geometry(
                date("2024-01-01"),
                scope,
                ScopeEvidence::default(),
                clock("2023-01-01T00:00:00Z"),
                GeometryDraft {
                    timing: &TimingIntent::default(),
                    recurrence: &mut RecurrenceIntent::Unchanged,
                    source_zone: "UTC",
                    metadata_changed: true,
                },
            )
            .unwrap();
        assert_eq!(result.0.effective_scope, EditScope::This);
        assert_eq!(result.2.kind, EditKind::Update);
    }
}

#[test]
fn floating_until_conversion_compares_instants_across_folds_and_skips_gap_candidates() {
    for (start, end, selected, rule, last) in [
        (
            "2024-11-01T01:30:00-04:00",
            "2024-11-01T02:30:00-04:00",
            "2024-11-02",
            "FREQ=DAILY;UNTIL=20241103T061500Z",
            "2024-11-03",
        ),
        (
            "2024-11-01T01:30:00-04:00",
            "2024-11-01T02:30:00-04:00",
            "2024-11-02",
            "FREQ=DAILY;UNTIL=20241103T011500",
            "2024-11-02",
        ),
        (
            "2024-03-08T02:30:00-05:00",
            "2024-03-08T03:30:00-05:00",
            "2024-03-09",
            "FREQ=DAILY;UNTIL=20240310T090000Z",
            "2024-03-09",
        ),
    ] {
        let mut stored = source();
        stored.start = start;
        stored.end = end;
        stored.home_zone = "America/New_York";
        stored.rrule = Some(rule);
        let result = plan(
            stored,
            selected,
            EditScope::All,
            Input {
                timing: TimingIntent {
                    start_time: Some(selected.into()),
                    end_time: Some(selected.into()),
                    all_day: Some(true),
                    ..TimingIntent::default()
                },
                ..Input::default()
            },
        );
        assert_eq!(
            dates(result.edited.as_ref().unwrap()).last().unwrap(),
            last,
            "{rule}"
        );
    }
}
