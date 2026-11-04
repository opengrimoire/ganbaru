//! Edits refuse an event whose timing value (timing plus every override), or any other value it
//! replicates as one group of rows, is over the bound.

use crate::events::commit::prepare_rows;
use crate::events::edit::prepare;
use crate::events::scope::read_snapshot;
use crate::recurrence::canonical::{EditScope, ScopeClock, parse_date};
use crate::value_bound::{MAX_VALUE_BYTES, TIMING_TOO_LARGE_CODE, VALUE_TOO_LARGE_CODE};
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
        assert!(error.contains(&MAX_VALUE_BYTES.to_string()), "{error}");
    });
}

async fn add_categories(pool: &sqlx::SqlitePool, count: usize, bytes: usize) {
    let category = "c".repeat(bytes);
    let mut tx = pool.begin().await.unwrap();
    for index in 0..count {
        sqlx::query(
            "INSERT INTO calendar_event_categories (id, event_id, category, sort_order)
             VALUES (?, 'source', ?, ?)",
        )
        .bind(format!("bulk-{index}"))
        .bind(&category)
        .bind(index as i64)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}

#[test]
fn edit_with_oversized_categories_is_refused_with_the_value_code() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        add_categories(&pool, 300, 1_000).await;
        let error = save(&pool, EditScope::All).await.unwrap_err();
        assert!(error.starts_with(VALUE_TOO_LARGE_CODE), "{error}");
        assert!(error.contains("categories"), "{error}");
        let error = read_source(&pool)
            .await
            .prepare_restore(true)
            .err()
            .unwrap();
        assert!(error.starts_with(VALUE_TOO_LARGE_CODE), "{error}");
    });
}

