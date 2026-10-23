//! Canonical Calendar metadata captured alongside recurrence and execution evidence.

mod archive;
mod copy;
mod create;
mod patch;
mod plan;
mod preservation;
mod projection;
mod rows;
mod write;

pub(super) use archive::PreparedArchive;
pub(super) use plan::PreparedMutation;

use std::collections::BTreeSet;
use std::io::{self, Write};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use super::occurrence::ReadBudget;
#[cfg(test)]
use crate::work_gate::WorkGate;
use preservation::Preservation;
use rows::*;

#[cfg(test)]
static COPY_GATE: WorkGate = WorkGate::new();

#[derive(Clone, Default, Serialize)]
pub(super) struct Metadata {
    events: Vec<Event>,
    alarms: Vec<Alarm>,
    attendees: Vec<Attendee>,
    categories: Vec<Category>,
    exdates: Vec<Exdate>,
    properties: Vec<Property>,
    notifications: Vec<Notification>,
    organizers: Vec<Organizer>,
    overrides: Vec<Override>,
    override_properties: Vec<OverrideProperty>,
    rdates: Vec<Rdate>,
    focus_configs: Vec<FocusConfig>,
    count_rhythms: Vec<CountRhythm>,
    sequence_steps: Vec<SequenceStep>,
    music_assignments: Vec<MusicAssignment>,
    task_links: Vec<TaskLink>,
    preservation: Preservation,
}

/// Excludes one occurrence date of a source event, refusing an exclusion that would push its
/// timing value over the sync bound. The row id derives from the source and the date, as on a
/// retained deletion, so the same exclusion made twice is one row.
pub(super) async fn add_exdate(
    connection: &mut SqliteConnection,
    event_id: &str,
    occurrence_date: &str,
) -> Result<(), String> {
    let metadata = Metadata::read(connection, event_id, &mut ReadBudget::default()).await?;
    if metadata
        .exdates
        .iter()
        .any(|row| row.occurrence_date == occurrence_date)
    {
        return Ok(());
    }
    let sort_order = match metadata.exdates.iter().map(|row| row.sort_order).max() {
        Some(last) => last
            .checked_add(1)
            .ok_or("Calendar exception order overflow")?,
        None => 0,
    };
    let row = Exdate {
        id: copy::copied_id(event_id, "exdate", occurrence_date)?,
        event_id: event_id.to_string(),
        occurrence_date: occurrence_date.to_string(),
        sort_order,
    };
    let mut size = metadata.timing_size();
    size.add_date_row(Some(&row.id), &row.occurrence_date);
    size.check()?;
    row.insert(connection).await
}

impl Metadata {
    pub(super) fn preservation_revision(&self) -> Result<String, String> {
        self.preservation.validate()?;
        revision(&self.preservation)
    }

