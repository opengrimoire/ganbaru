//! Write complete prepared projections inside the caller's authorized transaction.

use super::plan::{PreparedMutation, PreparedSide};
use sqlx::SqliteConnection;

impl super::Metadata {
    /// Insert an already validated independent import closure before its owners.
    pub(super) async fn write_preservation(
        &self,
        connection: &mut SqliteConnection,
    ) -> Result<(), String> {
        for row in &self.preservation.objects {
            row.insert(connection).await?;
        }
        for row in &self.preservation.diagnostics {
            row.insert(connection).await?;
        }
        for row in &self.preservation.components {
            row.insert(connection).await?;
        }
        for row in &self.preservation.properties {
            row.insert(connection).await?;
        }
        for row in &self.preservation.parameters {
            row.insert(connection).await?;
        }
        for row in &self.preservation.nodes {
            row.insert(connection).await?;
        }
        for row in &self.preservation.warnings {
            row.insert(connection).await?;
        }
        Ok(())
    }
}

impl PreparedMutation {
    /// No planning or source rereads occur after the first write.
    pub(in crate::events) async fn write(
        self,
        connection: &mut SqliteConnection,
    ) -> Result<(), String> {
        for side in self.sides {
            side.write(connection).await?;
        }
        Ok(())
    }
}

impl PreparedSide {
    async fn write(self, connection: &mut SqliteConnection) -> Result<(), String> {
        let metadata = self.metadata;
        let event = metadata
            .events
            .first()
            .ok_or("Prepared Calendar event is missing")?;
        let invalid_link: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM project_tasks WHERE id IN (SELECT value FROM json_each(?1)) AND project_id IS NOT ?2)"
        ).bind(serde_json::to_string(&metadata.task_links.iter().map(|row| &row.task_id).collect::<Vec<_>>()).map_err(|error| error.to_string())?)
            .bind(&event.project_id).fetch_one(&mut *connection).await.map_err(|error| format!("validate prepared Calendar task ownership: {error}"))?;
        if invalid_link {
            return Err("Calendar edit would move linked tasks outside their project".into());
        }
        if self.write_preservation {
            metadata.write_preservation(connection).await?;
        }
        if self.existing {
            event.update(connection).await?;
            sqlx::query("DELETE FROM calendar_event_alarms WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar alarms: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_attendees WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar attendees: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_categories WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar categories: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_exdates WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar exdates: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_extended_properties WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar properties: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_notifications WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar notifications: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_organizers WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar organizers: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_overrides WHERE parent_event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar overrides: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_rdates WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar rdates: {error}"))?;
            sqlx::query("DELETE FROM calendar_event_pomodoro_configs WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar focus_configs: {error}"))?;
            sqlx::query("DELETE FROM music_context_assignments WHERE owner_id = ?1 AND owner_kind IN ('event-snapshot', 'event-override')").bind(&event.id).execute(&mut *connection).await.map_err(|error| format!("clear prepared Calendar music_assignments: {error}"))?;
            sqlx::query("DELETE FROM project_task_event_links WHERE event_id = ?1")
                .bind(&event.id)
                .execute(&mut *connection)
                .await
                .map_err(|error| format!("clear prepared Calendar task_links: {error}"))?;
        } else {
            event.insert(connection).await?;
        }
        for row in &metadata.alarms {
            row.insert(connection).await?;
        }
        for row in &metadata.attendees {
            row.insert(connection).await?;
        }
        for row in &metadata.categories {
            row.insert(connection).await?;
        }
        for row in &metadata.exdates {
            row.insert(connection).await?;
        }
        for row in &metadata.properties {
            row.insert(connection).await?;
        }
        for row in &metadata.notifications {
            row.insert(connection).await?;
        }
        for row in &metadata.organizers {
            row.insert(connection).await?;
        }
        for row in &metadata.overrides {
            row.insert(connection).await?;
        }
        for row in &metadata.override_properties {
            row.insert(connection).await?;
        }
        for row in &metadata.rdates {
            row.insert(connection).await?;
        }
        for row in &metadata.focus_configs {
            row.insert(connection).await?;
        }
        for row in &metadata.count_rhythms {
            row.insert(connection).await?;
        }
        for row in &metadata.sequence_steps {
            row.insert(connection).await?;
        }
        for row in &metadata.music_assignments {
            row.insert(connection).await?;
        }
        for row in &metadata.task_links {
            row.insert(connection).await?;
        }
        Ok(())
    }
}
