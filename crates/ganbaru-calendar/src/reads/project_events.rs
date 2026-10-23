//! Compact listings of project-linked events for other domains that reference events by name, such as Notes
//! mentions.

use ganbaru_db::impl_sqlite_from_row;
use serde::Serialize;
use sqlx::SqlitePool;

/// An event or recurring series with the stored times needed to say when it happens, without its details.
///
/// Times keep the stored encoding: timed events use UTC instants and all-day events use dates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarProjectEventSummary {
    pub id: String,
    pub title: String,
    pub start_time: String,
    pub end_time: String,
    pub all_day: bool,
    pub recurring: bool,
    pub calendar_id: String,
    pub project_id: String,
}

impl_sqlite_from_row!(CalendarProjectEventSummary {
    id,
    title,
    start_time,
    end_time,
    all_day,
    recurring,
    calendar_id,
    project_id,
});

/// List the events and recurring series linked to the given projects, skipping cancelled ones. Recurring series come
/// first, then single events by distance from `now`, up to `limit` events.
///
/// # Errors
///
/// Returns an error when the filter cannot be encoded or the query fails.
pub async fn list_project_event_summaries(
    pool: &SqlitePool,
    project_ids: &[String],
    now: &str,
    limit: u32,
) -> Result<Vec<CalendarProjectEventSummary>, String> {
    if project_ids.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let project_ids = serde_json::to_string(project_ids)
        .map_err(|e| format!("encode project event summary filter: {e}"))?;
    sqlx::query_as::<_, CalendarProjectEventSummary>(
        "SELECT id, title, start_time, end_time, all_day = 1 AS all_day,
                recurring, calendar_id, project_id
         FROM (
           SELECT ce.*,
                  (ce.rrule IS NOT NULL AND ce.rrule <> '')
                    OR EXISTS (SELECT 1 FROM calendar_event_rdates r WHERE r.event_id = ce.id)
                    AS recurring
           FROM calendar_events AS ce
           WHERE ce.project_id IN (SELECT value FROM json_each(?)) AND ce.status <> 'cancelled'
         )
         ORDER BY recurring DESC, abs(julianday(start_time) - julianday(?)), id
         LIMIT ?",
    )
    .bind(project_ids)
    .bind(now)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list project event summaries: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{CalendarProjectEventSummary, list_project_event_summaries};
    use crate::test_support::block_on;
    use sqlx::SqlitePool;

    const NOW: &str = "2026-10-11T12:00:00Z";

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO project_groups (id, name, icon, order_key) VALUES ('group', 'Group', 'lucide:folder', 'a1')")
            .execute(&pool)
            .await
            .unwrap();
        for project in ["project-a", "project-b"] {
            sqlx::query(
                "INSERT INTO projects (id, group_id, name, order_key) VALUES (?, 'group', ?, 'a1')",
            )
            .bind(project)
            .bind(project)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    async fn insert_event(
        pool: &SqlitePool,
        id: &str,
        project_id: Option<&str>,
        start: &str,
        rrule: Option<&str>,
        status: &str,
    ) {
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, project_id, rrule, status)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(id)
        .bind(start)
        .bind(start)
        .bind(project_id)
        .bind(rrule)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    fn ids(events: &[CalendarProjectEventSummary]) -> Vec<&str> {
        events.iter().map(|event| event.id.as_str()).collect()
    }

    #[test]
    fn project_event_summaries_lead_with_series_then_the_nearest_events() {
        block_on(async {
            let pool = pool().await;
            insert_event(
                &pool,
                "far",
                Some("project-a"),
                "2026-01-01T09:00:00Z",
                None,
                "confirmed",
            )
            .await;
            insert_event(
                &pool,
                "near",
                Some("project-b"),
                "2026-10-10T09:00:00Z",
                None,
                "confirmed",
            )
            .await;
            insert_event(
                &pool,
                "series",
                Some("project-a"),
                "2025-01-01T09:00:00Z",
                Some("FREQ=WEEKLY"),
                "confirmed",
            )
            .await;
            insert_event(
                &pool,
                "cancelled",
                Some("project-a"),
                "2026-10-11T09:00:00Z",
                None,
                "cancelled",
            )
            .await;
            insert_event(
                &pool,
                "unlinked",
                None,
                "2026-10-11T09:00:00Z",
                None,
                "confirmed",
            )
            .await;

            let projects = ["project-a".to_string(), "project-b".to_string()];
            let events = list_project_event_summaries(&pool, &projects, NOW, 10)
                .await
                .unwrap();

            assert_eq!(ids(&events), vec!["series", "near", "far"]);
            assert!(events[0].recurring);
            assert!(!events[1].recurring && !events[1].all_day);
            assert_eq!(
                ids(&list_project_event_summaries(&pool, &projects, NOW, 2)
                    .await
                    .unwrap()),
                vec!["series", "near"]
            );
            assert_eq!(
                list_project_event_summaries(&pool, &[], NOW, 10).await,
                Ok(Vec::new())
            );
        });
    }
}
