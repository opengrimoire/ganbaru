//! Concrete recurrence sets and preservation targets for native scoped edits.

use chrono::{Datelike, SecondsFormat};
use serde::Serialize;

use super::partition::PartitionSide;
use super::scope::ScopeOccurrence;
use super::*;

mod build;
mod timing;

/// Borrowed user intent accompanies native source, clock and execution evidence.
pub(crate) struct GeometryDraft<'a> {
    pub timing: &'a TimingIntent,
    pub recurrence: &'a mut RecurrenceIntent,
    pub source_zone: &'a str,
    pub metadata_changed: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EditKind {
    Unchanged,
    Update,
    Detach,
    Split,
}

/// Copy all original override metadata, replacing only these identity/geometry
/// fields. Cancellation and range semantics are preserved explicitly.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlannedOverride {
    pub source_recurrence_id: String,
    pub recurrence_id: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub cancelled: bool,
    pub this_and_future: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlannedSeries {
    pub fields: PartitionSide,
    pub timezone: String,
    pub all_day: bool,
    pub overrides: Vec<PlannedOverride>,
    /// A standalone/detached result inherits the selected override's metadata.
    /// Otherwise metadata comes from the source template and retained overrides.
    pub metadata_occurrence_date: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum ActiveTarget {
    Edited,
    Preserved { recurrence_date: String },
}

/// Native identity of the selected occurrence after an operation. Overrides can
/// share geometry, so a visible preview must not identify its selection by time.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SelectedTarget {
    Original,
    Edited { recurrence_date: String },
    Preserved { recurrence_date: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlannedActiveTransfer {
    pub run_id: String,
    pub original_recurrence_date: String,
    pub target: ActiveTarget,
}

/// An update retains the source identity. Detach/split allocate a new edited
/// identity; protected materializations get separate identities. Persistence
/// must derive those IDs from the operation receipt and copy complete metadata.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditGeometryPlan {
    pub kind: EditKind,
    pub selected_target: Option<SelectedTarget>,
    pub before: Option<PlannedSeries>,
    pub edited: Option<PlannedSeries>,
    pub materialize: Vec<ScopeOccurrence>,
    pub active_transfer: Option<PlannedActiveTransfer>,
}

fn instant(value: i64) -> Result<String, String> {
    DateTime::from_timestamp_millis(value)
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
        .ok_or_else(|| "Calendar edit instant exceeds its supported range".into())
}

fn shifted_date(date: NaiveDate, days: i64) -> Result<NaiveDate, String> {
    date.checked_add_signed(Duration::days(days))
        .filter(|value| (1..=9999).contains(&value.year()))
        .ok_or_else(|| "Calendar edit exceeds its civil date range".into())
}

fn serialize_endpoint(
    civil: NaiveDateTime,
    zone: &TimeZone,
    all_day: bool,
) -> Result<String, String> {
    if all_day {
        return Ok(civil.date().to_string());
    }
    let epoch_ms = civil_time::explicit_instant(civil, zone)?;
    if civil_time::instant_to_local(epoch_ms, zone)? != civil {
        return Ok(civil.format("%Y-%m-%dT%H:%M:%S%.f").to_string());
    }
    instant(epoch_ms)
}

impl Template {
    /// Scope enumeration and all exact source lookups share one work allowance.
    pub(crate) fn prepare_edit_geometry(
        &self,
        selected: NaiveDate,
        requested: EditScope,
        evidence: ScopeEvidence,
        clock: ScopeClock,
        draft: GeometryDraft<'_>,
    ) -> Result<(ScopePlan, ResolvedTiming, EditGeometryPlan), String> {
        let mut budget = ExpansionBudget::default();
        let scope =
            self.plan_scope_with_budget(selected, requested, evidence, clock, &mut budget)?;
        let resolved = self.prepare_timing(
            &scope,
            draft.timing,
            draft.recurrence,
            draft.source_zone,
            clock,
            draft.metadata_changed,
        )?;
        let changed = draft.metadata_changed
            || resolved.start_ms != scope.selected.start_ms
            || resolved.end_ms != scope.selected.end_ms
            || resolved.all_day != self.all_day
            || resolved.timezone != draft.source_zone
            || !matches!(draft.recurrence, RecurrenceIntent::Unchanged);
        if !changed {
            return Ok((
                scope,
                resolved,
                EditGeometryPlan {
                    kind: EditKind::Unchanged,
                    selected_target: Some(SelectedTarget::Original),
                    before: None,
                    edited: None,
                    materialize: Vec::new(),
                    active_transfer: None,
                },
            ));
        }
        let recurring = self.rule.is_some() || !self.additions.is_empty();
        let this = scope.effective_scope == EditScope::This;
        let collapse = matches!(draft.recurrence, RecurrenceIntent::Clear);
        if collapse && !this && (scope.selected_started || scope.selected_has_history) {
            return Err(
                "Choose a mutable Calendar occurrence as the survivor when clearing a series"
                    .into(),
            );
        }
        let in_place = !recurring
            || (scope.effective_scope == EditScope::All
                && scope.protected_through.is_none()
                && scope.preserve.is_empty());
        let mut materialize = if !this && !in_place {
            scope.preserve.clone()
        } else {
            Vec::new()
        };
        let (kind, before, mut edited) = if !recurring || this || collapse {
            self.plan_single(&scope, &resolved, &draft, recurring, in_place)?
        } else {
            self.plan_series(&scope, &resolved, &draft, in_place, &materialize)?
        };
        edited.fields.exceptions.sort();
        edited.fields.exceptions.dedup();
        // Reparse the concrete result with the same engine used by persisted
        // window reads, before returning a plan to preview or persistence.
        let edited_template = edited.template(&self.id)?;
        for date in edited_template.overrides.keys() {
            // A new rule can make a formerly unreachable override live. Its
            // geometry must be valid before a preview or transaction accepts it.
            edited_template.resolve_identity_with_budget(*date, &mut budget)?;
        }
        materialize.sort_by(|a, b| a.recurrence_date.cmp(&b.recurrence_date));
        let selected_target = if materialize
            .iter()
            .any(|row| row.recurrence_date == scope.selected.recurrence_date)
        {
            Some(SelectedTarget::Preserved {
                recurrence_date: scope.selected.recurrence_date.clone(),
            })
        } else if before
            .as_ref()
            .map(|side| -> Result<bool, String> {
                Ok(side
                    .template(&self.id)?
                    .resolve_identity_with_budget(selected, &mut budget)?
                    .is_some())
            })
            .transpose()?
            .unwrap_or(false)
        {
            Some(SelectedTarget::Original)
        } else {
            let original = self
                .resolve_identity_with_budget(selected, &mut budget)?
                .ok_or("Calendar selected identity disappeared during planning")?;
            let original_override = original.override_index.and_then(|index| {
                self.overrides
                    .values()
                    .find(|(position, _)| *position == index)
                    .map(|(_, row)| row)
            });
            let date = if edited.metadata_occurrence_date.is_some() {
                edited_template.anchor_date()
            } else if let Some(original_override) = original_override {
                let original_date = override_date(
                    &original_override.recurrence_id,
                    &self.home_zone,
                    self.all_day,
                )?;
                if original_date != selected && original_override.this_and_future {
                    selected
                } else if let Some(row) = edited
                    .overrides
                    .iter()
                    .find(|row| row.source_recurrence_id == original_override.recurrence_id)
                {
                    parse_date(&row.recurrence_id)?
                } else {
                    selected
                }
            } else if edited_template.additions.contains_key(&selected) {
                selected
            } else {
                stored_time(
                    &resolved.start_time,
                    &edited_template.home_zone,
                    resolved.all_day,
                )?
                .0
                .date()
            };
            edited_template
                .resolve_identity_with_budget(date, &mut budget)?
                .map(|_| SelectedTarget::Edited {
                    recurrence_date: date.to_string(),
                })
        };
        let active_transfer = if let (Some(run_id), Some(date)) =
            (&scope.active_run_id, &scope.active_recurrence_date)
        {
            let target = if this && recurring && scope.selected_active {
                Some(ActiveTarget::Edited)
            } else if materialize
                .iter()
                .any(|value| &value.recurrence_date == date)
            {
                Some(ActiveTarget::Preserved {
                    recurrence_date: date.clone(),
                })
            } else {
                None
            };
            target.map(|target| PlannedActiveTransfer {
                run_id: run_id.clone(),
                original_recurrence_date: date.clone(),
                target,
            })
        } else {
            None
        };
        Ok((
            scope,
            resolved,
            EditGeometryPlan {
                kind,
                selected_target,
                before,
                edited: Some(edited),
                materialize,
                active_transfer,
            },
        ))
    }
}

#[cfg(test)]
mod tests;
