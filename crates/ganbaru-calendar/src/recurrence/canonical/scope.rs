//! Pure scope selection over complete native recurrence and execution evidence.

use serde::{Deserialize, Serialize};

use super::*;

const SEARCH_CHUNK_DAYS: i64 = 366;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditScope {
    This,
    Following,
    All,
}

/// Only the database adapter constructs execution evidence. Dates are identities,
/// never dates projected into the current window's display timezone.
#[derive(Default)]
pub(crate) struct ScopeEvidence {
    pub history_dates: BTreeSet<NaiveDate>,
    pub active: Option<(String, NaiveDate)>,
}

/// One native instant plus the device civil date needed by floating all-day events.
#[derive(Clone, Copy)]
pub(crate) struct ScopeClock {
    pub epoch_ms: i64,
    pub floating_today: Option<NaiveDate>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopeOccurrence {
    pub id: String,
    pub recurrence_date: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl From<Occurrence> for ScopeOccurrence {
    fn from(value: Occurrence) -> Self {
        Self {
            id: value.id,
            recurrence_date: value.recurrence_date.to_string(),
            start_ms: value.start_ms,
            end_ms: value.end_ms,
        }
    }
}

/// Native scope decisions reused by the draft planner and transactional apply.
/// This is not a commit authorization or a substitute for rechecking the source.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScopePlan {
    pub effective_scope: EditScope,
    pub selected: ScopeOccurrence,
    pub selected_started: bool,
    pub selected_has_history: bool,
    pub selected_active: bool,
    pub active_run_id: Option<String>,
    pub active_recurrence_date: Option<String>,
    pub protected_through: Option<String>,
    pub first_mutable: Option<ScopeOccurrence>,
    /// Retain these exact protected occurrences before replacing the mutable side.
    pub preserve: Vec<ScopeOccurrence>,
    /// Source recurrence fields only. Draft application and canonical metadata
    /// copying still belong to the semantic edit planner and transaction.
    pub partition: Option<SeriesPartition>,
}

impl Template {
    /// Enumerate original identities without filtering moved overrides by visible dates.
    pub(super) fn identity_range(
        &self,
        lower: NaiveDate,
        upper: NaiveDate,
        budget: &mut ExpansionBudget,
    ) -> Result<(Vec<Occurrence>, bool), String> {
        let mut seeds = BTreeMap::new();
        let rule_finished = if let Some(rule) = &self.rule {
            let generated = super::super::engine::generate_with_completion(
                rule,
                self.start,
                self.start_ms,
                lower,
                upper,
                budget,
                |value| self.generated_instant(value),
            )?;
            for seed in generated.seeds {
                seeds.insert(seed.date, seed);
            }
            generated.exhausted
                || (self.start.date() <= upper
                    && match rule.until {
                        Some(rule::Until::Date(date)) => date <= upper,
                        Some(rule::Until::Local(value)) => value.date() <= upper,
                        Some(rule::Until::Instant(value)) => {
                            civil_time::instant_to_local(value, &self.home_zone)?.date() <= upper
                        }
                        None => false,
                    })
        } else {
            if (lower..=upper).contains(&self.start.date()) {
                budget.occurrence()?;
                seeds.insert(
                    self.start.date(),
                    Seed {
                        date: self.start.date(),
                        instant_ms: self.start_ms,
                    },
                );
            }
            self.start.date() <= upper
        };
        for (&date, &seed) in self.additions.range(lower..=upper) {
            if let std::collections::btree_map::Entry::Vacant(entry) = seeds.entry(date) {
                budget.occurrence()?;
                entry.insert(seed);
            }
        }
        let exhausted = (rule_finished
            && self
                .additions
                .last_key_value()
                .is_none_or(|(&date, _)| date <= upper))
            || self.cancel_from.is_some_and(|date| date <= upper);
        let occurrences = seeds
            .into_values()
            .filter_map(|seed| self.occurrence(seed).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        Ok((occurrences, exhausted))
    }

    /// Plan protection from a native clock and complete persisted history. All
    /// exact lookups, historical expansion, and future search share one budget.
    #[cfg(test)]
    pub(crate) fn plan_scope(
        &self,
        selected_date: NaiveDate,
        requested: EditScope,
        evidence: ScopeEvidence,
        clock: ScopeClock,
    ) -> Result<ScopePlan, String> {
        self.plan_scope_with_budget(
            selected_date,
            requested,
            evidence,
            clock,
            &mut ExpansionBudget::default(),
        )
    }

    pub(super) fn plan_scope_with_budget(
        &self,
        selected_date: NaiveDate,
        requested: EditScope,
        evidence: ScopeEvidence,
        clock: ScopeClock,
        budget: &mut ExpansionBudget,
    ) -> Result<ScopePlan, String> {
        let selected = self
            .resolve_identity_with_budget(selected_date, budget)?
            .ok_or("Selected Calendar occurrence no longer exists")?;
        let selected_active = evidence
            .active
            .as_ref()
            .is_some_and(|(_, date)| *date == selected_date);
        let scope = if selected_active || (self.rule.is_none() && self.additions.is_empty()) {
            EditScope::This
        } else {
            requested
        };
        let mut protected = BTreeMap::new();
        let mut earliest_mutable = None;
        let today = if self.all_day {
            clock
                .floating_today
                .ok_or("Floating Calendar scope requires the native device date")?
        } else {
            civil_time::instant_to_local(clock.epoch_ms, &self.home_zone)?.date()
        };
        let first = self
            .additions
            .first_key_value()
            .map_or(self.start.date(), |(&date, _)| date.min(self.start.date()));
        // This-only edits do not need to enumerate unrelated history or future dates.
        let history_start = if scope == EditScope::Following {
            first.max(selected_date)
        } else {
            first
        };
        if scope != EditScope::This && history_start <= today {
            for occurrence in self.identity_range(history_start, today, budget)?.0 {
                if self.started(&occurrence, &clock, today) {
                    protected.insert(occurrence.recurrence_date, occurrence);
                } else if scope == EditScope::All
                    && earliest_mutable.is_none()
                    && !evidence.history_dates.contains(&occurrence.recurrence_date)
                    && !evidence
                        .active
                        .as_ref()
                        .is_some_and(|(_, date)| *date == occurrence.recurrence_date)
                {
                    earliest_mutable = Some(occurrence);
                }
            }
        }
        if scope != EditScope::This {
            // Overrides may move future original identities into the past. Durable
            // history can also exist beyond the clock after a clock correction.
            let dates: BTreeSet<_> = self
                .overrides
                .keys()
                .copied()
                .chain(evidence.history_dates.iter().copied())
                .chain(evidence.active.as_ref().map(|(_, date)| *date))
                .collect();
            for date in dates {
                if scope == EditScope::Following && date < selected_date {
                    continue;
                }
                if protected.contains_key(&date) {
                    continue;
                }
                let occurrence = self.resolve_identity_with_budget(date, budget)?;
                if let Some(occurrence) = occurrence {
                    if self.started(&occurrence, &clock, today)
                        || evidence.history_dates.contains(&date)
                        || evidence
                            .active
                            .as_ref()
                            .is_some_and(|(_, active)| *active == date)
                    {
                        protected.insert(date, occurrence);
                    }
                } else if evidence
                    .active
                    .as_ref()
                    .is_some_and(|(_, active)| *active == date)
                {
                    return Err(
                        "Active Focus occurrence is missing from its Calendar source".into(),
                    );
                }
            }
        }
        let first_mutable = if scope == EditScope::All {
            if earliest_mutable.is_some() {
                earliest_mutable
            } else {
                let lower = if first > today {
                    first
                } else {
                    today
                        .succ_opt()
                        .ok_or("Calendar clock exceeds its date range")?
                };
                self.first_mutable_from(lower, &evidence, &clock, today, budget)?
            }
        } else {
            None
        };
        let replacement_date = match scope {
            EditScope::This => None,
            EditScope::Following => Some(selected_date),
            EditScope::All => first_mutable.as_ref().map(|value| value.recurrence_date),
        };
        let protected_through = match replacement_date {
            Some(date) => protected.range(..date).next_back().map(|(&date, _)| date),
            None => protected.last_key_value().map(|(&date, _)| date),
        };
        let preserve = if let Some(replacement_date) = replacement_date {
            protected
                .into_iter()
                .filter(|(date, _)| *date >= replacement_date)
                .map(|(_, value)| value.into())
                .collect()
        } else {
            Vec::new()
        };
        let partition = replacement_date
            .map(|date| self.partition_at_with_budget(date, budget))
            .transpose()?;
        Ok(ScopePlan {
            effective_scope: scope,
            selected_started: self.started(&selected, &clock, today),
            selected_has_history: evidence.history_dates.contains(&selected_date),
            selected_active,
            selected: selected.into(),
            active_recurrence_date: evidence.active.as_ref().map(|(_, date)| date.to_string()),
            active_run_id: evidence.active.map(|(id, _)| id),
            protected_through: protected_through.map(|date| date.to_string()),
            first_mutable: first_mutable.map(Into::into),
            preserve,
            partition,
        })
    }

    pub(super) fn started(
        &self,
        occurrence: &Occurrence,
        clock: &ScopeClock,
        today: NaiveDate,
    ) -> bool {
        if self.all_day {
            occurrence.start_date <= today
        } else {
            occurrence.start_ms <= clock.epoch_ms
        }
    }

    pub(super) fn first_mutable_from(
        &self,
        mut lower: NaiveDate,
        evidence: &ScopeEvidence,
        clock: &ScopeClock,
        today: NaiveDate,
        budget: &mut ExpansionBudget,
    ) -> Result<Option<Occurrence>, String> {
        let last_date =
            NaiveDate::from_ymd_opt(9999, 12, 31).ok_or("invalid Calendar maximum date")?;
        // A finite source can prove exhaustion. An unlimited or extremely sparse
        // source instead stops at the shared work limit, never at an arbitrary horizon.
        loop {
            let upper = lower
                .checked_add_signed(Duration::days(SEARCH_CHUNK_DAYS - 1))
                .unwrap_or(last_date)
                .min(last_date);
            let (occurrences, exhausted) = self.identity_range(lower, upper, budget)?;
            for occurrence in occurrences {
                let date = occurrence.recurrence_date;
                if !self.started(&occurrence, clock, today)
                    && !evidence.history_dates.contains(&date)
                    && !evidence
                        .active
                        .as_ref()
                        .is_some_and(|(_, active)| *active == date)
                {
                    return Ok(Some(occurrence));
                }
            }
            if exhausted || upper == last_date {
                return Ok(None);
            }
            lower = upper
                .succ_opt()
                .ok_or("Calendar search exceeds its date range")?;
        }
    }
}