#[test]
fn every_group_estimate_grows_with_its_rows() {
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        let before = read_source(&pool).await.group_sizes();
        add_categories(&pool, 1, 10).await;
        let after = read_source(&pool).await.group_sizes();
        for ((name, before), (_, after)) in before.iter().zip(&after) {
            if *name == "categories" {
                assert!(after.bytes() >= before.bytes() + 10, "{name}");
            } else {
                assert_eq!(after.bytes(), before.bytes(), "{name}");
            }
        }
        assert!(read_source(&pool).await.check_value_bounds().is_ok());
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

/// Charges every stored row of `table` under `parent_column = parent` the way a stored row is
/// charged, leaving out the parent column, which the sync value does not carry.
async fn charge_table(
    pool: &sqlx::SqlitePool,
    table: &str,
    parent_column: &str,
    parent: &str,
) -> crate::value_bound::ValueSize {
    use sqlx::{Column, Row, TypeInfo, ValueRef};
    let sql = format!("SELECT * FROM {table} WHERE {parent_column} = ?");
    let rows = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(parent)
        .fetch_all(pool)
        .await
        .unwrap();
    let mut size = crate::value_bound::ValueSize::default();
    for row in &rows {
        size.row();
        for column in row.columns() {
            let name = column.name();
            if name == parent_column {
                continue;
            }
            let raw = row.try_get_raw(column.ordinal()).unwrap();
            if raw.is_null() {
                size.field(&None::<String>);
            } else if raw.type_info().name() == "TEXT" {
                size.named_field(name, &row.get::<String, _>(column.ordinal()));
            } else {
                size.integer();
            }
        }
    }
    size
}

#[test]
fn every_archive_value_fits_within_the_live_value_it_copies() {
    use super::fixtures::{CalendarEventMutationTarget, archive_calendar_event_tx};
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        sqlx::raw_sql(
            "INSERT INTO calendar_event_exdates (id, event_id, occurrence_date)
                 VALUES ('x', 'source', '2099-05-11');
             INSERT INTO calendar_event_rdates (id, event_id, occurrence_start)
                 VALUES ('r', 'source', '2099-06-01T10:00:00Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let source = read_source(&pool).await;
        let groups = source.group_sizes();
        let live = |name: &str| {
            groups
                .iter()
                .find(|(group, _)| *group == name)
                .map(|(_, size)| size.bytes())
                .unwrap()
        };
        let timing = source.timing_size().bytes();
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "source".into(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let archive =
            |table: &'static str| charge_table(&pool, table, "archive_event_id", "source");
        let mut overrides = archive("calendar_event_archive_overrides").await;
        overrides.absorb(&archive("calendar_event_archive_override_extended_properties").await);
        let mut focus = archive("calendar_event_archive_pomodoro_configs").await;
        focus.absorb(&archive("calendar_event_archive_pomodoro_config_count_rhythms").await);
        focus.absorb(&archive("calendar_event_archive_pomodoro_config_sequence_steps").await);
        let pairs = [
            (
                archive("calendar_event_archive_alarms").await,
                live("alarms"),
            ),
            (
                archive("calendar_event_archive_attendees").await,
                live("attendees"),
            ),
            (
                archive("calendar_event_archive_organizers").await,
                live("organizer"),
            ),
            (
                archive("calendar_event_archive_categories").await,
                live("categories"),
            ),
            (
                archive("calendar_event_archive_notifications").await,
                live("notifications"),
            ),
            (
                archive("calendar_event_archive_extended_properties").await,
                live("extended properties"),
            ),
            (focus, live("Focus settings")),
            (
                archive("calendar_event_archive_music_assignments").await,
                live("music assignments"),
            ),
            (
                archive("calendar_event_archive_task_links").await,
                live("task links"),
            ),
            (archive("calendar_event_archive_exdates").await, timing),
            (archive("calendar_event_archive_rdates").await, timing),
            (overrides, timing),
        ];
        for (index, (archived, live)) in pairs.iter().enumerate() {
            assert!(
                archived.bytes() > crate::value_bound::ValueSize::default().bytes(),
                "{index}"
            );
            assert!(
                archived.bytes() <= *live,
                "{index}: {} > {live}",
                archived.bytes()
            );
        }
    });
}

/// Copied import graphs keep every component's graph value within its original's charge.
#[test]
fn archived_component_graphs_fit_within_their_originals() {
    use super::fixtures::{CalendarEventMutationTarget, archive_calendar_event_tx};
    crate::test_support::block_on(async {
        let pool = super::metadata::seed().await;
        async fn graphs(pool: &sqlx::SqlitePool, original: bool) -> Vec<usize> {
            let ids: Vec<String> = sqlx::query_scalar(if original {
                "SELECT id FROM icalendar_components WHERE object_id = 'object'"
            } else {
                "SELECT id FROM icalendar_components WHERE object_id <> 'object'"
            })
            .fetch_all(pool)
            .await
            .unwrap();
            let mut sizes = Vec::new();
            for id in &ids {
                let mut size = crate::value_bound::ValueSize::default();
                for table in [
                    "icalendar_component_projection_warnings",
                    "icalendar_component_properties",
                    "icalendar_property_parameters",
                    "icalendar_value_nodes",
                ] {
                    size.absorb(&charge_table(pool, table, "component_id", id).await);
                }
                sizes.push(size.bytes());
            }
            sizes.sort_unstable_by(|a, b| b.cmp(a));
            sizes
        }
        let originals = graphs(&pool, true).await;
        let mut tx = pool.begin().await.unwrap();
        archive_calendar_event_tx(
            &mut tx,
            &CalendarEventMutationTarget {
                id: "source".into(),
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let copies = graphs(&pool, false).await;
        assert!(!copies.is_empty());
        assert!(copies.len() <= originals.len());
        for (copy, original) in copies.iter().zip(&originals) {
            assert!(copy <= original, "{copy} > {original}");
        }
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
        let free = MAX_VALUE_BYTES - timing_bytes(&pool).await;
        sqlx::query("UPDATE calendar_event_overrides SET title = ? WHERE id = 'filler'")
            .bind("t".repeat(free - 40))
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(MAX_VALUE_BYTES - timing_bytes(&pool).await, 40);
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
