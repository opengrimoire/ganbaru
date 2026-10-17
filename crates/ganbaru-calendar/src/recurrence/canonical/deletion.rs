//! Semantic deletion scopes over canonical identities and complete native evidence.

use super::scope::ScopeOccurrence;
use super::*;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeleteOutcome {
    Delete,
    Archive,
    Mixed,
}

/// Geometry only. The Calendar adapter owns complete metadata, archive snapshots,
/// Focus stopping, undo preimages, review validation and the actual transaction.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeletePlan {
    pub effective_scope: EditScope,
    pub selected: ScopeOccurrence,
    pub selected_started: bool,
    pub selected_has_history: bool,
    pub history_only: bool,
    pub outcome: DeleteOutcome,
    pub archive_occurrences: Vec<ScopeOccurrence>,
    /// A durable source reference requires its complete master archive snapshot,
    /// even when a capped historical source remains live.
    pub archive_source: bool,
    /// None removes the live master. A retained side preserves exact recurrence
    /// phase, COUNT accounting, RDATE, exclusions and override identities.
    pub source_after: Option<PartitionSide>,
    pub active_run_to_stop: Option<String>,
    pub valid_until_ms: Option<i64>,
}

impl Template {
    /// Started selections are history-only. Future selections remove their
    /// mutable side and archive protected identities, without a viewport horizon.
    pub(crate) fn plan_delete(
        &self,
        selected_date: NaiveDate,
        requested: EditScope,
        evidence: ScopeEvidence,
        clock: ScopeClock,
        source_has_reference: bool,
    ) -> Result<DeletePlan, String> {
        self.plan_delete_with_budget(
            selected_date,
            requested,
            evidence,
            clock,
            source_has_reference,
            &mut ExpansionBudget::default(),
        )
    }

