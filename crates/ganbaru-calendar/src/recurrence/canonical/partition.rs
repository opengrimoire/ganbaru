//! Recurrence-set partitions that preserve pre-exclusion COUNT and period phase.

use chrono::SecondsFormat;
use serde::Serialize;

use super::*;

#[cfg(test)]
mod tests;

/// Canonical recurrence fields for one side of a split, before draft field edits.
/// Override identities select complete original rows, including metadata; these
/// references must never be treated as replacement override payloads.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartitionSide {
    pub start_time: String,
    pub end_time: String,
    pub rrule: Option<String>,
    pub exceptions: Vec<String>,
    pub rdates: Vec<String>,
    pub override_recurrence_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SeriesPartition {
    pub boundary_date: String,
    pub before: PartitionSide,
    pub following: PartitionSide,
    #[serde(skip)]
    pub(super) generated_before: usize,
    #[serde(skip)]
    pub(super) boundary_generated: bool,
}

struct Interval {
    start: NaiveDateTime,
    end: NaiveDateTime,
    start_ms: i64,
    end_ms: i64,
}

fn instant(value: i64) -> Result<String, String> {
    DateTime::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| "Calendar partition instant is outside its supported range".into())
}

impl Template {
    /// Preserve the entire source set when an edit can retain its original row.
    pub(super) fn whole_side(&self) -> Result<PartitionSide, String> {
        self.partition_side(
            NaiveDate::MIN,
            true,
            self.original_interval(),
            self.rule.as_ref().map(rule::Rule::encode).transpose()?,
            self.exclusions.clone(),
        )
    }

