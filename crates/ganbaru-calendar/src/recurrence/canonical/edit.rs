//! Normalize user timing and recurrence intent against a native scope decision.

use chrono::{Datelike, SecondsFormat};
use serde::{Deserialize, Serialize};

use super::*;

/// Omitted endpoints retain the selected occurrence's canonical geometry.
/// Converting between timed and floating dates requires both explicit endpoints.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TimingIntent {
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub timezone: Option<String>,
    /// Zone of edited civil labels, independent of the event's retained home zone.
    pub input_zone: Option<String>,
    pub all_day: Option<bool>,
}

/// Clearing recurrence is explicit. An omitted operation cannot clear a series.
#[derive(Clone, Default, Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum RecurrenceIntent {
    #[default]
    Unchanged,
    Clear,
    Set(String),
}

impl RecurrenceIntent {
    /// New sources have no inherited recurrence; validate and canonicalize a rule.
    pub(crate) fn for_creation(&self) -> Result<Option<String>, String> {
        match self {
            Self::Set(value) => Ok(Some(rule::parse(value)?.encode()?)),
            Self::Unchanged | Self::Clear => Ok(None),
        }
    }
}

/// Selected-occurrence geometry after normalization, before scope application.
/// This does not replace a series anchor or authorize a write to that anchor.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedTiming {
    pub start_time: String,
    pub end_time: String,
    pub timezone: String,
    pub all_day: bool,
    pub start_ms: i64,
    pub end_ms: i64,
}

fn endpoint(value: &str, zone: &TimeZone, all_day: bool) -> Result<(NaiveDateTime, i64), String> {
    if all_day && value.len() != 10 {
        return Err("Floating Calendar draft endpoints require YYYY-MM-DD dates".into());
    }
    let result = stored_time(value, zone, all_day)?;
    if !(1..=9999).contains(&result.0.year()) {
        return Err("Calendar draft date is outside years 1 through 9999".into());
    }
    if all_day && result.0.date().to_string() != value {
        return Err("Floating Calendar draft endpoint is not a canonical date".into());
    }
    Ok(result)
}

fn format_endpoint(
    civil: NaiveDateTime,
    epoch_ms: i64,
    zone: &TimeZone,
    all_day: bool,
) -> Result<String, String> {
    if all_day {
        return Ok(civil.date().to_string());
    }
    if civil_time::instant_to_local(epoch_ms, zone)? != civil {
        // A civil input in a gap must retain its wall-clock intent for subsequent
        // generated occurrences, instead of silently changing the daily time.
        return Ok(civil.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
    }
    DateTime::from_timestamp_millis(epoch_ms)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| "Calendar draft endpoint exceeds its instant range".into())
}

fn resolve_endpoint(
    value: &str,
    input_zone: &TimeZone,
    home_zone: &TimeZone,
    all_day: bool,
) -> Result<(NaiveDateTime, i64), String> {
    let (civil, instant) = endpoint(value, input_zone, all_day)?;
    if all_day || input_zone == home_zone {
        Ok((civil, instant))
    } else {
        Ok((civil_time::instant_to_local(instant, home_zone)?, instant))
    }
}

impl TimingIntent {
    /// Creation requires both authored endpoints and an explicit home zone.
    /// Use the same gap, fold and input-zone rules as selected-occurrence edits.
    pub(crate) fn resolve_creation(&self) -> Result<ResolvedTiming, String> {
        let zone_name = self
            .timezone
            .as_deref()
            .ok_or("Calendar creation requires a home zone")?;
        let home_zone = civil_time::zone(zone_name)?;
        let all_day = self.all_day.unwrap_or(false);
        let zone = if all_day { TimeZone::UTC } else { home_zone };
        let input_zone = if all_day {
            TimeZone::UTC
        } else {
            self.input_zone
                .as_deref()
                .map(civil_time::zone)
                .transpose()?
                .unwrap_or_else(|| zone.clone())
        };
        let (start, start_ms) = resolve_endpoint(
            self.start_time
                .as_deref()
                .ok_or("Calendar creation requires a start endpoint")?,
            &input_zone,
            &zone,
            all_day,
        )?;
        let (end, end_ms) = resolve_endpoint(
            self.end_time
                .as_deref()
                .ok_or("Calendar creation requires an end endpoint")?,
            &input_zone,
            &zone,
            all_day,
        )?;
        if (all_day && end < start) || (!all_day && end_ms <= start_ms) {
            return Err(
                "Calendar creation requires a positive timed interval or ordered floating dates"
                    .into(),
            );
        }
        Ok(ResolvedTiming {
            start_time: format_endpoint(start, start_ms, &zone, all_day)?,
            end_time: format_endpoint(end, end_ms, &zone, all_day)?,
            timezone: zone_name.into(),
            all_day,
            start_ms,
            end_ms,
        })
    }
}

