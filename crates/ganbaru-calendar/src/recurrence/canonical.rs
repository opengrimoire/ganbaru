//! Canonical home-zone occurrence expansion independent of frontend projections.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
use jiff::tz::TimeZone;

use super::engine::{ExpansionBudget, Seed, generate};
use super::rule;
use ganbaru_civil_time as civil_time;

mod scope;
pub(crate) use scope::{EditScope, ScopeClock, ScopeEvidence, ScopePlan};
mod deletion;
pub(crate) use deletion::{DeleteOutcome, DeletePlan};
mod edit;
pub(crate) use edit::{RecurrenceIntent, ResolvedTiming, TimingIntent};
mod edit_plan;
pub(crate) use edit_plan::{
    ActiveTarget, EditGeometryPlan, EditKind, GeometryDraft, PlannedSeries, SelectedTarget,
};
mod partition;
pub(crate) use partition::{PartitionSide, SeriesPartition};

const MAX_WINDOW_DAYS: i64 = 3_660;
const MAX_ID_BYTES: usize = 1_024;

/// One captured render window, with inclusive dates and an exclusive instant end.
#[derive(Clone)]
pub struct Window {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub start_ms: i64,
    pub end_exclusive_ms: i64,
}

impl Window {
    /// Builds a render window from inclusive local dates in the render zone.
    pub fn new(start: &str, end: &str, render_zone: &TimeZone) -> Result<Self, String> {
        let start_date = parse_date(start)?;
        let end_date = parse_date(end)?;
        let days = (end_date - start_date).num_days();
        if !(0..=MAX_WINDOW_DAYS).contains(&days) {
            return Err("Calendar window must be ordered and no longer than 3660 days".into());
        }
        let next = end_date
            .checked_add_signed(Duration::days(1))
            .ok_or("Calendar window exceeds its date range")?;
        let midnight = chrono::NaiveTime::MIN;
        Ok(Self {
            start_date,
            end_date,
            start_ms: civil_time::explicit_instant(start_date.and_time(midnight), render_zone)?,
            end_exclusive_ms: civil_time::explicit_instant(next.and_time(midnight), render_zone)?,
        })
    }

    fn overlaps(&self, occurrence: &Occurrence, all_day: bool) -> bool {
        if all_day {
            occurrence.end_date >= self.start_date && occurrence.start_date <= self.end_date
        } else {
            occurrence.end_ms >= self.start_ms && occurrence.start_ms < self.end_exclusive_ms
        }
    }
}

pub(crate) struct StoredOverride {
    pub recurrence_id: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub cancelled: bool,
    pub this_and_future: bool,
}

/// Typed canonical input assembled by the Calendar database adapter.
pub(crate) struct StoredTemplate<'a> {
    pub id: &'a str,
    pub start: &'a str,
    pub end: &'a str,
    pub home_zone: &'a str,
    pub all_day: bool,
    pub rrule: Option<&'a str>,
    pub repeat_until: Option<&'a str>,
    pub exceptions: &'a [String],
    pub rdates: &'a [String],
    pub overrides: Vec<StoredOverride>,
}

/// Stable recurrence provenance plus concrete instants and optional applied override index.
#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    pub id: String,
    pub parent_id: Option<String>,
    pub recurrence_date: NaiveDate,
    pub start_ms: i64,
    pub end_ms: i64,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub override_index: Option<usize>,
}

pub(crate) struct Template {
    id: String,
    all_day: bool,
    home_zone: TimeZone,
    start: NaiveDateTime,
    end: NaiveDateTime,
    start_ms: i64,
    end_ms: i64,
    rule: Option<rule::Rule>,
    exclusions: BTreeSet<NaiveDate>,
    additions: BTreeMap<NaiveDate, Seed>,
    overrides: BTreeMap<NaiveDate, (usize, StoredOverride)>,
    cancel_from: Option<NaiveDate>,
}

impl Template {
    /// Home-zone identity of the explicit DTSTART, including a standalone source.
    pub(crate) fn anchor_date(&self) -> NaiveDate {
        self.start.date()
    }
}

pub fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|error| format!("invalid Calendar civil date {value}: {error}"))
}

