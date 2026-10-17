//! Bound and prepare complete rows for a native scoped Calendar mutation.

use std::io::{self, Write};

use super::super::edit::EventDraft;
use super::copy::copied_id;
use super::rows::*;
use super::{Metadata, revision};
use crate::recurrence::canonical::{EditGeometryPlan, EditKind, PlannedSeries};
use chrono::{DateTime, SecondsFormat};
use serde::Serialize;

const MAX_WRITE_ROWS: usize = 10_000;
const MAX_WRITE_BYTES: usize = 16 * 1024 * 1024;
const COPY_ID_ALLOWANCE_PER_ROW: usize = 512;

#[derive(Serialize)]
pub(super) struct PreparedSide {
    pub(super) metadata: Metadata,
    pub(super) existing: bool,
    pub(super) write_preservation: bool,
}

pub(in crate::events) struct PreparedMutation {
    pub(super) sides: Vec<PreparedSide>,
    pub(in crate::events) edited_id: String,
    pub(in crate::events) preserved_ids: Vec<(String, String)>,
}

impl PreparedMutation {
    /// A single new source shares the full writer and aggregate budgets of edits.
    pub(in crate::events) fn creation(metadata: Metadata) -> Result<Self, String> {
        Self::creations(vec![metadata])
    }

    /// Task scheduling shares the complete aggregate creation budget.
    pub(in crate::events) fn creations(metadata: Vec<Metadata>) -> Result<Self, String> {
        if metadata.iter().map(Metadata::record_count).sum::<usize>() > MAX_WRITE_ROWS {
            return Err("Calendar creation exceeds its row budget".into());
        }
        let edited_id = metadata
            .first()
            .ok_or("Calendar creation has no source")?
            .events
            .first()
            .ok_or("Calendar creation has no source")?
            .id
            .clone();
        let result = Self {
            sides: metadata
                .into_iter()
                .map(|metadata| PreparedSide {
                    metadata,
                    existing: false,
                    write_preservation: false,
                })
                .collect(),
            edited_id,
            preserved_ids: Vec::new(),
        };
        serde_json::to_writer(&mut MeasuredBytes(0), &result.sides)
            .map_err(|error| format!("measure Calendar creation: {error}"))?;
        Ok(result)
    }

    /// Active edits and protected materializations retain the accepted start at
    /// their resulting anchor. Resolve its ID through the canonical engine, so
    /// gaining recurrence cannot turn a bare anchor ID into a synthetic ID.
    pub(in crate::events) fn active_anchor_id(
        &self,
        target: &str,
        expected_start: i64,
    ) -> Result<String, String> {
        use crate::recurrence::canonical::{StoredOverride, StoredTemplate, Template};
        let metadata = &self
            .sides
            .iter()
            .find(|side| side.metadata.events[0].id == target)
            .ok_or("Calendar active target is absent from prepared rows")?
            .metadata;
        let event = &metadata.events[0];
        let template = Template::from_stored(StoredTemplate {
            id: &event.id,
            start: &event.start_time,
            end: &event.end_time,
            home_zone: &event.timezone,
            all_day: event.all_day != 0,
            rrule: event.rrule.as_deref(),
            repeat_until: event.repeat_until.as_deref(),
            exceptions: &metadata
                .exdates
                .iter()
                .map(|row| row.occurrence_date.clone())
                .collect::<Vec<_>>(),
            rdates: &metadata
                .rdates
                .iter()
                .map(|row| row.occurrence_start.clone())
                .collect::<Vec<_>>(),
            overrides: metadata
                .overrides
                .iter()
                .map(|row| StoredOverride {
                    recurrence_id: row.recurrence_id.clone(),
                    start: row.start_time.clone(),
                    end: row.end_time.clone(),
                    cancelled: row.status.as_deref() == Some("cancelled"),
                    this_and_future: row.recurrence_range.as_deref() == Some("this-and-future"),
                })
                .collect(),
        })?;
        let occurrence = template
            .resolve_identity(None)?
            .ok_or("Calendar active target has no canonical anchor")?;
        if occurrence.start_ms != expected_start {
            return Err("Calendar active target moved its accepted start".into());
        }
        Ok(occurrence.id)
    }
}

