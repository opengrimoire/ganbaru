use super::*;

fn source(rule: Option<&str>) -> StoredTemplate<'_> {
    StoredTemplate {
        id: "source",
        start: "2024-01-01T09:00:00Z",
        end: "2024-01-01T10:00:00Z",
        home_zone: "UTC",
        all_day: false,
        rrule: rule,
        repeat_until: None,
        exceptions: &[],
        rdates: &[],
        overrides: Vec::new(),
    }
}

fn restore_side(side: &PartitionSide, source: &Template, zone: &str) -> Template {
    Template::from_stored(StoredTemplate {
        id: "partition",
        start: &side.start_time,
        end: &side.end_time,
        home_zone: zone,
        all_day: source.all_day,
        rrule: side.rrule.as_deref(),
        repeat_until: None,
        exceptions: &side.exceptions,
        rdates: &side.rdates,
        overrides: source
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
    .unwrap()
}

fn geometry(values: Vec<Occurrence>) -> Vec<(NaiveDate, i64, i64)> {
    values
        .into_iter()
        .map(|value| (value.recurrence_date, value.start_ms, value.end_ms))
        .collect()
}

/// Reload serialized sides through the same canonical path used by persisted windows.
fn assert_partition(input: StoredTemplate<'_>, selected: &str) -> SeriesPartition {
    let zone = input.home_zone;
    let source = Template::from_stored(input).unwrap();
    let selected = parse_date(selected).unwrap();
    let plan = source
        .partition_at_with_budget(selected, &mut ExpansionBudget::default())
        .unwrap();
    let before = restore_side(&plan.before, &source, zone);
    let following = restore_side(&plan.following, &source, zone);
    let window = Window::new("2023-01-01", "2032-12-31", &TimeZone::UTC).unwrap();
    let mut expanded = expand_templates(&[source, before, following], &window).unwrap();
    let following = expanded.pop().unwrap();
    let before = expanded.pop().unwrap();
    let expected = geometry(expanded.pop().unwrap());
    assert!(before.iter().all(|value| value.recurrence_date < selected));
    assert!(
        following
            .iter()
            .all(|value| value.recurrence_date >= selected)
    );
    let mut actual = geometry(before);
    actual.extend(geometry(following));
    actual.sort();
    assert_eq!(actual, expected, "partition at {selected}");
    plan
}

#[test]
fn count_is_consumed_before_exclusions_without_restarting_the_series() {
    let exceptions = vec!["2024-01-02".into()];
    let mut input = source(Some("FREQ=DAILY;COUNT=5"));
    input.exceptions = &exceptions;
    let plan = assert_partition(input, "2024-01-04");
    assert_eq!(
        rule::parse(plan.before.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(3)
    );
    assert_eq!(
        rule::parse(plan.following.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(2)
    );
    assert_eq!(plan.following.start_time, "2024-01-04T09:00:00.000Z");
}

#[test]
fn native_partitions_preserve_advanced_period_selection_and_week_starts() {
    for (rule, boundary) in [
        (
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,SU;WKST=SU;COUNT=9",
            "2024-01-14",
        ),
        (
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=8",
            "2024-02-29",
        ),
        (
            "FREQ=MONTHLY;BYMONTHDAY=1,2,3,4,5,6,7;BYDAY=MO;COUNT=8",
            "2024-03-04",
        ),
        ("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=29;COUNT=3", "2024-02-29"),
        ("FREQ=YEARLY;BYYEARDAY=-1;COUNT=5", "2025-12-31"),
        ("FREQ=YEARLY;BYWEEKNO=1;BYDAY=MO;COUNT=6", "2024-12-30"),
    ] {
        assert_partition(source(Some(rule)), boundary);
    }
}

#[test]
fn until_and_legacy_repeat_until_remain_finite_on_the_following_side() {
    for rule in [
        "FREQ=DAILY;UNTIL=20240105",
        "FREQ=DAILY;UNTIL=20240105T090000",
        "FREQ=DAILY;UNTIL=20240105T090000Z",
    ] {
        let original = rule::parse(rule).unwrap();
        let plan = assert_partition(source(Some(rule)), "2024-01-03");
        assert_eq!(
            rule::parse(plan.following.rrule.as_deref().unwrap())
                .unwrap()
                .until,
            original.until
        );
    }
    let mut input = source(Some("FREQ=DAILY"));
    input.repeat_until = Some("2024-01-05");
    let plan = assert_partition(input, "2024-01-03");
    assert_eq!(
        rule::parse(plan.following.rrule.as_deref().unwrap())
            .unwrap()
            .until,
        Some(rule::Until::Date(parse_date("2024-01-05").unwrap()))
    );
}

#[test]
fn an_off_cadence_rdate_split_retains_the_original_anchor_and_suppresses_its_prefix() {
    let rdates = vec!["2024-01-05".into(), "2024-02-05".into()];
    let mut input = source(Some("FREQ=WEEKLY;INTERVAL=2;COUNT=4"));
    input.rdates = &rdates;
    let plan = assert_partition(input, "2024-01-05");
    assert_eq!(plan.following.start_time, "2024-01-01T09:00:00.000Z");
    assert!(plan.following.exceptions.contains(&"2024-01-01".into()));
    assert_eq!(
        rule::parse(plan.following.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(4)
    );
}

#[test]
fn a_split_at_dtstart_leaves_no_accidental_anchor_on_the_old_side() {
    let rdates = vec!["2023-12-15".into()];
    let mut input = source(Some("FREQ=DAILY;COUNT=3"));
    input.rdates = &rdates;
    let plan = assert_partition(input, "2024-01-01");
    assert!(plan.before.rrule.is_none());
    assert_eq!(plan.before.exceptions, ["2024-01-01"]);
    assert_eq!(plan.before.rdates, ["2023-12-15"]);
}

#[test]
fn cancelled_and_moved_overrides_do_not_reappear_after_anchor_preservation() {
    let mut input = source(Some("FREQ=DAILY;INTERVAL=2;COUNT=6"));
    // Split at a true off-cadence date, forcing the original-anchor representation.
    let rdates = vec!["2024-01-04".into()];
    input.rdates = &rdates;
    input.overrides = vec![
        StoredOverride {
            recurrence_id: "2024-01-03".into(),
            start: None,
            end: None,
            cancelled: true,
            this_and_future: false,
        },
        StoredOverride {
            recurrence_id: "2024-01-07".into(),
            start: Some("2024-01-02T12:00:00Z".into()),
            end: Some("2024-01-02T13:00:00Z".into()),
            cancelled: false,
            this_and_future: false,
        },
        StoredOverride {
            recurrence_id: "2024-01-09".into(),
            start: None,
            end: None,
            cancelled: true,
            this_and_future: true,
        },
    ];
    let plan = assert_partition(input, "2024-01-04");
    assert!(plan.following.exceptions.contains(&"2024-01-03".into()));
    assert_eq!(
        plan.following.override_recurrence_ids,
        ["2024-01-07", "2024-01-09"]
    );
}

#[test]
fn dst_gaps_and_fold_durations_survive_partition_round_trips() {
    let mut gap = source(Some("FREQ=DAILY;COUNT=4"));
    gap.home_zone = "America/New_York";
    gap.start = "2024-03-09T07:30:00Z";
    gap.end = "2024-03-09T08:30:00Z";
    let plan = assert_partition(gap, "2024-03-11");
    assert_eq!(
        rule::parse(plan.following.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(3)
    );
    let mut fold = source(Some("FREQ=DAILY;COUNT=370"));
    fold.home_zone = "America/New_York";
    fold.start = "2024-11-03T05:45:00Z";
    fold.end = "2024-11-03T06:15:00Z";
    let plan = assert_partition(fold, "2024-11-04");
    assert_eq!(plan.following.start_time, "2024-11-03T05:45:00.000Z");
}

#[test]
fn explicit_gap_and_later_fold_rdates_keep_their_original_civil_identity() {
    let mut gap = source(Some("FREQ=DAILY;COUNT=1"));
    gap.home_zone = "America/New_York";
    gap.start = "2024-03-09T07:30:00Z";
    gap.end = "2024-03-09T08:30:00Z";
    let rdates = vec!["2024-03-10".into(), "2024-03-11".into()];
    gap.rdates = &rdates;
    let plan = assert_partition(gap, "2024-03-10");
    assert_eq!(plan.following.rdates, ["2024-03-10", "2024-03-11"]);
    let mut fold = source(Some("FREQ=DAILY;COUNT=1"));
    fold.home_zone = "America/New_York";
    fold.start = "2024-11-02T05:30:00Z";
    fold.end = "2024-11-02T06:30:00Z";
    let rdates = vec!["2024-11-03T06:30:00Z".into()];
    fold.rdates = &rdates;
    let plan = assert_partition(fold, "2024-11-03");
    assert_eq!(plan.following.rdates, ["2024-11-03T06:30:00.000Z"]);
}

#[test]
fn split_end_in_a_gap_does_not_change_later_occurrence_durations() {
    let mut input = source(Some("FREQ=DAILY;COUNT=370"));
    input.home_zone = "America/New_York";
    input.start = "2024-03-09T06:30:00Z";
    input.end = "2024-03-09T07:30:00Z";
    let plan = assert_partition(input, "2024-03-10");
    assert_eq!(plan.following.start_time, "2024-03-09T06:30:00.000Z");
}

#[test]
fn legacy_wall_clock_gap_anchors_keep_their_recurrence_time() {
    for boundary in ["2024-03-10", "2024-03-11"] {
        let mut input = source(Some("FREQ=DAILY;COUNT=4"));
        input.home_zone = "America/New_York";
        input.start = "2024-03-10 02:30";
        input.end = "2024-03-10 04:30";
        assert_partition(input, boundary);
    }
}

#[test]
fn floating_dates_rdate_only_sets_and_fractional_timed_anchors_are_preserved() {
    let mut all_day = source(Some("FREQ=DAILY;COUNT=4"));
    all_day.all_day = true;
    all_day.start = "2024-01-01";
    all_day.end = "2024-01-02";
    assert_partition(all_day, "2024-01-03");
    let rdates = vec!["2024-01-05".into(), "2024-01-09".into()];
    let mut dates_only = source(None);
    dates_only.rdates = &rdates;
    assert_partition(dates_only, "2024-01-05");
    let mut fractional = source(Some("FREQ=DAILY;COUNT=4"));
    fractional.start = "2024-01-01T23:59:59.999Z";
    fractional.end = "2024-01-02T00:59:59.999Z";
    assert_partition(fractional, "2024-01-03");
}

#[test]
fn excluded_and_nonmember_boundaries_cannot_produce_a_split() {
    let exceptions = vec!["2024-01-03".into()];
    let mut input = source(Some("FREQ=DAILY;INTERVAL=2;COUNT=3"));
    input.exceptions = &exceptions;
    let template = Template::from_stored(input).unwrap();
    for date in ["2024-01-02", "2024-01-03", "2024-01-07"] {
        let result = template
            .partition_at_with_budget(parse_date(date).unwrap(), &mut ExpansionBudget::default());
        assert!(result.unwrap_err().contains("not a live source occurrence"));
    }
}

#[test]
fn an_unlimited_following_side_keeps_its_open_termination() {
    let plan = assert_partition(source(Some("FREQ=DAILY;INTERVAL=2")), "2024-01-09");
    let following = rule::parse(plan.following.rrule.as_deref().unwrap()).unwrap();
    assert!(following.count.is_none() && following.until.is_none());
    assert_eq!(
        rule::parse(plan.before.rrule.as_deref().unwrap())
            .unwrap()
            .count,
        Some(4)
    );
}

#[test]
fn an_oversized_prefix_fails_without_returning_a_partial_partition() {
    let template = Template::from_stored(source(Some("FREQ=DAILY"))).unwrap();
    let error = template
        .partition_at_with_budget(
            parse_date("2055-01-01").unwrap(),
            &mut ExpansionBudget::default(),
        )
        .unwrap_err();
    assert!(error.contains("10000 occurrences"), "{error}");
}
