//! Build concrete source and edited sides for the selected semantic scope.

use super::*;

impl Template {
    pub(super) fn plan_single(
        &self,
        scope: &ScopePlan,
        resolved: &ResolvedTiming,
        draft: &GeometryDraft<'_>,
        recurring: bool,
        in_place: bool,
    ) -> Result<(EditKind, Option<PlannedSeries>, PlannedSeries), String> {
        let this = scope.effective_scope == EditScope::This;
        let before = if !recurring || in_place {
            None
        } else if this {
            let mut side = self.whole_side()?;
            side.exceptions.push(scope.selected.recurrence_date.clone());
            Some(self.planned_source(side, draft.source_zone)?)
        } else {
            let partition = scope
                .partition
                .as_ref()
                .ok_or("Calendar edit has no mutable occurrences")?;
            Some(self.planned_source(partition.before.clone(), draft.source_zone)?)
        };
        let rule = match &*draft.recurrence {
            RecurrenceIntent::Set(rule) => Some(rule.clone()),
            _ => None,
        };
        let edited = PlannedSeries {
            fields: PartitionSide {
                start_time: resolved.start_time.clone(),
                end_time: resolved.end_time.clone(),
                rrule: rule,
                exceptions: Vec::new(),
                rdates: Vec::new(),
                override_recurrence_ids: Vec::new(),
            },
            timezone: resolved.timezone.clone(),
            all_day: resolved.all_day,
            overrides: Vec::new(),
            metadata_occurrence_date: Some(scope.selected.recurrence_date.clone()),
        };
        Ok((
            if in_place {
                EditKind::Update
            } else if this {
                EditKind::Detach
            } else {
                EditKind::Split
            },
            before,
            edited,
        ))
    }