fn parse_civil(value: &str) -> Result<NaiveDateTime, String> {
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Ok(parsed);
        }
    }
    Err(format!("invalid Calendar civil datetime: {value}"))
}

/// Resolve a stored or authored endpoint, preserving explicit offsets and native
/// home-zone gap and fold semantics for compound scheduling callers.
pub(crate) fn stored_time(
    value: &str,
    home_zone: &TimeZone,
    all_day: bool,
) -> Result<(NaiveDateTime, i64), String> {
    if all_day {
        let date = value.get(..10).ok_or("invalid all-day Calendar date")?;
        let civil = parse_date(date)?.and_time(chrono::NaiveTime::MIN);
        return Ok((civil, civil.and_utc().timestamp_millis()));
    }
    if let Ok(instant) = DateTime::parse_from_rfc3339(value) {
        let epoch_ms = instant.timestamp_millis();
        return Ok((civil_time::instant_to_local(epoch_ms, home_zone)?, epoch_ms));
    }
    // Offset-free wall-clock values (kept for civil times in a gap) resolve in the home zone.
    let civil = parse_civil(value)?;
    Ok((civil, civil_time::explicit_instant(civil, home_zone)?))
}

/// Interpret a persisted override identity with the same rules as expansion.
pub(crate) fn override_date(
    value: &str,
    home_zone: &TimeZone,
    all_day: bool,
) -> Result<NaiveDate, String> {
    if value.len() == 10 {
        parse_date(value)
    } else {
        Ok(stored_time(value, home_zone, all_day)?.0.date())
    }
}

impl Template {
    /// Resolve an original home-zone identity, independently of its displayed date.
    /// A missing date selects the stored anchor. Excluded or nonexistent identities
    /// return no occurrence, including dates beyond COUNT and UNTIL.
    pub(crate) fn resolve_identity(
        &self,
        date: Option<NaiveDate>,
    ) -> Result<Option<Occurrence>, String> {
        self.resolve_identity_with_budget(
            date.unwrap_or(self.start.date()),
            &mut ExpansionBudget::default(),
        )
    }

    fn resolve_identity_with_budget(
        &self,
        date: NaiveDate,
        budget: &mut ExpansionBudget,
    ) -> Result<Option<Occurrence>, String> {
        let seed = if let Some(rule) = &self.rule {
            generate(
                rule,
                self.start,
                self.start_ms,
                date,
                date,
                budget,
                |value| self.generated_instant(value),
            )?
            .into_iter()
            .next()
        } else if date == self.start.date() {
            budget.occurrence()?;
            Some(Seed {
                date,
                instant_ms: self.start_ms,
            })
        } else {
            None
        };
        let seed = match seed {
            Some(seed) => Some(seed),
            None => self
                .additions
                .get(&date)
                .copied()
                .map(|seed| {
                    budget.occurrence()?;
                    Ok::<_, String>(seed)
                })
                .transpose()?,
        };
        seed.map(|seed| self.occurrence(seed))
            .transpose()
            .map(Option::flatten)
    }