    /// Derive both stored recurrence sets from one canonical source. All work
    /// shares the scope request's allowance, including candidates excluded later.
    pub(super) fn partition_at_with_budget(
        &self,
        boundary: NaiveDate,
        budget: &mut ExpansionBudget,
    ) -> Result<SeriesPartition, String> {
        self.resolve_identity_with_budget(boundary, budget)?
            .ok_or("Calendar split boundary is not a live source occurrence")?;
        let prefix = self.rule_prefix(boundary, budget)?;
        let selected_seed = if let Some(rule) = &self.rule {
            generate(
                rule,
                self.start,
                self.start_ms,
                boundary,
                boundary,
                budget,
                |value| self.generated_instant(value),
            )?
            .into_iter()
            .next()
        } else {
            None
        };
        // Generated members retain daily/weekly/monthly/yearly period phase.
        // BYWEEKNO may belong to a different calendar year, and a backwards
        // civil interval can encode elapsed fold duration. Keep their anchor.
        let may_reanchor = self
            .rule
            .as_ref()
            .is_some_and(|rule| rule.by_week_no.is_empty())
            && (self.all_day || self.end > self.start);
        let boundary_generated = selected_seed.is_some();
        let interval = if may_reanchor {
            selected_seed
                .map(|seed| self.reanchored_interval(seed))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let reanchor = interval.is_some();
        let following_interval = interval.unwrap_or_else(|| self.original_interval());
        let before_rule = self.before_rule(prefix.len())?;
        let following_rule = self.following_rule(prefix.len(), reanchor)?;
        let mut before_exceptions: BTreeSet<_> =
            self.exclusions.range(..boundary).copied().collect();
        if self.start.date() >= boundary {
            // DTSTART remains an explicit set member even after removing RRULE.
            before_exceptions.insert(self.start.date());
        }
        let mut following_exceptions: BTreeSet<_> =
            self.exclusions.range(boundary..).copied().collect();
        if !reanchor {
            // Suppress every generated prefix identity, including cancelled and
            // excluded members. Otherwise dropping old overrides revives them.
            following_exceptions.extend(prefix.iter().map(|seed| seed.date));
            if self.start.date() < boundary {
                following_exceptions.insert(self.start.date());
            }
        }
        Ok(SeriesPartition {
            boundary_date: boundary.to_string(),
            generated_before: prefix.len(),
            boundary_generated,
            before: self.partition_side(
                boundary,
                false,
                self.original_interval(),
                before_rule,
                before_exceptions,
            )?,
            following: self.partition_side(
                boundary,
                true,
                following_interval,
                following_rule,
                following_exceptions,
            )?,
        })
    }

    fn original_interval(&self) -> Interval {
        Interval {
            start: self.start,
            end: self.end,
            start_ms: self.start_ms,
            end_ms: self.end_ms,
        }
    }

    fn reanchored_interval(&self, seed: Seed) -> Result<Option<Interval>, String> {
        let geometry = self
            .occurrence_geometry(seed, false)?
            .ok_or("Calendar split boundary is excluded")?;
        let start = seed.date.and_time(self.start.time());
        let end = seed
            .date
            .checked_add_signed(Duration::days(
                (self.end.date() - self.start.date()).num_days(),
            ))
            .ok_or("Calendar split duration exceeds its date range")?
            .and_time(self.end.time());
        // A generated start can be valid while its end falls in a gap. Persisting
        // that shifted civil end as the new anchor changes every later duration.
        if civil_time::instant_to_local(geometry.start_ms, &self.home_zone)? != start
            || civil_time::instant_to_local(geometry.end_ms, &self.home_zone)? != end
        {
            return Ok(None);
        }
        Ok(Some(Interval {
            start,
            end,
            start_ms: geometry.start_ms,
            end_ms: geometry.end_ms,
        }))
    }

    fn stored_endpoint(&self, civil: NaiveDateTime, epoch_ms: i64) -> Result<String, String> {
        if self.all_day {
            return Ok(civil.date().to_string());
        }
        if civil_time::instant_to_local(epoch_ms, &self.home_zone)? == civil {
            return instant(epoch_ms);
        }
        if civil_time::explicit_instant(civil, &self.home_zone)? != epoch_ms {
            return Err("Calendar partition cannot preserve its endpoint's civil intent".into());
        }
        // Offset-free wall-clock anchors may lie in a gap. Preserve their
        // civil components instead of changing the future recurrence's time.
        Ok(civil.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
    }

    fn rule_prefix(
        &self,
        boundary: NaiveDate,
        budget: &mut ExpansionBudget,
    ) -> Result<Vec<Seed>, String> {
        let Some(upper) = boundary
            .pred_opt()
            .filter(|date| *date >= self.start.date())
        else {
            return Ok(Vec::new());
        };
        match &self.rule {
            Some(rule) => generate(
                rule,
                self.start,
                self.start_ms,
                self.start.date(),
                upper,
                budget,
                |value| self.generated_instant(value),
            ),
            None => {
                budget.occurrence()?;
                Ok(vec![Seed {
                    date: self.start.date(),
                    instant_ms: self.start_ms,
                }])
            }
        }
    }

    fn before_rule(&self, prefix_count: usize) -> Result<Option<String>, String> {
        let Some(mut rule) = self.rule.clone().filter(|_| prefix_count > 0) else {
            return Ok(None);
        };
        // COUNT caps the exact generated prefix without introducing an UNTIL
        // rounding error for fractional stored instants or crossing a DST fold.
        rule.count =
            Some(u32::try_from(prefix_count).map_err(|_| "Calendar prefix count overflow")?);
        rule.until = None;
        rule.encode().map(Some)
    }

    fn following_rule(
        &self,
        prefix_count: usize,
        reanchor: bool,
    ) -> Result<Option<String>, String> {
        let Some(mut rule) = self.rule.clone() else {
            return Ok(None);
        };
        if reanchor {
            if let Some(count) = rule.count {
                let consumed =
                    u32::try_from(prefix_count).map_err(|_| "Calendar prefix count overflow")?;
                rule.count = Some(
                    count
                        .checked_sub(consumed)
                        .filter(|remaining| *remaining > 0)
                        .ok_or("Calendar split would exhaust the following COUNT")?,
                );
            }
        }
        rule.encode().map(Some)
    }

    fn partition_side(
        &self,
        boundary: NaiveDate,
        following: bool,
        interval: Interval,
        rrule: Option<String>,
        exceptions: BTreeSet<NaiveDate>,
    ) -> Result<PartitionSide, String> {
        let on_side = |date: NaiveDate| (date >= boundary) == following;
        Ok(PartitionSide {
            start_time: self.stored_endpoint(interval.start, interval.start_ms)?,
            end_time: self.stored_endpoint(interval.end, interval.end_ms)?,
            rrule,
            exceptions: exceptions
                .into_iter()
                .map(|date| date.to_string())
                .collect(),
            rdates: self
                .additions
                .iter()
                .filter(|entry| on_side(*entry.0))
                .map(|(_, seed)| {
                    if self.all_day
                        || civil_time::explicit_instant(
                            seed.date.and_time(self.start.time()),
                            &self.home_zone,
                        )? == seed.instant_ms
                    {
                        // Keep civil intent through an explicit gap. Formatting
                        // its shifted instant alone would change the local time.
                        Ok(seed.date.to_string())
                    } else {
                        // An explicit later fold must retain its chosen instant.
                        instant(seed.instant_ms)
                    }
                })
                .collect::<Result<_, _>>()?,
            override_recurrence_ids: self
                .overrides
                .iter()
                .filter(|entry| on_side(*entry.0))
                .map(|(_, (_, value))| value.recurrence_id.clone())
                .collect(),
        })
    }
}