impl Template {
    /// Validate one selected-occurrence draft using native execution protection.
    /// The caller must obtain `scope` from this exact source and clock snapshot.
    pub(crate) fn prepare_timing(
        &self,
        scope: &ScopePlan,
        timing: &TimingIntent,
        recurrence: &mut RecurrenceIntent,
        source_zone: &str,
        clock: ScopeClock,
        metadata_changed: bool,
    ) -> Result<ResolvedTiming, String> {
        if let RecurrenceIntent::Set(value) = recurrence {
            let parsed = rule::parse(value)?;
            if self.rule.as_ref() == Some(&parsed) {
                *recurrence = RecurrenceIntent::Unchanged;
            } else {
                *value = parsed.encode()?;
            }
        } else if matches!(recurrence, RecurrenceIntent::Clear)
            && self.rule.is_none()
            && self.additions.is_empty()
        {
            *recurrence = RecurrenceIntent::Unchanged;
        }
        let all_day = timing.all_day.unwrap_or(self.all_day);
        if all_day != self.all_day && (timing.start_time.is_none() || timing.end_time.is_none()) {
            return Err(
                "Changing Calendar date kind requires explicit start and end endpoints".into(),
            );
        }
        let zone_name = timing.timezone.as_deref().unwrap_or(source_zone);
        if let Some(zone) = &timing.timezone {
            civil_time::zone(zone)?;
        }
        let zone = if all_day {
            TimeZone::UTC
        } else {
            civil_time::zone(zone_name)?
        };
        let input_zone = if all_day {
            TimeZone::UTC
        } else {
            timing
                .input_zone
                .as_deref()
                .map(civil_time::zone)
                .transpose()?
                .unwrap_or_else(|| zone.clone())
        };
        let resolve_endpoint = |value: &str| -> Result<(NaiveDateTime, i64), String> {
            // Keep authored gap intent when input and home clocks are the same.
            // Otherwise the explicit edit denotes an instant in the input zone.
            resolve_endpoint(value, &input_zone, &zone, all_day)
        };
        let original_start = civil_time::instant_to_local(scope.selected.start_ms, &zone)?;
        let original_end = civil_time::instant_to_local(scope.selected.end_ms, &zone)?;
        let (start, start_ms) = timing
            .start_time
            .as_deref()
            .map(resolve_endpoint)
            .transpose()?
            .unwrap_or((original_start, scope.selected.start_ms));
        let (end, end_ms) = timing
            .end_time
            .as_deref()
            .map(resolve_endpoint)
            .transpose()?
            .unwrap_or((original_end, scope.selected.end_ms));
        if (all_day && end < start) || (!all_day && end_ms <= start_ms) {
            return Err(
                "Calendar draft requires a positive timed interval or ordered floating dates"
                    .into(),
            );
        }
        let changed = metadata_changed
            || start_ms != scope.selected.start_ms
            || end_ms != scope.selected.end_ms
            || all_day != self.all_day
            || zone_name != source_zone
            || !matches!(recurrence, RecurrenceIntent::Unchanged);
        if changed && scope.effective_scope == EditScope::This {
            if scope.selected_has_history
                && !scope.selected_active
                && (self.all_day
                    || !scope.selected_started
                    || scope.selected.end_ms <= clock.epoch_ms)
            {
                return Err(
                    "Recorded Calendar history cannot be rewritten by a scoped edit".into(),
                );
            }
            if scope.selected_started
                && !scope.selected_active
                && (self.all_day || scope.selected.end_ms <= clock.epoch_ms)
            {
                return Err(
                    "Completed Calendar occurrences cannot be rewritten by a scoped edit".into(),
                );
            }
            if scope.selected_started || scope.selected_active {
                if end_ms < clock.epoch_ms {
                    return Err(
                        "An active Calendar occurrence cannot end before the current native time"
                            .into(),
                    );
                }
                if start_ms != scope.selected.start_ms || all_day != self.all_day {
                    return Err("An active Calendar occurrence cannot change its recorded start or date kind".into());
                }
                if (self.rule.is_some() || !self.additions.is_empty())
                    && !matches!(recurrence, RecurrenceIntent::Unchanged)
                {
                    return Err(
                        "An active Calendar occurrence cannot change its recurrence chain".into(),
                    );
                }
            }
        }
        Ok(ResolvedTiming {
            start_time: format_endpoint(start, start_ms, &zone, all_day)?,
            end_time: format_endpoint(end, end_ms, &zone, all_day)?,
            timezone: zone_name.into(),
            all_day,
            start_ms,
            end_ms,
        })
    }
}

#[cfg(test)]
mod tests;
