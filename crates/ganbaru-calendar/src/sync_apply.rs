//! Calendar work that runs inside the sync engine transaction after rows of a Calendar table
//! materialize.
//!
//! The hook receives only the connection and a change sink, so the desktop service and the
//! headless Android pass run the same code. It rebuilds the hook-maintained projection columns
//! of `icalendar_components` (`projected_kind`, `projected_id`) for the components the batch
//! touched, and reports every changed Calendar row under its table name so the host refreshes
//! views, notification schedules, and the Focus projection after commit.

use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::calendar::{COMPONENTS_TABLE, DOMAIN, EVENTS_TABLE};
use ganbaru_sync::{ChangeSink, HookError, HookFuture, Hooks, MaterializeHook, rebuild_every_key};
use sqlx::SqliteConnection;

/// Rebuilds projections and reports changed rows after sync materializes Calendar rows.
struct CalendarApplyHook;

impl MaterializeHook for CalendarApplyHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            if table.id == EVENTS_TABLE {
                refresh_event_projections(conn, changed_keys).await?;
            } else if table.id == COMPONENTS_TABLE {
                refresh_projections(conn, changed_keys).await?;
            }
            for key in changed_keys {
                changes.report(table.name, key.clone());
            }
            Ok(())
        })
    }

    fn rebuild_all<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        rebuild_every_key(self, conn, table, changes)
    }
}

/// `hooks` with the Calendar post-apply hook registered for every Calendar table.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(CalendarApplyHook))
    })
}

/// Whether a change notice topic names a Calendar table.
pub fn is_calendar_topic(topic: &str) -> bool {
    DOMAIN.tables.iter().any(|table| table.name == topic)
}

fn keys_json(keys: &[String]) -> Result<String, HookError> {
    serde_json::to_string(keys).map_err(|error| HookError::from(error.to_string()))
}

fn hook_error(context: &str, error: sqlx::Error) -> HookError {
    HookError::from(format!("{context}: {error}"))
}

