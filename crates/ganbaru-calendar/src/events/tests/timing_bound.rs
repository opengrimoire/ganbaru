//! Edits refuse an event whose timing value (timing plus every override) is over the bound.

use crate::events::commit::prepare_rows;
use crate::events::edit::prepare;
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use crate::timing_bound::{MAX_TIMING_VALUE_BYTES, TIMING_TOO_LARGE_CODE};
use serde_json::{Value, json};

fn now() -> ScopeClock {
    ScopeClock {
        epoch_ms: chrono::DateTime::parse_from_rfc3339("2099-05-08T00:00:00Z")
            .unwrap()
            .timestamp_millis(),
        floating_today: None,
    }
}

async fn add_overrides(pool: &sqlx::SqlitePool, count: usize, title_bytes: usize) {
    let title = "t".repeat(title_bytes);
    let first = chrono::NaiveDate::from_ymd_opt(2100, 1, 1).unwrap();
    let mut tx = pool.begin().await.unwrap();
    for index in 0..count {
        let day = first + chrono::Days::new(index as u64);
        sqlx::query(
            "INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title)
             VALUES (?, 'source', ?, ?)",
        )
        .bind(format!("bulk-{index}"))
        .bind(day.to_string())
        .bind(&title)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

async fn save(pool: &sqlx::SqlitePool, scope: EditScope) -> Result<(), String> {
    let draft: Value = json!({"fields":[{"field":"title","value":"Edited"}]});
    let mut tx = pool.begin().await.unwrap();
    let revision = prepare(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        scope,
        now(),
        serde_json::from_value(draft.clone()).unwrap(),
    )?
    .review_revision;
    let rows = prepare_rows(
        read_snapshot(&mut tx, "source").await.unwrap(),
        parse_date("2099-05-10").unwrap(),
        scope,
        now(),
        serde_json::from_value(draft).unwrap(),
        "save-1",
        &revision,
    )?;
    rows.write(&mut tx).await?;
    tx.commit().await.map_err(|error| error.to_string())
}

#[test]
fn edit_with_many_changed_occurrences_is_refused_with_the_localizable_code() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        add_overrides(&pool, 300, 1_000).await;
        let error = save(&pool, EditScope::All).await.unwrap_err();
        assert!(error.starts_with(TIMING_TOO_LARGE_CODE), "{error}");
        assert!(
            error.contains(&MAX_TIMING_VALUE_BYTES.to_string()),
            "{error}"
        );
    });
}

#[test]
fn edit_with_a_few_changed_occurrences_still_saves() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        add_overrides(&pool, 50, 1_000).await;
        save(&pool, EditScope::All).await.unwrap();
    });
}

async fn read_source(pool: &sqlx::SqlitePool) -> crate::events::metadata::Metadata {
    let mut connection = pool.acquire().await.unwrap();
    crate::events::metadata::Metadata::read(
        &mut connection,
        "source",
        &mut crate::events::occurrence::ReadBudget::default(),
    )
    .await
    .unwrap()
}

async fn timing_bytes(pool: &sqlx::SqlitePool) -> usize {
    read_source(pool).await.timing_size().bytes()
}

async fn exdate_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM calendar_event_exdates WHERE event_id = 'source'")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn add_exdate(pool: &sqlx::SqlitePool, date: &str) -> Result<(), String> {
    let mut tx = pool.begin().await.unwrap();
    crate::events::archive::add_exdate(&mut tx, "source", date, "2099-05-09T12:00:00Z").await?;
    tx.commit().await.map_err(|error| error.to_string())
}

#[test]
fn excluding_one_occurrence_is_refused_when_its_date_row_crosses_the_bound() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        add_overrides(&pool, 200, 1_000).await;
        // A last override fills the value to fewer free bytes than one date row costs.
        sqlx::query(
            "INSERT INTO calendar_event_overrides (id, parent_event_id, recurrence_id, title)
             VALUES ('filler', 'source', '2101-01-01', '')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let free = MAX_TIMING_VALUE_BYTES - timing_bytes(&pool).await;
        sqlx::query("UPDATE calendar_event_overrides SET title = ? WHERE id = 'filler'")
            .bind("t".repeat(free - 40))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(MAX_TIMING_VALUE_BYTES - timing_bytes(&pool).await, 40);
        let before = exdate_count(&pool).await;

        let error = add_exdate(&pool, "2099-05-09").await.unwrap_err();
        assert!(error.starts_with(TIMING_TOO_LARGE_CODE), "{error}");
        assert_eq!(exdate_count(&pool).await, before);

        sqlx::query("UPDATE calendar_event_overrides SET title = '' WHERE id = 'filler'")
            .execute(&pool)
            .await
            .unwrap();
        add_exdate(&pool, "2099-05-09").await.unwrap();
        add_exdate(&pool, "2099-05-09").await.unwrap();
        assert_eq!(exdate_count(&pool).await, before + 1);
    });
}

#[test]
fn undo_refuses_to_restore_a_source_over_the_bound() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        read_source(&pool).await.prepare_restore(true).unwrap();
        add_overrides(&pool, 300, 1_000).await;
        let error = read_source(&pool)
            .await
            .prepare_restore(true)
            .err()
            .unwrap();
        assert!(error.starts_with(TIMING_TOO_LARGE_CODE), "{error}");
    });
}