    pub(super) fn plan_series(
        &self,
        scope: &ScopePlan,
        resolved: &ResolvedTiming,
        draft: &GeometryDraft<'_>,
        in_place: bool,
        materialize: &[ScopeOccurrence],
    ) -> Result<(EditKind, Option<PlannedSeries>, PlannedSeries), String> {
        let (before, mut side) = if in_place {
            (None, self.whole_side()?)
        } else {
            let partition = scope
                .partition
                .as_ref()
                .ok_or("Calendar edit has no mutable occurrences")?;
            (
                Some(self.planned_source(partition.before.clone(), draft.source_zone)?),
                partition.following.clone(),
            )
        };
        let selected_start =
            civil_time::instant_to_local(scope.selected.start_ms, &self.home_zone)?;
        let resolved_zone = if resolved.all_day {
            TimeZone::UTC
        } else {
            civil_time::zone(&resolved.timezone)?
        };
        let desired_start = stored_time(&resolved.start_time, &resolved_zone, resolved.all_day)?.0;
        let reanchor = !in_place
            && (matches!(draft.recurrence, RecurrenceIntent::Set(_))
                || selected_start.date() != desired_start.date()
                || resolved.timezone != draft.source_zone);
        if let RecurrenceIntent::Set(rule) = &*draft.recurrence {
            side.rrule = Some(rule.clone());
        } else if reanchor {
            let partition = scope
                .partition
                .as_ref()
                .ok_or("Calendar edit has no source partition")?;
            if let Some(mut rule) = self.rule.clone() {
                if let Some(count) = rule.count {
                    let consumed = u32::try_from(partition.generated_before)
                        .map_err(|_| "Calendar prefix count overflow")?;
                    // An off-pattern additional date becomes the explicit
                    // DTSTART. It consumes one new COUNT slot of its own.
                    rule.count = Some(
                        count
                            .checked_sub(consumed)
                            .and_then(|remaining| {
                                remaining.checked_add(u32::from(!partition.boundary_generated))
                            })
                            .filter(|remaining| *remaining > 0)
                            .ok_or("Calendar edit exhausted its generated remainder")?,
                    );
                }
                side.rrule = Some(rule.encode()?);
            }
        }
        let mut reanchored_identity = None;
        if reanchor {
            let anchor = if scope.effective_scope == EditScope::Following {
                &scope.selected
            } else {
                scope
                    .first_mutable
                    .as_ref()
                    .ok_or("Calendar edit has no mutable occurrences")?
            };
            side.start_time = self.occurrence_endpoint(anchor.start_ms)?;
            side.end_time = self.occurrence_endpoint(anchor.end_ms)?;
            // Prefix exclusions created solely to retain the old cadence
            // cease to apply after choosing a new DTSTART. Keep real EXDATEs.
            let boundary = parse_date(&anchor.recurrence_date)?;
            reanchored_identity = Some(anchor.recurrence_date.clone());
            let mut additions = Vec::with_capacity(side.rdates.len());
            for value in side.rdates {
                let date = if value.len() == 10 {
                    parse_date(&value)?
                } else {
                    stored_time(&value, &self.home_zone, self.all_day)?.0.date()
                };
                if date != boundary {
                    additions.push(value);
                }
            }
            side.rdates = additions;
            side.exceptions = self
                .exclusions
                .range(boundary..)
                .map(ToString::to_string)
                .collect();
        }
        let protected: BTreeSet<_> = materialize
            .iter()
            .map(|value| value.recurrence_date.clone())
            .collect();
        side.exceptions.extend(protected.iter().cloned());
        let protected_overrides: BTreeSet<_> = self
            .overrides
            .iter()
            .filter(|(date, _)| protected.contains(&date.to_string()))
            .map(|(_, (_, row))| row.recurrence_id.as_str())
            .collect();
        side.override_recurrence_ids
            .retain(|id| !protected_overrides.contains(id.as_str()));
        if resolved.all_day
            && !self.all_day
            && matches!(draft.recurrence, RecurrenceIntent::Unchanged)
        {
            if let Some(encoded) = &mut side.rrule {
                let mut rule = rule::parse(encoded)?;
                let last_date = match rule.until {
                    Some(rule::Until::Instant(epoch_ms)) => {
                        let date = civil_time::instant_to_local(epoch_ms, &self.home_zone)?.date();
                        // During a fold, a later instant can have an earlier
                        // civil clock. Compare the generated instant, not clocks.
                        let admitted = self
                            .generated_instant(date.and_time(self.start.time()))?
                            .is_some_and(|candidate| candidate <= epoch_ms);
                        Some(if admitted {
                            date
                        } else {
                            date.pred_opt()
                                .ok_or("Calendar termination exceeds its date range")?
                        })
                    }
                    Some(rule::Until::Local(cutoff)) => {
                        Some(if cutoff.time() < self.start.time() {
                            cutoff
                                .date()
                                .pred_opt()
                                .ok_or("Calendar termination exceeds its date range")?
                        } else {
                            cutoff.date()
                        })
                    }
                    _ => None,
                };
                if let Some(last_date) = last_date {
                    rule.until = Some(rule::Until::Date(last_date));
                    *encoded = rule.encode()?;
                }
            }
        }
        let mut edited = self.apply_timing(
            side,
            scope,
            resolved,
            draft.timing,
            resolved.timezone != draft.source_zone,
        )?;
        if let Some(original_identity) = reanchored_identity {
            let anchor_date =
                stored_time(&edited.fields.start_time, &resolved_zone, resolved.all_day)?
                    .0
                    .date()
                    .to_string();
            for row in &mut edited.overrides {
                if row.recurrence_id == original_identity {
                    row.recurrence_id = anchor_date.clone();
                    if draft.timing.start_time.is_some() || draft.timing.end_time.is_some() {
                        row.start_time = Some(edited.fields.start_time.clone());
                        row.end_time = Some(edited.fields.end_time.clone());
                    }
                }
            }
        }
        Ok((
            if in_place {
                EditKind::Update
            } else {
                EditKind::Split
            },
            before,
            edited,
        ))
    }
}