struct MeasuredBytes(usize);
impl Write for MeasuredBytes {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|value| *value <= MAX_WRITE_BYTES)
            .ok_or_else(|| io::Error::other("Calendar write exceeds its byte budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Metadata {
    /// Restore exact captured rows through the same writer. Imported objects
    /// remain in place and must be verified before this prepared write is used.
    pub(in crate::events) fn prepare_restore(
        &self,
        existing: bool,
    ) -> Result<PreparedMutation, String> {
        self.reserve_deletion_copies(1)?;
        let id = self
            .events
            .first()
            .ok_or("Calendar Undo source is missing")?
            .id
            .clone();
        Ok(PreparedMutation {
            sides: vec![PreparedSide {
                metadata: self.clone(),
                existing,
                write_preservation: false,
            }],
            edited_id: id,
            preserved_ids: Vec::new(),
        })
    }

    /// Reserve all independent archive copies, the retained source and Undo
    /// preimage before allocating scoped deletion fanout.
    pub(in crate::events) fn reserve_deletion_copies(&self, copies: usize) -> Result<(), String> {
        let rows = self.record_count();
        let mut measured = MeasuredBytes(0);
        serde_json::to_writer(&mut measured, self)
            .map_err(|error| format!("measure Calendar deletion source: {error}"))?;
        if rows
            .checked_mul(copies)
            .and_then(|rows| rows.checked_mul(2))
            .is_none_or(|rows| rows > MAX_WRITE_ROWS)
            || rows
                .checked_mul(COPY_ID_ALLOWANCE_PER_ROW)
                .and_then(|ids| {
                    measured
                        .0
                        .checked_mul(2)
                        .and_then(|bytes| bytes.checked_add(ids))
                })
                .and_then(|bytes| bytes.checked_mul(copies))
                .is_none_or(|bytes| bytes > MAX_WRITE_BYTES)
        {
            return Err("Calendar deletion exceeds its aggregate row or byte budget".into());
        }
        Ok(())
    }

    /// Apply only the native retained recurrence set. Original child identities
    /// and complete metadata survive without draft reconstruction or copying.
    pub(in crate::events) fn retained_deletion(
        &self,
        side: &crate::recurrence::canonical::PartitionSide,
        now_ms: i64,
    ) -> Result<PreparedMutation, String> {
        let mut metadata = self.clone();
        let event = metadata
            .events
            .first_mut()
            .ok_or("Calendar deletion source is missing")?;
        event.start_time.clone_from(&side.start_time);
        event.end_time.clone_from(&side.end_time);
        event.rrule.clone_from(&side.rrule);
        event.repeat_until = None;
        event.updated_at = DateTime::from_timestamp_millis(now_ms)
            .ok_or("Calendar deletion clock is invalid")?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        event.sequence = event
            .sequence
            .checked_add(1)
            .ok_or("Calendar sequence overflow")?;
        let id = event.id.clone();
        metadata
            .overrides
            .retain(|row| side.override_recurrence_ids.contains(&row.recurrence_id));
        metadata.override_properties.retain(|row| {
            metadata
                .overrides
                .iter()
                .any(|owner| owner.id == row.override_id)
        });
        metadata.exdates = side
            .exceptions
            .iter()
            .enumerate()
            .map(|(index, date)| {
                let existing = self.exdates.iter().find(|row| row.occurrence_date == *date);
                Ok(Exdate {
                    id: existing
                        .map(|row| Ok(row.id.clone()))
                        .unwrap_or_else(|| copied_id(&id, "exdate", date))?,
                    event_id: id.clone(),
                    occurrence_date: date.clone(),
                    sort_order: index as i64,
                })
            })
            .collect::<Result<_, String>>()?;
        metadata
            .rdates
            .retain(|row| side.rdates.contains(&row.occurrence_start));
        Ok(PreparedMutation {
            sides: vec![PreparedSide {
                metadata,
                existing: true,
                write_preservation: false,
            }],
            edited_id: id,
            preserved_ids: Vec::new(),
        })
    }

    /// Bound an independent archive before cloning its graph and child rows.
    pub(super) fn reserve_archive_copy(&self) -> Result<(), String> {
        let rows = self.record_count();
        let mut measured = MeasuredBytes(0);
        serde_json::to_writer(&mut measured, self)
            .map_err(|error| format!("measure Calendar archive source: {error}"))?;
        if rows > MAX_WRITE_ROWS
            || measured
                .0
                .checked_mul(2)
                .and_then(|value| {
                    rows.checked_mul(COPY_ID_ALLOWANCE_PER_ROW)
                        .and_then(|ids| value.checked_add(ids))
                })
                .is_none_or(|value| value > MAX_WRITE_BYTES)
        {
            return Err("Calendar archive exceeds its aggregate row or byte budget".into());
        }
        Ok(())
    }

    fn record_count(&self) -> usize {
        self.events.len()
            + self.alarms.len()
            + self.attendees.len()
            + self.categories.len()
            + self.exdates.len()
            + self.properties.len()
            + self.notifications.len()
            + self.organizers.len()
            + self.overrides.len()
            + self.override_properties.len()
            + self.rdates.len()
            + self.focus_configs.len()
            + self.count_rhythms.len()
            + self.sequence_steps.len()
            + self.music_assignments.len()
            + self.task_links.len()
            + self.preservation.components.len()
            + self.preservation.properties.len()
            + self.preservation.parameters.len()
            + self.preservation.nodes.len()
            + self.preservation.warnings.len()
            + self.preservation.objects.len()
            + self.preservation.diagnostics.len()
    }

    /// Reserve the worst-case source fanout before allocating any side copies.
    pub(in crate::events) fn prepare_mutation(
        &self,
        plan: &EditGeometryPlan,
        draft: &EventDraft,
        command_id: &str,
        now_ms: i64,
    ) -> Result<PreparedMutation, String> {
        let source = self.events.first().ok_or("Calendar source is missing")?;
        let edited_id = if matches!(plan.kind, EditKind::Unchanged | EditKind::Update) {
            source.id.clone()
        } else {
            format!(
                "calendar-edit-{}",
                revision(&(command_id, &source.id, "edited"))?
            )
        };
        let copies = usize::from(plan.before.is_some())
            + usize::from(plan.edited.is_some())
            + plan.materialize.len();
        let mut edited_sample = self.clone();
        edited_sample.apply_draft(draft, now_ms)?;
        let rows = self.record_count().max(edited_sample.record_count());
        let mut measured = MeasuredBytes(0);
        serde_json::to_writer(&mut measured, self)
            .map_err(|error| format!("measure Calendar write source: {error}"))?;
        serde_json::to_writer(&mut measured, &edited_sample)
            .map_err(|error| format!("measure Calendar edited source: {error}"))?;
        serde_json::to_writer(&mut measured, plan)
            .map_err(|error| format!("measure Calendar planned geometry: {error}"))?;
        if rows
            .checked_mul(copies)
            .is_none_or(|value| value > MAX_WRITE_ROWS)
            || rows
                .checked_mul(COPY_ID_ALLOWANCE_PER_ROW)
                .and_then(|value| value.checked_add(measured.0))
                .and_then(|value| value.checked_mul(copies))
                .is_none_or(|value| value > MAX_WRITE_BYTES)
        {
            return Err("Calendar write fanout exceeds its aggregate row or byte budget".into());
        }
        let mut result = PreparedMutation {
            sides: Vec::with_capacity(copies),
            edited_id,
            preserved_ids: Vec::new(),
        };
        if let Some(before) = &plan.before {
            result
                .sides
                .push(self.prepare_side(before, None, &source.id, now_ms)?);
        }
        if let Some(edited) = &plan.edited {
            result
                .sides
                .push(self.prepare_side(edited, Some(draft), &result.edited_id, now_ms)?);
        }
        for occurrence in &plan.materialize {
            let id = format!(
                "calendar-edit-{}",
                revision(&(command_id, &source.id, &occurrence.recurrence_date))?
            );
            let format = |value| -> Result<String, String> {
                let instant = DateTime::from_timestamp_millis(value)
                    .ok_or("Calendar materialization instant is invalid")?;
                Ok(if source.all_day != 0 {
                    instant.date_naive().to_string()
                } else {
                    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
                })
            };
            let series = PlannedSeries {
                fields: crate::recurrence::canonical::PartitionSide {
                    start_time: format(occurrence.start_ms)?,
                    end_time: format(occurrence.end_ms)?,
                    rrule: None,
                    exceptions: Vec::new(),
                    rdates: Vec::new(),
                    override_recurrence_ids: Vec::new(),
                },
                timezone: source.timezone.clone(),
                all_day: source.all_day != 0,
                overrides: Vec::new(),
                metadata_occurrence_date: Some(occurrence.recurrence_date.clone()),
            };
            result
                .sides
                .push(self.prepare_side(&series, None, &id, now_ms)?);
            result
                .preserved_ids
                .push((occurrence.recurrence_date.clone(), id));
        }
        let mut measured = MeasuredBytes(0);
        for side in &result.sides {
            serde_json::to_writer(&mut measured, side)
                .map_err(|error| format!("measure prepared Calendar write: {error}"))?;
        }
        if result
            .sides
            .iter()
            .map(|side| side.metadata.record_count())
            .sum::<usize>()
            > MAX_WRITE_ROWS
        {
            return Err("Prepared Calendar write exceeds its aggregate row budget".into());
        }
        Ok(result)
    }

    fn prepare_side(
        &self,
        series: &PlannedSeries,
        draft: Option<&EventDraft>,
        target: &str,
        now_ms: i64,
    ) -> Result<PreparedSide, String> {
        let mut metadata = self.clone();
        if let Some(date) = &series.metadata_occurrence_date {
            metadata.inherit_occurrence(date)?;
        }
        if let Some(draft) = draft {
            metadata.apply_draft(draft, now_ms)?;
        }
        let source = &self.events[0];
        let existing = source.id == target;
        let now = DateTime::from_timestamp_millis(now_ms)
            .ok_or("Calendar write clock is invalid")?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let event = &mut metadata.events[0];
        event.description =
            crate::description::sanitize_calendar_description_html(&event.description);
        event.start_time.clone_from(&series.fields.start_time);
        event.end_time.clone_from(&series.fields.end_time);
        event.rrule.clone_from(&series.fields.rrule);
        event.repeat_until = None;
        event.timezone.clone_from(&series.timezone);
        event.all_day = i64::from(series.all_day);
        event.updated_at.clone_from(&now);
        event.sequence = if existing {
            source
                .sequence
                .checked_add(1)
                .ok_or("Calendar sequence overflow")?
        } else {
            0
        };
        if !existing {
            event.source_uid = None;
            event.created_at.clone_from(&now);
        }
        if event.all_day != 0 {
            metadata.focus_configs.clear();
            metadata.count_rhythms.clear();
            metadata.sequence_steps.clear();
        }
        let mut overrides = Vec::with_capacity(series.overrides.len());
        for planned in &series.overrides {
            let mut row = metadata
                .overrides
                .iter()
                .find(|row| row.recurrence_id == planned.source_recurrence_id)
                .cloned()
                .ok_or("Calendar plan references an uncaptured override")?;
            row.recurrence_id.clone_from(&planned.recurrence_id);
            row.start_time.clone_from(&planned.start_time);
            row.end_time.clone_from(&planned.end_time);
            if planned.cancelled {
                row.status = Some("cancelled".into());
            }
            if let Some(description) = &mut row.description {
                *description = crate::description::sanitize_calendar_description_html(description);
            }
            row.recurrence_range = planned.this_and_future.then(|| "this-and-future".into());
            overrides.push(row);
        }
        metadata
            .override_properties
            .retain(|row| overrides.iter().any(|owner| owner.id == row.override_id));
        metadata.overrides = overrides;
        metadata.exdates = series
            .fields
            .exceptions
            .iter()
            .enumerate()
            .map(|(index, date)| {
                Ok(Exdate {
                    id: copied_id(target, "exdate", date)?,
                    event_id: source.id.clone(),
                    occurrence_date: date.clone(),
                    sort_order: index as i64,
                })
            })
            .collect::<Result<_, String>>()?;
        metadata.rdates = series
            .fields
            .rdates
            .iter()
            .enumerate()
            .map(|(index, value)| {
                Ok(Rdate {
                    id: copied_id(target, "rdate", value)?,
                    event_id: source.id.clone(),
                    occurrence_start: value.clone(),
                    sort_order: index as i64,
                })
            })
            .collect::<Result<_, String>>()?;
        let write_preservation = !existing
            || metadata.events[0].calendar_id != source.calendar_id
            || series.metadata_occurrence_date.is_some()
                && metadata.events[0].icalendar_component_id != source.icalendar_component_id;
        if write_preservation {
            let mut target_row = metadata.events[0].clone();
            target_row.id = target.into();
            metadata = metadata.copy_to(target_row)?;
        }
        Ok(PreparedSide {
            metadata,
            existing,
            write_preservation,
        })
    }

    /// Apply stored override deltas, preserving an explicit empty string.
    pub(in crate::events) fn inherit_occurrence(&mut self, date: &str) -> Result<(), String> {
        let event = &mut self.events[0];
        let zone = ganbaru_civil_time::zone(&event.timezone)?;
        let mut selected = None;
        for row in &self.overrides {
            let key = crate::recurrence::canonical::override_date(
                &row.recurrence_id,
                &zone,
                event.all_day != 0,
            )?
            .to_string();
            if key == date {
                selected = Some(row);
                break;
            }
        }
        let Some(row) = selected else {
            return Ok(());
        };
        macro_rules! inherit { ($($field:ident),+) => { $(if let Some(value) = &row.$field { event.$field.clone_from(value); })+ }; }
        inherit!(
            title,
            description,
            location,
            url,
            status,
            transparency,
            visibility
        );
        if row.color.is_some() {
            event.color = row.color;
        }
        for property in self
            .override_properties
            .iter()
            .filter(|property| property.override_id == row.id)
        {
            if let Some(existing) = self
                .properties
                .iter_mut()
                .find(|value| value.property_key == property.property_key)
            {
                existing.property_value.clone_from(&property.property_value);
            } else {
                self.properties.push(Property {
                    id: copied_id(&event.id, "override-extension", &property.id)?,
                    event_id: event.id.clone(),
                    property_key: property.property_key.clone(),
                    property_value: property.property_value.clone(),
                    sort_order: property.sort_order,
                });
            }
        }
        // Keep both original components in the captured graph. The detached
        // root uses the occurrence's source provenance when one was imported.
        if row.icalendar_component_id.is_some() {
            event
                .icalendar_component_id
                .clone_from(&row.icalendar_component_id);
        }
        Ok(())
    }
}