    fn plan_delete_with_budget(
        &self,
        selected_date: NaiveDate,
        requested: EditScope,
        evidence: ScopeEvidence,
        clock: ScopeClock,
        source_has_reference: bool,
        budget: &mut ExpansionBudget,
    ) -> Result<DeletePlan, String> {
        let selected = self
            .resolve_identity_with_budget(selected_date, budget)?
            .ok_or("Selected Calendar occurrence no longer exists")?;
        let today = if self.all_day {
            clock
                .floating_today
                .ok_or("Floating Calendar deletion requires the native device date")?
        } else {
            civil_time::instant_to_local(clock.epoch_ms, &self.home_zone)?.date()
        };
        let selected_started = self.started(&selected, &clock, today);
        let recurring = self.rule.is_some() || !self.additions.is_empty();
        let selected_has_history = evidence.history_dates.contains(&selected_date)
            || (!recurring && !evidence.history_dates.is_empty());
        let selected_active = evidence
            .active
            .as_ref()
            .is_some_and(|(_, date)| *date == selected_date);
        if !recurring
            && evidence
                .active
                .as_ref()
                .is_some_and(|(_, date)| *date != selected_date)
        {
            return Err("Active Focus occurrence is missing from its Calendar source".into());
        }
        let scope = if selected_active || !recurring {
            EditScope::This
        } else {
            requested
        };
        if scope == EditScope::This {
            let archive =
                selected_started || selected_has_history || selected_active || source_has_reference;
            let valid_until_ms = (!selected_started && !self.all_day).then_some(selected.start_ms);
            let mut after = recurring.then(|| self.whole_side()).transpose()?;
            if let Some(side) = &mut after {
                exclude(side, selected_date);
            }
            return Ok(DeletePlan {
                effective_scope: scope,
                selected: selected.clone().into(),
                selected_started,
                selected_has_history,
                history_only: selected_started,
                outcome: if archive {
                    DeleteOutcome::Archive
                } else {
                    DeleteOutcome::Delete
                },
                archive_occurrences: if recurring && archive {
                    vec![selected.into()]
                } else {
                    Vec::new()
                },
                archive_source: !recurring && archive,
                source_after: after,
                active_run_to_stop: if selected_active {
                    evidence.active.map(|(id, _)| id)
                } else {
                    None
                },
                valid_until_ms,
            });
        }
        let first = self
            .additions
            .first_key_value()
            .map_or(self.start.date(), |(&date, _)| date.min(self.start.date()));
        let lower = if scope == EditScope::Following {
            first.max(selected_date)
        } else {
            first
        };
        let mut candidates = BTreeMap::from([(selected_date, selected.clone())]);
        if lower <= today {
            for occurrence in self.identity_range(lower, today, budget)?.0 {
                candidates.insert(occurrence.recurrence_date, occurrence);
            }
        }
        let explicit_dates: BTreeSet<_> = self
            .overrides
            .keys()
            .copied()
            .chain(self.additions.keys().copied())
            .chain(evidence.history_dates.iter().copied())
            .chain(evidence.active.as_ref().map(|(_, date)| *date))
            .filter(|date| *date >= lower)
            .collect();
        for date in explicit_dates {
            let occurrence = self.resolve_identity_with_budget(date, budget)?;
            let active = evidence
                .active
                .as_ref()
                .is_some_and(|(_, value)| *value == date);
            let Some(occurrence) = occurrence else {
                if active {
                    return Err(
                        "Active Focus occurrence is missing from its Calendar source".into(),
                    );
                }
                continue;
            };
            candidates.insert(date, occurrence);
        }
        let started = |row: &Occurrence| self.started(row, &clock, today);
        let protected = |row: &Occurrence| {
            started(row)
                || evidence.history_dates.contains(&row.recurrence_date)
                || evidence
                    .active
                    .as_ref()
                    .is_some_and(|(_, date)| *date == row.recurrence_date)
        };
        let last_started = candidates
            .values()
            .filter(|row| started(row))
            .map(|row| row.recurrence_date)
            .max();
        // A distant override can move a future original identity into the past.
        // Inspect every original identity through it before retaining that prefix.
        if scope == EditScope::All
            && !selected_started
            && let Some(last) = last_started.filter(|date| *date > today)
        {
            let lower = today
                .succ_opt()
                .ok_or("Calendar clock exceeds its date range")?;
            for occurrence in self.identity_range(lower, last, budget)?.0 {
                candidates.insert(occurrence.recurrence_date, occurrence);
            }
        }
        let mut archived: BTreeMap<NaiveDate, Occurrence> = BTreeMap::new();
        let mut next_start = None;
        let mut deleting = false;
        let archive_source = !selected_started && source_has_reference;
        let mut after = if selected_started {
            archived.extend(
                candidates
                    .iter()
                    .filter(|(_, row)| started(row))
                    .map(|(date, row)| (*date, row.clone())),
            );
            Some(self.whole_side()?)
        } else if scope == EditScope::Following {
            archived.extend(
                candidates
                    .iter()
                    .filter(|(_, row)| protected(row))
                    .map(|(date, row)| (*date, row.clone())),
            );
            deleting = !protected(&selected);
            next_start = Some(selected.start_ms);
            Some(self.partition_at_with_budget(selected_date, budget)?.before)
        } else {
            archived.extend(
                candidates
                    .iter()
                    .filter(|(_, row)| !started(row) && protected(row))
                    .map(|(date, row)| (*date, row.clone())),
            );
            deleting = candidates.values().any(|row| !protected(row));
            if let Some(last) = last_started {
                let lower = last
                    .succ_opt()
                    .ok_or("Calendar deletion exceeds its date range")?;
                let boundary = self.first_mutable_from(
                    lower,
                    &ScopeEvidence::default(),
                    &clock,
                    today,
                    budget,
                )?;
                let mut prefix = if let Some(boundary) = boundary {
                    next_start = Some(boundary.start_ms);
                    deleting |= !protected(&boundary);
                    self.partition_at_with_budget(boundary.recurrence_date, budget)?
                        .before
                } else {
                    self.whole_side()?
                };
                for row in candidates.values().filter(|row| !started(row)) {
                    exclude(&mut prefix, row.recurrence_date);
                }
                Some(prefix)
            } else {
                None
            }
        };
        // Later untracked candidates may follow a selected history-bearing member.
        if !selected_started && !deleting && !archive_source {
            let lower = if scope == EditScope::Following {
                selected_date
            } else {
                first
            };
            deleting = self
                .first_mutable_from(lower, &evidence, &clock, today, budget)?
                .is_some();
        }
        if selected_started && !self.all_day {
            // A history-only review changes when another scoped member starts.
            // Historical future members also matter, so use empty protection
            // evidence when finding the next clock transition.
            next_start = self
                .first_mutable_from(
                    lower.max(today),
                    &ScopeEvidence::default(),
                    &clock,
                    today,
                    budget,
                )?
                .map(|row| row.start_ms);
        }
        if let Some(side) = &mut after {
            for date in archived.keys() {
                exclude(side, *date);
            }
        }
        let explicit_deadline = candidates
            .values()
            .filter(|row| !started(row))
            .map(|row| row.start_ms)
            .min();
        let active_run_to_stop = evidence
            .active
            .and_then(|(id, date)| archived.contains_key(&date).then_some(id));
        deleting &= !archive_source;
        let has_archive = archive_source || !archived.is_empty();
        let outcome = match (deleting, has_archive) {
            (true, true) => DeleteOutcome::Mixed,
            (true, false) => DeleteOutcome::Delete,
            _ => DeleteOutcome::Archive,
        };
        Ok(DeletePlan {
            effective_scope: scope,
            selected: selected.into(),
            selected_started,
            selected_has_history,
            history_only: selected_started,
            outcome,
            archive_occurrences: archived.into_values().map(Into::into).collect(),
            archive_source,
            source_after: after,
            active_run_to_stop,
            valid_until_ms: if self.all_day {
                None
            } else {
                next_start
                    .into_iter()
                    .chain(explicit_deadline)
                    .filter(|value| *value > clock.epoch_ms)
                    .min()
            },
        })
    }
}

fn exclude(side: &mut PartitionSide, date: NaiveDate) {
    let date = date.to_string();
    if !side.exceptions.contains(&date) {
        side.exceptions.push(date);
        side.exceptions.sort();
    }
}

#[cfg(test)]
mod tests;