    /// A deleted source leaves its original imported graph available. Capture
    /// that exact closure to reject later changes instead of rewinding them.
    pub(super) async fn read_original_preservation(
        &self,
        connection: &mut SqliteConnection,
        budget: &mut ReadBudget,
    ) -> Result<Self, String> {
        let roots = self
            .events
            .iter()
            .filter_map(|row| row.icalendar_component_id.clone())
            .chain(
                self.alarms
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .chain(
                self.attendees
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .chain(
                self.overrides
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .collect();
        Ok(Self {
            preservation: Preservation::read(connection, roots, budget).await?,
            ..Self::default()
        })
    }

    /// Project links and imported preservation state protect interoperability
    /// identity independently of Focus history or the displayed occurrence.
    pub(super) fn has_durable_references(&self) -> bool {
        !self.task_links.is_empty() || !self.preservation.objects.is_empty()
    }

    /// Explicit current-event admission cannot silently change a configured series.
    pub(super) fn can_enable_focus(&self) -> bool {
        self.focus_configs.is_empty()
            && self.rdates.is_empty()
            && self
                .events
                .first()
                .is_some_and(|event| event.rrule.as_deref().is_none_or(str::is_empty))
    }

    /// Read exact values under the caller's transaction and shared source budget.
    pub(super) async fn read(
        connection: &mut SqliteConnection,
        id: &str,
        budget: &mut ReadBudget,
    ) -> Result<Self, String> {
        let events = Event::read(connection, id, budget).await?;
        let alarms = Alarm::read(connection, id, budget).await?;
        let attendees = Attendee::read(connection, id, budget).await?;
        let categories = Category::read(connection, id, budget).await?;
        let exdates = Exdate::read(connection, id, budget).await?;
        let properties = Property::read(connection, id, budget).await?;
        let notifications = Notification::read(connection, id, budget).await?;
        let organizers = Organizer::read(connection, id, budget).await?;
        let overrides = Override::read(connection, id, budget).await?;
        let override_properties = OverrideProperty::read(connection, id, budget).await?;
        let rdates = Rdate::read(connection, id, budget).await?;
        let focus_configs = FocusConfig::read(connection, id, budget).await?;
        let count_rhythms = CountRhythm::read(connection, id, budget).await?;
        let sequence_steps = SequenceStep::read(connection, id, budget).await?;
        let music_assignments = MusicAssignment::read(connection, id, budget).await?;
        let task_links = TaskLink::read(connection, id, budget).await?;
        if events.len() != 1 {
            return Err("Calendar metadata source is missing".into());
        }
        let roots: BTreeSet<String> = events
            .iter()
            .filter_map(|row| row.icalendar_component_id.clone())
            .chain(
                overrides
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .chain(
                attendees
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .chain(
                alarms
                    .iter()
                    .filter_map(|row| row.icalendar_component_id.clone()),
            )
            .collect();
        let preservation = Preservation::read(connection, roots, budget).await?;
        Ok(Self {
            events,
            alarms,
            attendees,
            categories,
            exdates,
            properties,
            notifications,
            organizers,
            overrides,
            override_properties,
            rdates,
            focus_configs,
            count_rhythms,
            sequence_steps,
            music_assignments,
            task_links,
            preservation,
        })
    }

    /// Hash exact rows, including child identities, provenance and imported values.
    /// This is a review revision, not permission to replay a client-supplied plan.
    pub(super) fn revision(&self) -> Result<String, String> {
        self.preservation.validate()?;
        if self.events.iter().any(|row| {
            row.geo_lat
                .into_iter()
                .chain(row.geo_lng)
                .any(|value| !value.is_finite())
        }) {
            return Err("Calendar metadata contains non-finite coordinates".into());
        }
        revision(self)
    }
}

struct RevisionWriter(Sha256);
impl Write for RevisionWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn revision(value: &impl Serialize) -> Result<String, String> {
    let mut writer = RevisionWriter(Sha256::new());
    writer.0.update(b"ganbaru-calendar-source-v1\0");
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| format!("hash Calendar source: {error}"))?;
    Ok(format!("{:x}", writer.0.finalize()))
}

/// Exercise metadata copying in isolated transaction and protection fixtures.
/// Production commits use the reviewed native semantic operation.
#[cfg(test)]
pub(super) async fn copy_event_metadata(
    connection: &mut SqliteConnection,
    source: &str,
    target: &str,
) -> Result<(), String> {
    let permit = COPY_GATE.try_acquire().map_err(
        |_| "A Calendar metadata copy is already being prepared; retry when it finishes",
    )?;
    let metadata = Metadata::read(connection, source, &mut ReadBudget::default()).await?;
    let target_row = Event::read(connection, target, &mut ReadBudget::default())
        .await?
        .pop()
        .ok_or("Calendar metadata copy target is missing")?;
    let mismatched_task: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_task_event_links link
         JOIN project_tasks task ON task.id = link.task_id
         JOIN calendar_events target ON target.id = ?2
         WHERE link.event_id = ?1 AND task.project_id IS NOT target.project_id)",
    )
    .bind(source)
    .bind(target)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| format!("validate copied Calendar task links: {error}"))?;
    if mismatched_task {
        return Err("Calendar copy would move linked tasks outside their project".into());
    }
    let (copy, _permit) = tokio::task::spawn_blocking(move || {
        // Cancellation of the waiter cannot admit another worker prematurely.
        metadata
            .prepare_base_copy(&target_row)
            .map(|copy| (copy, permit))
    })
    .await
    .map_err(|error| format!("prepare Calendar metadata worker: {error}"))??;
    copy.write(connection, target).await
}
