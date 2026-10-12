//! Apply temporal intent while preserving recurrence child identities.

use super::*;

impl Template {
    pub(super) fn occurrence_endpoint(&self, epoch_ms: i64) -> Result<String, String> {
        if self.all_day {
            Ok(civil_time::instant_to_local(epoch_ms, &TimeZone::UTC)?
                .date()
                .to_string())
        } else {
            instant(epoch_ms)
        }
    }

    pub(super) fn planned_source(
        &self,
        fields: PartitionSide,
        timezone: &str,
    ) -> Result<PlannedSeries, String> {
        let retained: BTreeSet<_> = fields.override_recurrence_ids.iter().collect();
        let overrides: Vec<_> = self
            .overrides
            .iter()
            .filter(|(_, (_, row))| retained.contains(&row.recurrence_id))
            .map(|(date, (_, row))| PlannedOverride {
                source_recurrence_id: row.recurrence_id.clone(),
                recurrence_id: date.to_string(),
                start_time: row.start.clone(),
                end_time: row.end.clone(),
                cancelled: row.cancelled,
                this_and_future: row.this_and_future,
            })
            .collect();
        if overrides.len() != retained.len() {
            return Err("Calendar plan refers to an unknown source override".into());
        }
        Ok(PlannedSeries {
            fields,
            timezone: timezone.into(),
            all_day: self.all_day,
            overrides,
            metadata_occurrence_date: None,
        })
    }

    pub(super) fn apply_timing(
        &self,
        mut fields: PartitionSide,
        scope: &ScopePlan,
        resolved: &ResolvedTiming,
        timing: &TimingIntent,
        zone_changed: bool,
    ) -> Result<PlannedSeries, String> {
        let zone = if resolved.all_day {
            TimeZone::UTC
        } else {
            civil_time::zone(&resolved.timezone)?
        };
        if timing.start_time.is_some() || timing.end_time.is_some() {
            let (base, _) = stored_time(&fields.start_time, &self.home_zone, self.all_day)?;
            let old = civil_time::instant_to_local(scope.selected.start_ms, &self.home_zone)?;
            let start = stored_time(&resolved.start_time, &zone, resolved.all_day)?.0;
            let end = stored_time(&resolved.end_time, &zone, resolved.all_day)?.0;
            let start_date = shifted_date(base.date(), (start.date() - old.date()).num_days())?;
            let end_date = shifted_date(start_date, (end.date() - start.date()).num_days())?;
            fields.start_time = if start_date == start.date() {
                resolved.start_time.clone()
            } else {
                serialize_endpoint(start_date.and_time(start.time()), &zone, resolved.all_day)?
            };
            fields.end_time = if end_date == end.date() {
                resolved.end_time.clone()
            } else {
                serialize_endpoint(end_date.and_time(end.time()), &zone, resolved.all_day)?
            };
        } else if zone_changed && !self.all_day {
            // Zone-only edits retain explicit instants, including offset-free
            // civil endpoints that would otherwise be reinterpreted in the new zone.
            fields.start_time =
                instant(stored_time(&fields.start_time, &self.home_zone, false)?.1)?;
            fields.end_time = instant(stored_time(&fields.end_time, &self.home_zone, false)?.1)?;
        }
        let (anchor, _) = stored_time(&fields.start_time, &zone, resolved.all_day)?;
        let original_anchor = self.start;
        // EXDATE and RECURRENCE-ID are stable civil identities. RDATEs retain
        // their independent dates; a changed shared clock applies to those dates.
        for value in &mut fields.rdates {
            let old_date = if value.len() == 10 {
                parse_date(value)?
            } else {
                stored_time(value, &self.home_zone, self.all_day)?.0.date()
            };
            if resolved.all_day {
                *value = old_date.to_string();
            } else if self.all_day || zone_changed || anchor.time() != original_anchor.time() {
                *value = serialize_endpoint(old_date.and_time(anchor.time()), &zone, false)?;
            }
        }
        let mut planned = self.planned_source(fields, &resolved.timezone)?;
        planned.all_day = resolved.all_day;
        let start_clock = anchor.time();
        let end_clock = stored_time(&planned.fields.end_time, &zone, resolved.all_day)?
            .0
            .time();
        for row in &mut planned.overrides {
            if self.all_day == resolved.all_day && !zone_changed {
                continue;
            }
            for (value, clock) in [
                (&mut row.start_time, start_clock),
                (&mut row.end_time, end_clock),
            ] {
                if let Some(stored) = value {
                    let (civil, epoch_ms) = stored_time(stored, &self.home_zone, self.all_day)?;
                    *stored = if resolved.all_day {
                        civil.date().to_string()
                    } else if self.all_day {
                        serialize_endpoint(civil.date().and_time(clock), &zone, false)?
                    } else {
                        instant(epoch_ms)?
                    };
                }
            }
        }
        Ok(planned)
    }
}

impl PlannedSeries {
    /// Decode exactly the recurrence fields that the native transaction will store.
    pub(super) fn template(&self, id: &str) -> Result<Template, String> {
        Template::from_stored(StoredTemplate {
            id,
            start: &self.fields.start_time,
            end: &self.fields.end_time,
            home_zone: &self.timezone,
            all_day: self.all_day,
            rrule: self.fields.rrule.as_deref(),
            repeat_until: None,
            exceptions: &self.fields.exceptions,
            rdates: &self.fields.rdates,
            overrides: self
                .overrides
                .iter()
                .map(|row| StoredOverride {
                    recurrence_id: row.recurrence_id.clone(),
                    start: row.start_time.clone(),
                    end: row.end_time.clone(),
                    cancelled: row.cancelled,
                    this_and_future: row.this_and_future,
                })
                .collect(),
        })
    }
}