    /// Decode canonical storage; unsupported forms remain explicitly diagnosable.
    pub(crate) fn from_stored(input: StoredTemplate<'_>) -> Result<Self, String> {
        let StoredTemplate {
            id,
            start,
            end,
            home_zone,
            all_day,
            rrule,
            repeat_until,
            exceptions,
            rdates,
            overrides,
        } = input;
        if id.trim().is_empty() || id.len() > MAX_ID_BYTES {
            return Err("Calendar template identity is empty or exceeds 1024 bytes".into());
        }
        let home_zone = if all_day {
            TimeZone::UTC
        } else {
            civil_time::zone(home_zone)?
        };
        let (start, start_ms) = stored_time(start, &home_zone, all_day)?;
        let (end, end_ms) = stored_time(end, &home_zone, all_day)?;
        if (all_day && end < start) || (!all_day && end_ms <= start_ms) {
            return Err(
                "Calendar template must have a valid positive timed range or ordered all-day dates"
                    .into(),
            );
        }
        let mut rule = rrule
            .filter(|value| !value.is_empty())
            .map(rule::parse)
            .transpose()?;
        // Calendar imports can store rule termination in a separate column.
        if let Some(rule) = &mut rule {
            if rule.count.is_none() && rule.until.is_none() {
                rule.until = repeat_until
                    .map(parse_date)
                    .transpose()?
                    .map(rule::Until::Date);
            }
        }
        let mut exclusions = BTreeSet::new();
        for value in exceptions {
            let civil = if value.len() == 10 {
                parse_date(value)?
            } else {
                stored_time(value, &home_zone, all_day)?.0.date()
            };
            exclusions.insert(civil);
        }
        let mut additions = BTreeMap::new();
        for value in rdates {
            let (civil, instant_ms) = if value.len() == 10 {
                let civil = parse_date(value)?.and_time(start.time());
                let instant = if all_day {
                    civil.and_utc().timestamp_millis()
                } else {
                    civil_time::explicit_instant(civil, &home_zone)?
                };
                (civil, instant)
            } else {
                stored_time(value, &home_zone, all_day)?
            };
            if !all_day && civil.time() != start.time() {
                return Err("Calendar RDATE with a different local time is preservation-only until datetime recurrence identities are supported".into());
            }
            let seed = Seed {
                date: civil.date(),
                instant_ms,
            };
            if let Some(previous) = additions.insert(seed.date, seed) {
                if previous.instant_ms != seed.instant_ms {
                    return Err(
                        "Calendar RDATE has multiple instants for one recurrence date".into(),
                    );
                }
            }
        }
        let mut indexed_overrides = BTreeMap::new();
        let mut cancel_from = None;
        for (index, value) in overrides.into_iter().enumerate() {
            let recurrence_date = override_date(&value.recurrence_id, &home_zone, all_day)?;
            if value.this_and_future {
                if !value.cancelled {
                    return Err(
                        "Calendar non-cancelled RANGE=THISANDFUTURE is preservation-only".into(),
                    );
                }
                cancel_from = Some(cancel_from.map_or(recurrence_date, |previous: NaiveDate| {
                    previous.min(recurrence_date)
                }));
            }
            if indexed_overrides
                .insert(recurrence_date, (index, value))
                .is_some()
            {
                return Err("Calendar has duplicate overrides for one recurrence identity".into());
            }
        }
        Ok(Self {
            id: id.into(),
            all_day,
            home_zone,
            start,
            end,
            start_ms,
            end_ms,
            rule,
            exclusions,
            additions,
            overrides: indexed_overrides,
            cancel_from,
        })
    }

    fn generated_instant(&self, value: NaiveDateTime) -> Result<Option<i64>, String> {
        if self.all_day {
            Ok(Some(value.and_utc().timestamp_millis()))
        } else {
            civil_time::generated_instant(value, &self.home_zone)
        }
    }

    fn occurrence(&self, seed: Seed) -> Result<Option<Occurrence>, String> {
        self.occurrence_geometry(seed, true)
    }

    fn occurrence_geometry(
        &self,
        seed: Seed,
        apply_override: bool,
    ) -> Result<Option<Occurrence>, String> {
        if self.exclusions.contains(&seed.date)
            || self.cancel_from.is_some_and(|from| seed.date >= from)
        {
            return Ok(None);
        }
        let indexed = apply_override
            .then(|| self.overrides.get(&seed.date))
            .flatten();
        if indexed.is_some_and(|(_, value)| value.cancelled) {
            return Ok(None);
        }
        let day_span = (self.end.date() - self.start.date()).num_days();
        let end_date = seed
            .date
            .checked_add_signed(Duration::days(day_span))
            .ok_or("Calendar occurrence end date overflow")?;
        let mut start_date = seed.date;
        let mut end_date = end_date;
        let mut start_ms = seed.instant_ms;
        let mut end_ms = if seed.date == self.start.date() {
            self.end_ms
        } else if self.all_day {
            end_date
                .and_time(self.end.time())
                .and_utc()
                .timestamp_millis()
        } else if self.end <= self.start {
            // A stored positive interval can cross a fold with a backwards wall
            // clock. Preserve its elapsed duration instead of rejecting it or
            // generating a negative civil interval on an ordinary day.
            start_ms
                .checked_add(self.end_ms - self.start_ms)
                .ok_or("Calendar elapsed duration exceeds its instant range")?
        } else {
            civil_time::explicit_instant(end_date.and_time(self.end.time()), &self.home_zone)?
        };
        if !self.all_day && indexed.is_none() && end_ms <= start_ms {
            // Explicit RDATE may select the later fold while a compatible end
            // resolves to the earlier fold. Retain a positive source duration.
            end_ms = start_ms
                .checked_add(self.end_ms - self.start_ms)
                .ok_or("Calendar elapsed duration exceeds its instant range")?;
        }
        if let Some((_, value)) = indexed {
            if let Some(start) = &value.start {
                let (civil, instant) = stored_time(start, &self.home_zone, self.all_day)?;
                start_ms = instant;
                start_date = civil.date();
            }
            if let Some(end) = &value.end {
                let (civil, instant) = stored_time(end, &self.home_zone, self.all_day)?;
                end_ms = instant;
                end_date = civil.date();
            }
        }
        if end_date < start_date || (!self.all_day && end_ms <= start_ms) {
            return Err("Calendar occurrence override produces an invalid range".into());
        }
        let original = seed.date == self.start.date();
        Ok(Some(Occurrence {
            id: if original {
                self.id.clone()
            } else {
                format!("{}::{}", self.id, seed.date)
            },
            parent_id: (!original).then(|| self.id.clone()),
            recurrence_date: seed.date,
            start_ms,
            end_ms,
            start_date,
            end_date,
            override_index: indexed.map(|(index, _)| *index),
        }))
    }

