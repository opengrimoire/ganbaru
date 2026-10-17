use super::*;

fn date(value: &str) -> NaiveDate {
    parse_date(value).unwrap()
}

fn clock() -> ScopeClock {
    ScopeClock {
        epoch_ms: DateTime::parse_from_rfc3339("2026-05-10T09:30:00Z")
            .unwrap()
            .timestamp_millis(),
        floating_today: Some(date("2026-05-10")),
    }
}

fn stored() -> StoredTemplate<'static> {
    StoredTemplate {
        id: "series",
        start: "2026-05-01T09:00:00Z",
        end: "2026-05-01T10:00:00Z",
        home_zone: "UTC",
        all_day: false,
        rrule: Some("FREQ=DAILY;COUNT=30"),
        repeat_until: None,
        exceptions: &[],
        rdates: &[],
        overrides: Vec::new(),
    }
}

#[test]
fn edited_display_clocks_convert_natively_without_changing_the_home_zone() {
    let (result, _) = prepare(
        stored(),
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent {
            start_time: Some("2026-05-15T23:30:00".into()),
            end_time: Some("2026-05-16T00:30:00".into()),
            input_zone: Some("Asia/Tokyo".into()),
            ..TimingIntent::default()
        },
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert_eq!(result.timezone, "UTC");
    assert_eq!(result.start_time, "2026-05-15T14:30:00.000Z");
    assert_eq!(result.end_time, "2026-05-15T15:30:00.000Z");
}

#[test]
fn omitted_fold_endpoint_keeps_its_explicit_instant_when_only_displayed_end_changes() {
    let mut source = stored();
    source.start = "2026-11-01T01:30:00-05:00";
    source.end = "2026-11-01T02:30:00-05:00";
    source.home_zone = "America/New_York";
    let (result, _) = prepare(
        source,
        "2026-11-01",
        ScopeEvidence::default(),
        TimingIntent {
            end_time: Some("2026-11-01T17:00:00".into()),
            input_zone: Some("Asia/Tokyo".into()),
            ..TimingIntent::default()
        },
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert_eq!(result.start_time, "2026-11-01T06:30:00.000Z");
    assert_eq!(result.end_time, "2026-11-01T08:00:00.000Z");
    assert_eq!(result.timezone, "America/New_York");
}

fn prepare(
    source: StoredTemplate<'_>,
    selected: &str,
    evidence: ScopeEvidence,
    timing: TimingIntent,
    mut recurrence: RecurrenceIntent,
) -> Result<(ResolvedTiming, RecurrenceIntent), String> {
    let zone = source.home_zone;
    let template = Template::from_stored(source)?;
    let scope = template.plan_scope(date(selected), EditScope::This, evidence, clock())?;
    let result = template.prepare_timing(&scope, &timing, &mut recurrence, zone, clock(), false)?;
    Ok((result, recurrence))
}

#[test]
fn omitted_timing_uses_the_selected_occurrence_including_its_override() {
    let mut source = stored();
    source.overrides.push(StoredOverride {
        recurrence_id: "2026-05-15".into(),
        start: Some("2026-06-02T14:30:00Z".into()),
        end: Some("2026-06-02T15:00:00Z".into()),
        cancelled: false,
        this_and_future: false,
    });
    let (result, recurrence) = prepare(
        source,
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent::default(),
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert_eq!(result.start_time, "2026-06-02T14:30:00.000Z");
    assert_eq!(result.end_time, "2026-06-02T15:00:00.000Z");
    assert_eq!(recurrence, RecurrenceIntent::Unchanged);
}

#[test]
fn active_end_can_be_shortened_to_now_but_cannot_remove_elapsed_execution() {
    let evidence = || ScopeEvidence {
        active: Some(("run".into(), date("2026-05-10"))),
        ..ScopeEvidence::default()
    };
    let error = prepare(
        stored(),
        "2026-05-10",
        evidence(),
        TimingIntent {
            end_time: Some("2026-05-10T09:29:59Z".into()),
            ..TimingIntent::default()
        },
        RecurrenceIntent::Unchanged,
    )
    .err()
    .unwrap();
    assert!(error.contains("current native time"), "{error}");
    let (result, _) = prepare(
        stored(),
        "2026-05-10",
        evidence(),
        TimingIntent {
            end_time: Some("2026-05-10T09:30:00Z".into()),
            ..TimingIntent::default()
        },
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert_eq!(result.end_ms, clock().epoch_ms);
}

#[test]
fn equivalent_rule_is_unchanged_and_legacy_termination_participates_in_equality() {
    let (_, recurrence) = prepare(
        stored(),
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent::default(),
        RecurrenceIntent::Set("COUNT=30;INTERVAL=1;FREQ=DAILY".into()),
    )
    .unwrap();
    assert_eq!(recurrence, RecurrenceIntent::Unchanged);
    let mut source = stored();
    source.rrule = Some("FREQ=DAILY");
    source.repeat_until = Some("2026-05-30");
    let (_, recurrence) = prepare(
        source,
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent::default(),
        RecurrenceIntent::Set("FREQ=DAILY;UNTIL=20260530".into()),
    )
    .unwrap();
    assert_eq!(recurrence, RecurrenceIntent::Unchanged);
}

#[test]
fn clearing_and_replacing_a_rule_are_explicit_and_unsupported_input_is_rejected() {
    let (_, recurrence) = prepare(
        stored(),
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent::default(),
        RecurrenceIntent::Clear,
    )
    .unwrap();
    assert_eq!(recurrence, RecurrenceIntent::Clear);
    for rule in ["", "FREQ=HOURLY", "FREQ=DAILY;COUNT=0"] {
        assert!(
            prepare(
                stored(),
                "2026-05-15",
                ScopeEvidence::default(),
                TimingIntent::default(),
                RecurrenceIntent::Set(rule.into())
            )
            .is_err()
        );
    }
    let (_, recurrence) = prepare(
        stored(),
        "2026-05-15",
        ScopeEvidence::default(),
        TimingIntent::default(),
        RecurrenceIntent::Set("FREQ=WEEKLY;COUNT=3".into()),
    )
    .unwrap();
    let RecurrenceIntent::Set(value) = recurrence else {
        panic!("rule change disappeared");
    };
    assert!(value.contains("COUNT=3"));
}

#[test]
fn active_standalone_can_gain_a_rule_without_changing_its_start() {
    let mut source = stored();
    source.start = "2026-05-10T09:00:00Z";
    source.end = "2026-05-10T10:00:00Z";
    source.rrule = None;
    let evidence = ScopeEvidence {
        active: Some(("run".into(), date("2026-05-10"))),
        ..ScopeEvidence::default()
    };
    let (result, recurrence) = prepare(
        source,
        "2026-05-10",
        evidence,
        TimingIntent::default(),
        RecurrenceIntent::Set("FREQ=DAILY;COUNT=4".into()),
    )
    .unwrap();
    assert_eq!(result.start_time, "2026-05-10T09:00:00.000Z");
    assert!(matches!(recurrence, RecurrenceIntent::Set(_)));
}

#[test]
fn active_run_allows_equivalent_instant_but_forbids_recorded_start_and_rule_changes() {
    let evidence = || ScopeEvidence {
        active: Some(("run".into(), date("2026-05-10"))),
        ..ScopeEvidence::default()
    };
    let timing = TimingIntent {
        start_time: Some("2026-05-10T04:00:00-05:00".into()),
        end_time: Some("2026-05-10T11:00:00Z".into()),
        ..TimingIntent::default()
    };
    let (result, _) = prepare(
        stored(),
        "2026-05-10",
        evidence(),
        timing,
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert_eq!(result.start_time, "2026-05-10T09:00:00.000Z");
    assert_eq!(result.end_time, "2026-05-10T11:00:00.000Z");
    let timing = TimingIntent {
        start_time: Some("2026-05-10T09:01:00Z".into()),
        ..TimingIntent::default()
    };
    assert!(
        prepare(
            stored(),
            "2026-05-10",
            evidence(),
            timing,
            RecurrenceIntent::Unchanged
        )
        .err()
        .unwrap()
        .contains("recorded start")
    );
    assert!(
        prepare(
            stored(),
            "2026-05-10",
            evidence(),
            TimingIntent::default(),
            RecurrenceIntent::Clear
        )
        .err()
        .unwrap()
        .contains("recurrence chain")
    );
}

#[test]
fn closed_or_recorded_history_cannot_be_edited_even_after_a_clock_correction() {
    assert!(
        prepare(
            stored(),
            "2026-05-09",
            ScopeEvidence::default(),
            TimingIntent {
                end_time: Some("2026-05-09T11:00:00Z".into()),
                ..TimingIntent::default()
            },
            RecurrenceIntent::Unchanged
        )
        .err()
        .unwrap()
        .contains("Completed")
    );
    let evidence = ScopeEvidence {
        history_dates: BTreeSet::from([date("2026-05-15")]),
        ..ScopeEvidence::default()
    };
    assert!(
        prepare(
            stored(),
            "2026-05-15",
            evidence,
            TimingIntent {
                end_time: Some("2026-05-15T11:00:00Z".into()),
                ..TimingIntent::default()
            },
            RecurrenceIntent::Unchanged
        )
        .err()
        .unwrap()
        .contains("Recorded")
    );
}

#[test]
fn future_scope_can_prepare_edits_without_rewriting_the_selected_historical_occurrence() {
    let template = Template::from_stored(stored()).unwrap();
    let scope = template
        .plan_scope(
            date("2026-05-02"),
            EditScope::All,
            ScopeEvidence::default(),
            clock(),
        )
        .unwrap();
    assert!(scope.selected_started);
    let timing = TimingIntent {
        start_time: Some("2026-05-02T08:30:00Z".into()),
        ..TimingIntent::default()
    };
    assert!(
        template
            .prepare_timing(
                &scope,
                &timing,
                &mut RecurrenceIntent::Unchanged,
                "UTC",
                clock(),
                false
            )
            .is_ok()
    );
    assert_eq!(scope.first_mutable.unwrap().recurrence_date, "2026-05-11");
}

#[test]
fn opening_protected_occurrence_with_no_changes_is_allowed_but_metadata_edits_are_not() {
    let template = Template::from_stored(stored()).unwrap();
    let scope = template
        .plan_scope(
            date("2026-05-09"),
            EditScope::This,
            ScopeEvidence::default(),
            clock(),
        )
        .unwrap();
    assert!(
        template
            .prepare_timing(
                &scope,
                &TimingIntent::default(),
                &mut RecurrenceIntent::Unchanged,
                "UTC",
                clock(),
                false
            )
            .is_ok()
    );
    assert!(
        template
            .prepare_timing(
                &scope,
                &TimingIntent::default(),
                &mut RecurrenceIntent::Unchanged,
                "UTC",
                clock(),
                true
            )
            .err()
            .unwrap()
            .contains("Completed")
    );
}

#[test]
fn date_kind_conversion_requires_both_endpoints_and_strict_floating_dates() {
    let timing = TimingIntent {
        all_day: Some(true),
        ..TimingIntent::default()
    };
    assert!(
        prepare(
            stored(),
            "2026-05-15",
            ScopeEvidence::default(),
            timing,
            RecurrenceIntent::Unchanged
        )
        .is_err()
    );
    for start in [
        "2026-05-15T00:00:00Z",
        "2026-5-15",
        "2026-02-30",
        "0000-05-15",
    ] {
        let timing = TimingIntent {
            all_day: Some(true),
            start_time: Some(start.into()),
            end_time: Some("2026-05-16".into()),
            ..TimingIntent::default()
        };
        assert!(
            prepare(
                stored(),
                "2026-05-15",
                ScopeEvidence::default(),
                timing,
                RecurrenceIntent::Unchanged
            )
            .is_err(),
            "{start}"
        );
    }
    let timing = TimingIntent {
        all_day: Some(true),
        start_time: Some("2026-05-15".into()),
        end_time: Some("2026-05-15".into()),
        ..TimingIntent::default()
    };
    let (result, _) = prepare(
        stored(),
        "2026-05-15",
        ScopeEvidence::default(),
        timing,
        RecurrenceIntent::Unchanged,
    )
    .unwrap();
    assert!(result.all_day);
    assert_eq!(result.start_time, "2026-05-15");
}

#[test]
fn gap_input_retains_civil_intent_and_explicit_later_fold_keeps_its_instant() {
    let cases = [
        (
            "2026-03-08T02:30:00",
            "2026-03-08T04:00:00",
            "2026-03-08T02:30:00",
            "2026-03-08T08:00:00.000Z",
        ),
        (
            "2026-11-01T01:30:00-05:00",
            "2026-11-01T02:00:00-05:00",
            "2026-11-01T06:30:00.000Z",
            "2026-11-01T07:00:00.000Z",
        ),
    ];
    for (start, end, expected_start, expected_end) in cases {
        let timing = TimingIntent {
            start_time: Some(start.into()),
            end_time: Some(end.into()),
            timezone: Some("America/New_York".into()),
            ..TimingIntent::default()
        };
        let (result, _) = prepare(
            stored(),
            "2026-05-15",
            ScopeEvidence::default(),
            timing,
            RecurrenceIntent::Unchanged,
        )
        .unwrap();
        assert_eq!(result.start_time, expected_start);
        assert_eq!(result.end_time, expected_end);
    }
}

#[test]
fn invalid_zone_and_nonpositive_intervals_fail_before_planning_writes() {
    for timing in [
        TimingIntent {
            timezone: Some("Not/AZone".into()),
            ..TimingIntent::default()
        },
        TimingIntent {
            end_time: Some("2026-05-15T09:00:00Z".into()),
            ..TimingIntent::default()
        },
        TimingIntent {
            end_time: Some("invalid".into()),
            ..TimingIntent::default()
        },
    ] {
        assert!(
            prepare(
                stored(),
                "2026-05-15",
                ScopeEvidence::default(),
                timing,
                RecurrenceIntent::Unchanged
            )
            .is_err()
        );
    }
}
