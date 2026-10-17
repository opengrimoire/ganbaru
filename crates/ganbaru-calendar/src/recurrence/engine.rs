//! Period-based recurrence selection with checked arithmetic and shared work limits.

use std::collections::BTreeSet;

use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, Weekday};

use super::rule::{ByDay, Frequency, Rule, Until};

const MAX_CANDIDATE_DAYS: usize = 500_000;
const MAX_OCCURRENCES: usize = 10_000;

/// One request shares its work allowance across every source template and override lookup.
#[derive(Default)]
pub(super) struct ExpansionBudget {
    candidates: usize,
    occurrences: usize,
}

impl ExpansionBudget {
    fn candidate(&mut self) -> Result<(), String> {
        self.candidates += 1;
        if self.candidates > MAX_CANDIDATE_DAYS {
            return Err("Calendar recurrence request exceeds its candidate budget; narrow the requested window".into());
        }
        Ok(())
    }

    pub(super) fn occurrence(&mut self) -> Result<(), String> {
        self.occurrences += 1;
        if self.occurrences > MAX_OCCURRENCES {
            return Err("Calendar recurrence request exceeds 10000 occurrences; narrow the requested window".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Seed {
    pub date: NaiveDate,
    pub instant_ms: i64,
}

fn add_days(date: NaiveDate, days: i64) -> Result<NaiveDate, String> {
    let result = if days < 0 {
        date.checked_sub_days(Days::new(days.unsigned_abs()))
    } else {
        date.checked_add_days(Days::new(days as u64))
    };
    result.ok_or_else(|| "Calendar recurrence exceeds the civil date range".into())
}

fn date(year: i32, month: u32, day: u32) -> Result<NaiveDate, String> {
    if !(1..=9_999).contains(&year) {
        return Err("Calendar recurrence year must be within 1..=9999".into());
    }
    NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| "invalid Calendar recurrence date".into())
}

fn week_start(value: NaiveDate, weekday: Weekday) -> Result<NaiveDate, String> {
    let offset = (i64::from(value.weekday().num_days_from_monday())
        - i64::from(weekday.num_days_from_monday()))
    .rem_euclid(7);
    add_days(value, -offset)
}

fn week_year_start(year: i32, weekday: Weekday) -> Result<NaiveDate, String> {
    week_start(date(year, 1, 4)?, weekday)
}

fn month_length(value: NaiveDate) -> u32 {
    let mut days = 31;
    while NaiveDate::from_ymd_opt(value.year(), value.month(), days).is_none() {
        days -= 1;
    }
    days
}

fn signed_position_matches(values: &[i16], position: u32, length: u32) -> bool {
    values.is_empty()
        || values.iter().any(|&value| {
            let target = if value > 0 {
                i32::from(value)
            } else {
                length as i32 + i32::from(value) + 1
            };
            position as i32 == target
        })
}

fn matches_by_day(value: NaiveDate, rule: &Rule, day: &ByDay) -> bool {
    if day.weekday != value.weekday() {
        return false;
    }
    let Some(ordinal) = day.ordinal else {
        return true;
    };
    let month_scope = rule.frequency == Frequency::Monthly || !rule.by_month.is_empty();
    let (position, length) = if month_scope {
        (value.day(), month_length(value))
    } else {
        (value.ordinal(), if value.leap_year() { 366 } else { 365 })
    };
    if ordinal > 0 {
        (position - 1) / 7 + 1 == ordinal as u32
    } else {
        (length - position) / 7 + 1 == u32::from(ordinal.unsigned_abs())
    }
}

fn matches(
    value: NaiveDate,
    anchor: NaiveDate,
    period_year: i32,
    rule: &Rule,
) -> Result<bool, String> {
    if !rule.by_month.is_empty() && !rule.by_month.contains(&(value.month() as i16)) {
        return Ok(false);
    }
    if !signed_position_matches(&rule.by_month_day, value.day(), month_length(value)) {
        return Ok(false);
    }
    if !rule.by_year_day.is_empty()
        && (value.year() != period_year
            || !signed_position_matches(
                &rule.by_year_day,
                value.ordinal(),
                if value.leap_year() { 366 } else { 365 },
            ))
    {
        return Ok(false);
    }
    if !rule.by_week_no.is_empty() {
        let first = week_year_start(period_year, rule.week_start)?;
        let next = week_year_start(period_year + 1, rule.week_start)?;
        if value < first
            || value >= next
            || !signed_position_matches(
                &rule.by_week_no,
                ((value - first).num_days() / 7 + 1) as u32,
                ((next - first).num_days() / 7) as u32,
            )
        {
            return Ok(false);
        }
    }
    if !rule.by_day.is_empty()
        && !rule
            .by_day
            .iter()
            .any(|day| matches_by_day(value, rule, day))
    {
        return Ok(false);
    }
    let has_day_selector = !rule.by_day.is_empty()
        || !rule.by_month_day.is_empty()
        || !rule.by_year_day.is_empty()
        || !rule.by_week_no.is_empty();
    Ok(match rule.frequency {
        Frequency::Daily => true,
        Frequency::Weekly => !rule.by_day.is_empty() || value.weekday() == anchor.weekday(),
        Frequency::Monthly => has_day_selector || value.day() == anchor.day(),
        Frequency::Yearly if !rule.by_week_no.is_empty() && rule.by_day.is_empty() => {
            value.weekday() == anchor.weekday()
        }
        Frequency::Yearly => {
            has_day_selector
                || (value.day() == anchor.day()
                    && (!rule.by_month.is_empty() || value.month() == anchor.month()))
        }
    })
}

fn period(
    anchor: NaiveDate,
    rule: &Rule,
    index: i64,
) -> Result<(NaiveDate, NaiveDate, i32), String> {
    let offset = index
        .checked_mul(i64::from(rule.interval))
        .ok_or("Calendar recurrence interval overflow")?;
    match rule.frequency {
        Frequency::Daily => {
            let start = add_days(anchor, offset)?;
            Ok((start, add_days(start, 1)?, start.year()))
        }
        Frequency::Weekly => {
            let start = add_days(
                week_start(anchor, rule.week_start)?,
                offset
                    .checked_mul(7)
                    .ok_or("Calendar recurrence week overflow")?,
            )?;
            Ok((start, add_days(start, 7)?, start.year()))
        }
        Frequency::Monthly => {
            let month = i64::from(anchor.year()) * 12 + i64::from(anchor.month0()) + offset;
            let year = i32::try_from(month.div_euclid(12))
                .map_err(|_| "Calendar recurrence year overflow")?;
            let start = date(year, month.rem_euclid(12) as u32 + 1, 1)?;
            Ok((
                start,
                add_days(start, i64::from(month_length(start)))?,
                year,
            ))
        }
        Frequency::Yearly => {
            let year = i32::try_from(i64::from(anchor.year()) + offset)
                .map_err(|_| "Calendar recurrence year overflow")?;
            let (start, end) = if rule.by_week_no.is_empty() {
                (date(year, 1, 1)?, date(year + 1, 1, 1)?)
            } else {
                (
                    week_year_start(year, rule.week_start)?,
                    week_year_start(year + 1, rule.week_start)?,
                )
            };
            Ok((start, end, year))
        }
    }
}

fn first_period(anchor: NaiveDate, lower: NaiveDate, rule: &Rule) -> i64 {
    if rule.count.is_some() || lower <= anchor {
        return 0;
    }
    let distance = match rule.frequency {
        Frequency::Daily => (lower - anchor).num_days(),
        Frequency::Weekly => (lower - anchor).num_days() / 7,
        Frequency::Monthly => {
            i64::from(lower.year() - anchor.year()) * 12 + i64::from(lower.month())
                - i64::from(anchor.month())
        }
        Frequency::Yearly => i64::from(lower.year() - anchor.year()),
    };
    (distance / i64::from(rule.interval) - 1).max(0)
}

fn past_until(rule: &Rule, anchor: NaiveDateTime, seed: Seed) -> bool {
    match rule.until {
        None => false,
        Some(Until::Date(until)) => seed.date > until,
        Some(Until::Local(until)) => seed.date.and_time(anchor.time()) > until,
        Some(Until::Instant(until)) => seed.instant_ms > until,
    }
}

/// Select the limited RRULE set before exclusions. The caller supplies home-zone gap semantics.
pub(super) fn generate(
    rule: &Rule,
    anchor: NaiveDateTime,
    anchor_ms: i64,
    lower: NaiveDate,
    upper: NaiveDate,
    budget: &mut ExpansionBudget,
    instant: impl FnMut(NaiveDateTime) -> Result<Option<i64>, String>,
) -> Result<Vec<Seed>, String> {
    generate_with_completion(rule, anchor, anchor_ms, lower, upper, budget, instant)
        .map(|result| result.seeds)
}

pub(super) struct Generation {
    pub seeds: Vec<Seed>,
    pub exhausted: bool,
}

/// Report proven COUNT/UNTIL exhaustion separately from an empty requested range.
pub(super) fn generate_with_completion(
    rule: &Rule,
    anchor: NaiveDateTime,
    anchor_ms: i64,
    lower: NaiveDate,
    upper: NaiveDate,
    budget: &mut ExpansionBudget,
    mut instant: impl FnMut(NaiveDateTime) -> Result<Option<i64>, String>,
) -> Result<Generation, String> {
    if lower > upper {
        return Err("Calendar recurrence window starts after its end".into());
    }
    let mut result = Vec::new();
    if anchor.date() >= lower && anchor.date() <= upper {
        budget.occurrence()?;
        result.push(Seed {
            date: anchor.date(),
            instant_ms: anchor_ms,
        });
    }
    let mut counted = 1_u32;
    let mut index = first_period(anchor.date(), lower, rule);
    loop {
        if rule.count.is_some_and(|limit| counted >= limit) {
            return Ok(Generation {
                seeds: result,
                // COUNT includes DTSTART even when this requested range ends
                // before it. That future explicit anchor is not exhausted yet.
                exhausted: anchor.date() <= upper,
            });
        }
        let (start, end, year) = period(anchor.date(), rule, index)?;
        if start > upper {
            break;
        }
        let mut candidates = Vec::new();
        let mut cursor = start;
        while cursor < end {
            budget.candidate()?;
            if matches(cursor, anchor.date(), year, rule)? {
                candidates.push(cursor);
            }
            cursor = add_days(cursor, 1)?;
        }
        if !rule.by_set_pos.is_empty() {
            let mut selected = BTreeSet::new();
            for &position in &rule.by_set_pos {
                let index = if position > 0 {
                    i32::from(position) - 1
                } else {
                    candidates.len() as i32 + i32::from(position)
                };
                if index >= 0 {
                    if let Some(value) = candidates.get(index as usize) {
                        selected.insert(*value);
                    }
                }
            }
            candidates = selected.into_iter().collect();
        }
        for date in candidates {
            if date <= anchor.date() || date > upper {
                continue;
            }
            let Some(instant_ms) = instant(date.and_time(anchor.time()))? else {
                continue;
            };
            let seed = Seed { date, instant_ms };
            if past_until(rule, anchor, seed) {
                return Ok(Generation {
                    seeds: result,
                    exhausted: true,
                });
            }
            counted = counted
                .checked_add(1)
                .ok_or("Calendar recurrence count overflow")?;
            if date >= lower {
                budget.occurrence()?;
                result.push(seed);
            }
            if rule.count.is_some_and(|limit| counted >= limit) {
                return Ok(Generation {
                    seeds: result,
                    exhausted: true,
                });
            }
        }
        index += 1;
    }
    Ok(Generation {
        seeds: result,
        exhausted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(rule: &str, start: &str, end: &str) -> Vec<String> {
        let anchor = NaiveDate::parse_from_str(start, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        generate(
            &super::super::rule::parse(rule).unwrap(),
            anchor,
            anchor.and_utc().timestamp_millis(),
            anchor.date(),
            NaiveDate::parse_from_str(end, "%Y-%m-%d").unwrap(),
            &mut ExpansionBudget::default(),
            |value| Ok(Some(value.and_utc().timestamp_millis())),
        )
        .unwrap()
        .into_iter()
        .map(|seed| seed.date.to_string())
        .collect()
    }

    #[test]
    fn monthly_and_leap_day_anchors_skip_invalid_dates_without_drifting() {
        assert_eq!(
            dates("FREQ=MONTHLY;COUNT=4", "2024-01-31", "2024-08-01"),
            ["2024-01-31", "2024-03-31", "2024-05-31", "2024-07-31"]
        );
        assert_eq!(
            dates("FREQ=YEARLY;COUNT=3", "2024-02-29", "2033-01-01"),
            ["2024-02-29", "2028-02-29", "2032-02-29"]
        );
    }

    #[test]
    fn intersections_and_set_positions_apply_within_the_complete_period() {
        assert_eq!(
            dates(
                "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=4",
                "2024-01-01",
                "2024-04-01"
            ),
            ["2024-01-01", "2024-01-31", "2024-02-29", "2024-03-29"]
        );
        assert_eq!(
            dates(
                "FREQ=MONTHLY;BYMONTHDAY=1,2,3,4,5,6,7;BYDAY=MO;COUNT=3",
                "2024-01-01",
                "2024-04-01"
            ),
            ["2024-01-01", "2024-02-05", "2024-03-04"]
        );
        assert_eq!(
            dates("FREQ=MONTHLY;BYDAY=MO;COUNT=4", "2024-01-01", "2024-02-01"),
            ["2024-01-01", "2024-01-08", "2024-01-15", "2024-01-22"]
        );
    }

    #[test]
    fn weekly_intervals_honor_the_imported_week_start() {
        assert_eq!(
            dates(
                "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,SU;WKST=MO;COUNT=4",
                "1997-08-05",
                "1997-09-01"
            ),
            ["1997-08-05", "1997-08-10", "1997-08-19", "1997-08-24"]
        );
        assert_eq!(
            dates(
                "FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,SU;WKST=SU;COUNT=4",
                "1997-08-05",
                "1997-09-01"
            ),
            ["1997-08-05", "1997-08-17", "1997-08-19", "1997-08-31"]
        );
    }

    #[test]
    fn year_days_week_numbers_and_yearly_ordinals_keep_their_scope() {
        assert_eq!(
            dates(
                "FREQ=YEARLY;BYYEARDAY=60,-1;COUNT=5",
                "2023-01-01",
                "2025-01-01"
            ),
            [
                "2023-01-01",
                "2023-03-01",
                "2023-12-31",
                "2024-02-29",
                "2024-12-31"
            ]
        );
        assert_eq!(
            dates(
                "FREQ=YEARLY;BYWEEKNO=1;BYDAY=MO;COUNT=4",
                "2023-01-02",
                "2026-01-01"
            ),
            ["2023-01-02", "2024-01-01", "2024-12-30", "2025-12-29"]
        );
        assert_eq!(
            dates("FREQ=YEARLY;BYDAY=-1MO;COUNT=3", "2023-01-02", "2025-01-01"),
            ["2023-01-02", "2023-12-25", "2024-12-30"]
        );
    }

    #[test]
    fn nonexistent_local_times_do_not_consume_count_and_utc_until_compares_instants() {
        let anchor = NaiveDate::from_ymd_opt(2024, 3, 9)
            .unwrap()
            .and_hms_opt(2, 30, 0)
            .unwrap();
        let zone = ganbaru_civil_time::zone("America/New_York").unwrap();
        let rule = super::super::rule::parse("FREQ=DAILY;COUNT=3").unwrap();
        let seeds = generate(
            &rule,
            anchor,
            ganbaru_civil_time::explicit_instant(anchor, &zone).unwrap(),
            anchor.date(),
            NaiveDate::from_ymd_opt(2024, 3, 20).unwrap(),
            &mut ExpansionBudget::default(),
            |value| ganbaru_civil_time::generated_instant(value, &zone),
        )
        .unwrap();
        assert_eq!(
            seeds
                .iter()
                .map(|seed| seed.date.to_string())
                .collect::<Vec<_>>(),
            ["2024-03-09", "2024-03-11", "2024-03-12"]
        );
        assert_eq!(
            dates(
                "FREQ=DAILY;UNTIL=20240102T085959Z",
                "2024-01-01",
                "2024-01-05"
            ),
            ["2024-01-01"]
        );
    }

    #[test]
    fn long_running_unlimited_rules_seek_to_the_window_and_limits_fail_explicitly() {
        let anchor = NaiveDate::from_ymd_opt(1900, 1, 1)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap();
        let lower = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let mut budget = ExpansionBudget::default();
        let result = generate(
            &super::super::rule::parse("FREQ=DAILY").unwrap(),
            anchor,
            anchor.and_utc().timestamp_millis(),
            lower,
            add_days(lower, 6).unwrap(),
            &mut budget,
            |value| Ok(Some(value.and_utc().timestamp_millis())),
        )
        .unwrap();
        assert_eq!(result.len(), 7);
        assert!(budget.candidates < 10);
        budget.occurrences = MAX_OCCURRENCES;
        assert!(budget.occurrence().is_err());
        budget.candidates = MAX_CANDIDATE_DAYS;
        assert!(budget.candidate().is_err());
    }
}