/// Rebuilds the projection of every component in an iCalendar object that holds a component
/// of the changed events: the components their rows, alarms, and overrides link, and the
/// components that still project onto an event key, which covers a deleted event.
pub(crate) async fn refresh_event_projections(
    conn: &mut SqliteConnection,
    event_ids: &[String],
) -> Result<(), HookError> {
    let components: Vec<String> = sqlx::query_scalar(
        "WITH changed(id) AS (SELECT value FROM json_each(?1)),
         linked(id) AS (
             SELECT icalendar_component_id FROM calendar_events
             WHERE id IN changed AND icalendar_component_id IS NOT NULL
             UNION SELECT icalendar_component_id FROM calendar_event_alarms
             WHERE event_id IN changed AND icalendar_component_id IS NOT NULL
             UNION SELECT icalendar_component_id FROM calendar_event_overrides
             WHERE parent_event_id IN changed AND icalendar_component_id IS NOT NULL
             UNION SELECT id FROM icalendar_components
             WHERE projected_kind = 'event' AND projected_id IN changed
         )
         SELECT id FROM icalendar_components
         WHERE object_id IN (SELECT object_id FROM icalendar_components WHERE id IN linked)",
    )
    .bind(keys_json(event_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| hook_error("find projected Calendar components", error))?;
    refresh_projections(conn, &components).await
}

/// Sets each component's projection to the event, alarm, or override that links it, or clears
/// it when nothing does. Unchanged projections are left alone.
pub async fn refresh_projections(
    conn: &mut SqliteConnection,
    component_ids: &[String],
) -> Result<(), HookError> {
    if component_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "WITH target(id) AS (SELECT value FROM json_each(?1)),
         linked(id, event, alarm, override) AS (
             SELECT id,
                 (SELECT MIN(id) FROM calendar_events WHERE icalendar_component_id = target.id),
                 (SELECT MIN(id) FROM calendar_event_alarms
                  WHERE icalendar_component_id = target.id),
                 (SELECT MIN(id) FROM calendar_event_overrides
                  WHERE icalendar_component_id = target.id)
             FROM target
         ),
         projection(id, kind, projected) AS (
             SELECT id,
                 CASE WHEN event IS NOT NULL THEN 'event'
                      WHEN alarm IS NOT NULL THEN 'alarm'
                      WHEN override IS NOT NULL THEN 'override' END,
                 COALESCE(event, alarm, override)
             FROM linked
         )
         UPDATE icalendar_components
         SET projected_kind = projection.kind, projected_id = projection.projected
         FROM projection
         WHERE icalendar_components.id = projection.id
           AND (icalendar_components.projected_kind IS NOT projection.kind
                OR icalendar_components.projected_id IS NOT projection.projected)",
    )
    .bind(keys_json(component_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| hook_error("refresh Calendar component projections", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO calendar_events
                (id, title, start_time, end_time, timezone, calendar_id, created_at, updated_at)
             VALUES ('event-a', '', '2099-05-10T10:00:00Z', '2099-05-10T11:00:00Z', 'UTC',
                     'local', 'now', 'now');
             INSERT INTO icalendar_objects
                (id, import_id, calendar_id, source_kind, source_fingerprint, created_at, updated_at)
             VALUES ('object-a', 'import-a', 'local', 'import-file', 'hash', 'now', 'now');
             INSERT INTO icalendar_components
                (id, object_id, calendar_id, component_type, preservation_status, created_at, updated_at)
             VALUES ('component-event', 'object-a', 'local', 'VEVENT', 'lossless', 'now', 'now');
             INSERT INTO icalendar_components
                (id, object_id, parent_component_id, calendar_id, component_type,
                 preservation_status, created_at, updated_at)
             VALUES ('component-alarm', 'object-a', 'component-event', 'local', 'VALARM',
                     'lossless', 'now', 'now');",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn projections(pool: &sqlx::SqlitePool) -> Vec<(String, Option<String>, Option<String>)> {
        sqlx::query_as(
            "SELECT id, projected_kind, projected_id FROM icalendar_components ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[test]
    fn an_applied_event_projects_its_linked_components() {
        crate::test_support::block_on(async {
            let pool = seed().await;
            sqlx::raw_sql(
                "UPDATE calendar_events SET icalendar_component_id = 'component-event' WHERE id = 'event-a';",
            )
            .execute(&pool)
            .await
            .unwrap();
            let mut connection = pool.acquire().await.unwrap();
            refresh_event_projections(&mut connection, &["event-a".to_string()])
                .await
                .unwrap();
            drop(connection);
            assert_eq!(
                projections(&pool).await,
                vec![
                    ("component-alarm".into(), None, None),
                    (
                        "component-event".into(),
                        Some("event".into()),
                        Some("event-a".into())
                    ),
                ]
            );
        });
    }

    #[test]
    fn a_deleted_event_clears_the_projection_that_named_it() {
        crate::test_support::block_on(async {
            let pool = seed().await;
            sqlx::raw_sql(
                "UPDATE icalendar_components SET projected_kind = 'event', projected_id = 'event-a'
                 WHERE id = 'component-event';
                 DELETE FROM calendar_events WHERE id = 'event-a';",
            )
            .execute(&pool)
            .await
            .unwrap();
            let mut connection = pool.acquire().await.unwrap();
            refresh_event_projections(&mut connection, &["event-a".to_string()])
                .await
                .unwrap();
            drop(connection);
            assert!(
                projections(&pool)
                    .await
                    .iter()
                    .all(|(_, kind, id)| kind.is_none() && id.is_none())
            );
        });
    }

    #[test]
    fn an_applied_component_projects_onto_the_event_that_already_links_it() {
        crate::test_support::block_on(async {
            let pool = seed().await;
            sqlx::raw_sql(
                "UPDATE calendar_events SET icalendar_component_id = 'component-event' WHERE id = 'event-a';",
            )
            .execute(&pool)
            .await
            .unwrap();
            let mut connection = pool.acquire().await.unwrap();
            refresh_projections(&mut connection, &["component-event".to_string()])
                .await
                .unwrap();
            drop(connection);
            assert_eq!(
                projections(&pool).await[1],
                (
                    "component-event".into(),
                    Some("event".into()),
                    Some("event-a".into())
                )
            );
        });
    }

    #[test]
    fn every_calendar_table_has_the_hook_and_names_a_calendar_topic() {
        let hooks = with_hooks(Hooks::new());
        for table in DOMAIN.tables {
            assert!(is_calendar_topic(table.name), "{}", table.name);
        }
        assert!(!is_calendar_topic("project_tasks"));
        assert!(format!("{hooks:?}").contains(&EVENTS_TABLE.0.to_string()));
    }
}