    /// Resolve sets in the home zone and filter only after moved overrides are applied.
    fn expand(
        &self,
        window: &Window,
        budget: &mut ExpansionBudget,
    ) -> Result<Vec<Occurrence>, String> {
        let day_span = (self.end.date() - self.start.date()).num_days();
        let (lower, upper) = if self.all_day {
            (window.start_date, window.end_date)
        } else {
            (
                civil_time::instant_to_local(window.start_ms, &self.home_zone)?.date(),
                civil_time::instant_to_local(window.end_exclusive_ms - 1, &self.home_zone)?.date(),
            )
        };
        let lower = lower
            .checked_sub_signed(Duration::days(day_span))
            .ok_or("Calendar overlap window overflow")?;
        let mut seeds = BTreeMap::new();
        if let Some(rule) = &self.rule {
            for seed in generate(
                rule,
                self.start,
                self.start_ms,
                lower,
                upper,
                budget,
                |value| self.generated_instant(value),
            )? {
                seeds.insert(seed.date, seed);
            }
        } else {
            seeds.insert(
                self.start.date(),
                Seed {
                    date: self.start.date(),
                    instant_ms: self.start_ms,
                },
            );
        }
        // An exact override can move an otherwise off-window recurrence into the window.
        for &date in self.overrides.keys() {
            if date >= lower && date <= upper {
                continue;
            }
            let Some(instant_ms) = self.generated_instant(date.and_time(self.start.time()))? else {
                continue;
            };
            let provisional = Seed { date, instant_ms };
            if self
                .occurrence(provisional)?
                .is_none_or(|value| !window.overlaps(&value, self.all_day))
            {
                continue;
            }
            if let Some(rule) = &self.rule {
                for seed in generate(
                    rule,
                    self.start,
                    self.start_ms,
                    date,
                    date,
                    budget,
                    |value| self.generated_instant(value),
                )? {
                    seeds.insert(seed.date, seed);
                }
            }
        }
        // RDATE is independent of RRULE COUNT and UNTIL; EXDATE wins below.
        for (&date, &seed) in &self.additions {
            if let std::collections::btree_map::Entry::Vacant(entry) = seeds.entry(date) {
                budget.occurrence()?;
                entry.insert(seed);
            }
        }
        let mut result = Vec::new();
        for seed in seeds.into_values() {
            if let Some(value) = self.occurrence(seed)? {
                if window.overlaps(&value, self.all_day) {
                    if self.rule.is_none() && value.recurrence_date == self.start.date() {
                        budget.occurrence()?;
                    }
                    result.push(value);
                }
            }
        }
        Ok(result)
    }
}

/// Every template shares one request budget; outputs retain corresponding input order.
pub(crate) fn expand_templates(
    templates: &[Template],
    window: &Window,
) -> Result<Vec<Vec<Occurrence>>, String> {
    let mut budget = ExpansionBudget::default();
    templates
        .iter()
        .map(|template| template.expand(window, &mut budget))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_google_utc_terminations_do_not_reappear_in_later_windows() {
        let window = Window::new("2026-05-01", "2026-05-31", &TimeZone::UTC).unwrap();
        for (start, end, rrule) in [
            (
                "2021-02-08T12:00:00Z",
                "2021-02-08T12:30:00Z",
                "FREQ=DAILY;UNTIL=20210322T055959Z",
            ),
            (
                "2021-05-11T14:00:00Z",
                "2021-05-11T18:00:00Z",
                "FREQ=WEEKLY;WKST=MO;UNTIL=20210612T045959Z;BYDAY=FR,TU,WE",
            ),
        ] {
            let template = Template::from_stored(StoredTemplate {
                id: "imported",
                start,
                end,
                home_zone: "America/Mexico_City",
                all_day: false,
                rrule: Some(rrule),
                repeat_until: None,
                exceptions: &[],
                rdates: &[],
                overrides: Vec::new(),
            })
            .unwrap();
            assert!(
                expand_templates(&[template], &window).unwrap()[0].is_empty(),
                "{rrule}"
            );
        }
    }

    #[test]
    fn shared_recurrence_sets_keep_count_exclusions_additions_and_window_overlap() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Fixture {
            name: String,
            start_date: String,
            end_date: String,
            count: Option<u32>,
            until: Option<String>,
            exceptions: Vec<String>,
            rdate: Vec<String>,
            window_start: String,
            window_end: String,
            expected_dates: Vec<String>,
        }
        let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/client/src/lib/calendar/recurrence-set-fixtures.json"
        )))
        .unwrap();
        for fixture in fixtures {
            let mut rrule = "FREQ=DAILY".to_string();
            if let Some(count) = fixture.count {
                rrule.push_str(&format!(";COUNT={count}"));
            }
            if let Some(until) = fixture.until {
                rrule.push_str(&format!(";UNTIL={}", until.replace('-', "")));
            }
            let start = format!("{}T09:00:00Z", fixture.start_date);
            let end = format!("{}T10:00:00Z", fixture.end_date);
            let template = Template::from_stored(StoredTemplate {
                id: "shared",
                start: &start,
                end: &end,
                home_zone: "UTC",
                all_day: false,
                rrule: Some(&rrule),
                repeat_until: None,
                exceptions: &fixture.exceptions,
                rdates: &fixture.rdate,
                overrides: Vec::new(),
            })
            .unwrap();
            let window =
                Window::new(&fixture.window_start, &fixture.window_end, &TimeZone::UTC).unwrap();
            let occurrences = expand_templates(&[template], &window)
                .unwrap()
                .pop()
                .unwrap();
            assert_eq!(
                occurrences
                    .iter()
                    .map(|row| row.start_date.to_string())
                    .collect::<Vec<_>>(),
                fixture.expected_dates,
                "{}",
                fixture.name
            );
            let identities: BTreeSet<_> = occurrences.iter().map(|row| &row.id).collect();
            assert_eq!(identities.len(), occurrences.len(), "{}", fixture.name);
        }
    }

    #[test]
    fn identity_lookup_retains_explicit_fold_and_moved_override_provenance() {
        let mut source = input(Some("FREQ=DAILY;COUNT=2"));
        source.start = "2024-11-03T06:30:00Z";
        source.end = "2024-11-03T07:30:00Z";
        source.overrides.push(StoredOverride {
            recurrence_id: "2024-11-04".into(),
            start: Some("2024-12-20T15:00:00Z".into()),
            end: Some("2024-12-20T16:00:00Z".into()),
            cancelled: false,
            this_and_future: false,
        });
        let template = Template::from_stored(source).unwrap();
        let anchor = template.resolve_identity(None).unwrap().unwrap();
        assert_eq!(
            anchor.start_ms,
            DateTime::parse_from_rfc3339("2024-11-03T06:30:00Z")
                .unwrap()
                .timestamp_millis()
        );
        let original_date = parse_date("2024-11-04").unwrap();
        let moved = template
            .resolve_identity(Some(original_date))
            .unwrap()
            .unwrap();
        assert_eq!(moved.recurrence_date, original_date);
        assert_eq!(moved.start_date, parse_date("2024-12-20").unwrap());
        assert_eq!(moved.id, "series::2024-11-04");
        assert!(
            template
                .resolve_identity(Some(moved.start_date))
                .unwrap()
                .is_none()
        );
        let window = Window::new("2024-12-20", "2024-12-20", &TimeZone::UTC).unwrap();
        let projected = expand_templates(&[template], &window).unwrap();
        assert_eq!(projected[0].len(), 1);
        assert_eq!(projected[0][0].start_ms, moved.start_ms);
        assert_eq!(projected[0][0].id, moved.id);
    }

    #[test]
    fn identity_lookup_skips_generated_gap_but_keeps_independent_rdate() {
        let mut source = input(Some("FREQ=DAILY;COUNT=2"));
        source.start = "2024-03-09T07:30:00Z";
        source.end = "2024-03-09T08:30:00Z";
        let additions = vec!["2024-03-20".into()];
        source.rdates = &additions;
        let template = Template::from_stored(source).unwrap();
        assert!(
            template
                .resolve_identity(Some(parse_date("2024-03-10").unwrap()))
                .unwrap()
                .is_none()
        );
        assert!(
            template
                .resolve_identity(Some(parse_date("2024-03-11").unwrap()))
                .unwrap()
                .is_some()
        );
        assert!(
            template
                .resolve_identity(Some(parse_date("2024-03-12").unwrap()))
                .unwrap()
                .is_none()
        );
        assert!(
            template
                .resolve_identity(Some(parse_date("2024-03-20").unwrap()))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn identity_lookup_never_treats_exhausted_work_as_a_missing_occurrence() {
        let mut source = input(Some("FREQ=DAILY;COUNT=1000000"));
        source.start = "1000-01-01T09:00:00Z";
        source.end = "1000-01-01T10:00:00Z";
        source.home_zone = "UTC";
        let template = Template::from_stored(source).unwrap();
        let error = template
            .resolve_identity(Some(parse_date("3000-01-01").unwrap()))
            .unwrap_err();
        assert!(error.contains("candidate budget"), "{error}");
    }

    fn input<'a>(rrule: Option<&'a str>) -> StoredTemplate<'a> {
        StoredTemplate {
            id: "series",
            start: "2024-03-09T14:00:00Z",
            end: "2024-03-09T15:00:00Z",
            home_zone: "America/New_York",
            all_day: false,
            rrule,
            repeat_until: None,
            exceptions: &[],
            rdates: &[],
            overrides: Vec::new(),
        }
    }

    fn window(start: &str, end: &str, zone: &str) -> Window {
        Window::new(start, end, &civil_time::zone(zone).unwrap()).unwrap()
    }

    #[test]
    fn positive_fold_crossing_ranges_are_preserved_and_generate_positive_intervals() {
        let mut source = input(Some("FREQ=DAILY;COUNT=2"));
        source.start = "2024-11-03T05:45:00Z";
        source.end = "2024-11-03T06:15:00Z";
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(&[template], &window("2024-11-03", "2024-11-04", "UTC"))
            .unwrap()
            .remove(0);
        assert_eq!(occurrences.len(), 2);
        assert_eq!(
            occurrences[0].start_ms,
            DateTime::parse_from_rfc3339("2024-11-03T05:45:00Z")
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(
            occurrences[0].end_ms,
            DateTime::parse_from_rfc3339("2024-11-03T06:15:00Z")
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(
            occurrences[1].end_ms - occurrences[1].start_ms,
            30 * 60 * 1000
        );
        assert_eq!(
            civil_time::instant_to_local(
                occurrences[1].end_ms,
                &civil_time::zone("America/New_York").unwrap()
            )
            .unwrap()
            .to_string(),
            "2024-11-04 02:15:00"
        );
    }

    #[test]
    fn home_zone_wall_time_survives_dst_and_render_zone_changes() {
        let template = Template::from_stored(input(Some("FREQ=DAILY;COUNT=3"))).unwrap();
        let occurrences = expand_templates(
            &[template],
            &window("2024-03-09", "2024-03-12", "Asia/Tokyo"),
        )
        .unwrap()
        .remove(0);
        assert_eq!(
            occurrences
                .iter()
                .map(|value| value.id.as_str())
                .collect::<Vec<_>>(),
            ["series", "series::2024-03-10", "series::2024-03-11"]
        );
        assert_eq!(
            occurrences[1].start_ms - occurrences[0].start_ms,
            23 * 60 * 60 * 1000
        );
        assert_eq!(
            occurrences[2].start_ms - occurrences[1].start_ms,
            24 * 60 * 60 * 1000
        );
    }

    #[test]
    fn exclusions_subtract_the_limited_rule_and_rdates_remain_independent() {
        let exceptions = vec!["2024-03-10".into()];
        let rdates = vec!["2024-03-15".into(), "2024-03-15".into()];
        let mut source = input(Some("FREQ=DAILY;COUNT=3"));
        source.exceptions = &exceptions;
        source.rdates = &rdates;
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(&[template], &window("2024-03-09", "2024-03-20", "UTC"))
            .unwrap()
            .remove(0);
        assert_eq!(
            occurrences
                .iter()
                .map(|value| value.recurrence_date.to_string())
                .collect::<Vec<_>>(),
            ["2024-03-09", "2024-03-11", "2024-03-15"]
        );
    }

    #[test]
    fn moved_override_enters_the_window_and_keeps_original_provenance() {
        let mut source = input(Some("FREQ=DAILY;COUNT=3"));
        source.overrides.push(StoredOverride {
            recurrence_id: "2024-03-10T13:00:00Z".into(),
            start: Some("2024-03-20T13:00:00Z".into()),
            end: Some("2024-03-20T14:00:00Z".into()),
            cancelled: false,
            this_and_future: false,
        });
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(&[template], &window("2024-03-20", "2024-03-20", "UTC"))
            .unwrap()
            .remove(0);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].id, "series::2024-03-10");
        assert_eq!(occurrences[0].parent_id.as_deref(), Some("series"));
        assert_eq!(occurrences[0].start_date.to_string(), "2024-03-20");
        assert_eq!(occurrences[0].override_index, Some(0));
    }

    #[test]
    fn moved_original_override_is_applied_and_invalid_override_identity_is_not_invented() {
        let mut source = input(Some("FREQ=DAILY;COUNT=2"));
        source.overrides.push(StoredOverride {
            recurrence_id: "2024-03-09".into(),
            start: Some("2024-03-20T13:00:00Z".into()),
            end: Some("2024-03-20T14:00:00Z".into()),
            cancelled: false,
            this_and_future: false,
        });
        source.overrides.push(StoredOverride {
            recurrence_id: "2024-03-30".into(),
            start: Some("2024-03-20T15:00:00Z".into()),
            end: Some("2024-03-20T16:00:00Z".into()),
            cancelled: false,
            this_and_future: false,
        });
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(&[template], &window("2024-03-20", "2024-03-20", "UTC"))
            .unwrap()
            .remove(0);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].id, "series");
    }

    #[test]
    fn all_day_dates_do_not_shift_with_home_or_render_timezones() {
        let mut source = input(Some("FREQ=DAILY;COUNT=3"));
        source.all_day = true;
        source.start = "2024-03-09T00:00:00Z";
        source.end = "2024-03-10T00:00:00Z";
        source.home_zone = "Unresolved/AllDayZone";
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(
            &[template],
            &window("2024-03-10", "2024-03-10", "Pacific/Honolulu"),
        )
        .unwrap()
        .remove(0);
        assert_eq!(occurrences.len(), 2);
        assert_eq!(occurrences[0].start_date.to_string(), "2024-03-09");
        assert_eq!(occurrences[1].start_date.to_string(), "2024-03-10");
    }

    #[test]
    fn cancellation_from_boundary_suppresses_rule_and_additional_dates() {
        let rdates = vec!["2024-03-20".into()];
        let mut source = input(Some("FREQ=DAILY;COUNT=3"));
        source.rdates = &rdates;
        source.overrides.push(StoredOverride {
            recurrence_id: "2024-03-10".into(),
            start: None,
            end: None,
            cancelled: true,
            this_and_future: true,
        });
        let template = Template::from_stored(source).unwrap();
        let occurrences = expand_templates(&[template], &window("2024-03-09", "2024-03-20", "UTC"))
            .unwrap()
            .remove(0);
        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].id, "series");
    }
}
