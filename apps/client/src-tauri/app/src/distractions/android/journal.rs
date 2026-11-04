//! Guardian journal batch import. It holds no Android types, so its batch rules compile and are
//! tested on every host.

use super::{PendingEvent, normalize_event};
use chrono::{DateTime, SecondsFormat, Utc};
use ganbaru_distractions::contracts::DistractionsUsageSampleRow;
use ganbaru_distractions::usage::totals::insert_own_samples;
use ganbaru_distractions::usage::{
    SpooledBlockEvent, SpooledBlockEventError, UsageDateWindow, record_spooled_block_event,
};
use sqlx::SqlitePool;

/// The outcome of one committed Guardian batch.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct JournalImport {
    /// Every event the batch settled, recorded or dropped, for Guardian to acknowledge.
    pub processed_ids: Vec<String>,
    /// Events the vault refused or that failed normalization, which no retry can record.
    pub dropped: usize,
}

fn usage_sample(event: &PendingEvent) -> DistractionsUsageSampleRow {
    DistractionsUsageSampleRow {
        id: event.id.clone(),
        source_type: "mobile-app".to_string(),
        source_key: event.package_name.clone(),
        display_name: Some(event.display_name.clone()),
        started_at_ms: event.started_at_ms,
        elapsed_seconds: event.elapsed_seconds,
        local_date: event.local_date.clone(),
        created_at_ms: event.occurred_at_ms,
    }
}

/// The vault block event a normalized Guardian block becomes, with its single rule snapshot.
fn block_event(event: &PendingEvent) -> Result<SpooledBlockEvent, String> {
    let occurred_at = DateTime::<Utc>::from_timestamp_millis(event.occurred_at_ms)
        .ok_or_else(|| "mobile Distractions block timestamp is invalid".to_string())?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    let usage_limit = event.reason.as_deref() == Some("usage_limit");
    Ok(SpooledBlockEvent {
        id: event.id.clone(),
        run_id: event.run_id.clone(),
        occurred_at,
        source_type: "mobile_app".to_string(),
        source_key: event.package_name.clone(),
        display_name: Some(event.display_name.clone()),
        phase: event.phase.clone(),
        decision: if usage_limit {
            "limit_exhausted"
        } else {
            "blocked"
        }
        .to_string(),
        rule_id: event.rule_id.clone(),
        rule_kind: Some(
            if usage_limit {
                "usage_limit"
            } else {
                "mobile_app"
            }
            .to_string(),
        ),
        rule_label: Some(event.display_name.clone()),
        blocker_mode: Some(if usage_limit { "limit" } else { "blacklist" }.to_string()),
    })
}

/// Imports one Guardian journal batch for `active_vault_id` in one transaction: usage into this
/// device's usage journal inside `window`, and block events with their rule snapshots.
///
/// Each event is settled alone, so one bad event never holds back the batch Guardian returns
/// again in the same order. An event that fails normalization or that the vault refuses is
/// dropped with a log line. Any other vault failure rolls the batch back and keeps it in the
/// journal for the next attempt. Events the vault already holds record again harmlessly, so a
/// batch that committed but was not acknowledged imports again without a second row.
pub(super) async fn import_journal_batch(
    pool: &SqlitePool,
    events: Vec<PendingEvent>,
    active_vault_id: &str,
    window: &UsageDateWindow,
) -> Result<JournalImport, String> {
    let mut processed_ids = Vec::with_capacity(events.len());
    let mut normalized = Vec::with_capacity(events.len());
    let mut dropped = 0;
    for event in events {
        let id = event.id.clone();
        match normalize_event(event) {
            Ok(event) if event.vault_id != active_vault_id => {
                return Err("Guardian returned events from a different vault".into());
            }
            Ok(event) => normalized.push(event),
            Err(reason) => {
                eprintln!("dropped Guardian Distractions event: {reason}");
                dropped += 1;
            }
        }
        processed_ids.push(id);
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin mobile Distractions import: {error}"))?;
    let usage = normalized
        .iter()
        .filter(|event| event.kind == "usage")
        .map(usage_sample)
        .collect::<Vec<_>>();
    insert_own_samples(&mut transaction, &usage, window).await?;
    for event in normalized.iter().filter(|event| event.kind == "block") {
        let recorded = match block_event(event) {
            Ok(block) => record_spooled_block_event(&mut transaction, &block).await,
            Err(reason) => Err(SpooledBlockEventError::Refused(reason)),
        };
        match recorded {
            Ok(()) => {}
            Err(SpooledBlockEventError::Refused(reason)) => {
                eprintln!("dropped Guardian Distractions block event: {reason}");
                dropped += 1;
            }
            Err(SpooledBlockEventError::Failed(reason)) => return Err(reason),
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit mobile Distractions import: {error}"))?;
    Ok(JournalImport {
        processed_ids,
        dropped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str) -> PendingEvent {
        PendingEvent {
            id: id.into(),
            kind: "usage".into(),
            package_name: "com.example.video".into(),
            display_name: "Video".into(),
            started_at_ms: 1_000,
            elapsed_seconds: 1,
            local_date: "2026-10-02".into(),
            occurred_at_ms: 2_000,
            reason: None,
            rule_id: None,
            run_id: None,
            phase: None,
            vault_id: "vault".into(),
        }
    }

    fn block(id: &str) -> PendingEvent {
        let mut block = event(id);
        block.kind = "block".into();
        block.elapsed_seconds = 0;
        block.reason = Some("usage_limit".into());
        block.rule_id = Some("rule-video".into());
        block
    }

    fn window() -> UsageDateWindow {
        ganbaru_distractions::usage::usage_date_window(
            chrono::NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
        )
    }

    async fn vault_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    async fn ids(pool: &SqlitePool, table: &str, column: &str) -> Vec<String> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT {column} FROM {table} ORDER BY {column}"
        )))
        .fetch_all(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn events_the_vault_refuses_or_that_fail_normalization_are_dropped_and_the_rest_import() {
        let pool = vault_pool().await;
        sqlx::raw_sql(
            "CREATE TRIGGER reject_block BEFORE INSERT ON distractions_block_events
             WHEN NEW.id = 'block-2' BEGIN SELECT RAISE(ABORT, 'injected refusal'); END;
             CREATE TRIGGER reject_snapshot BEFORE INSERT ON distractions_block_event_rule_snapshots
             WHEN NEW.block_event_id = 'block-4' BEGIN SELECT RAISE(ABORT, 'injected refusal'); END;",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut invalid_package = block("block-3");
        invalid_package.package_name = "android".into();
        let mut invalid_usage = event("usage-2");
        invalid_usage.elapsed_seconds = 0;
        let batch = vec![
            event("usage-1"),
            block("block-1"),
            block("block-2"),
            invalid_package,
            block("block-4"),
            invalid_usage,
            block("block-5"),
        ];
        let expected_ids: Vec<String> = batch.iter().map(|event| event.id.clone()).collect();
        let import = import_journal_batch(&pool, batch, "vault", &window())
            .await
            .unwrap();
        assert_eq!(
            import,
            JournalImport {
                processed_ids: expected_ids,
                dropped: 4,
            }
        );
        assert_eq!(
            ids(&pool, "distractions_usage_samples", "id").await,
            ["usage-1"]
        );
        // A refused rule snapshot takes its event with it, so no event lacks its snapshot.
        assert_eq!(
            ids(&pool, "distractions_block_events", "id").await,
            ["block-1", "block-5"]
        );
        assert_eq!(
            ids(
                &pool,
                "distractions_block_event_rule_snapshots",
                "block_event_id"
            )
            .await,
            ["block-1", "block-5"]
        );
        let recorded: (String, String, String, String) = sqlx::query_as(
            "SELECT event.decision, event.rule_id, snapshot.rule_kind, snapshot.blocker_mode
             FROM distractions_block_events AS event
             JOIN distractions_block_event_rule_snapshots AS snapshot
                ON snapshot.block_event_id = event.id AND snapshot.rule_id IS event.rule_id
             WHERE event.id = 'block-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            recorded,
            (
                "limit_exhausted".into(),
                "rule-video".into(),
                "usage_limit".into(),
                "limit".into()
            )
        );

        // A batch the vault committed but Guardian did not acknowledge imports again harmlessly.
        let retry = vec![event("usage-1"), block("block-1")];
        import_journal_batch(&pool, retry, "vault", &window())
            .await
            .unwrap();
        assert_eq!(
            ids(&pool, "distractions_usage_samples", "id").await,
            ["usage-1"]
        );
        assert_eq!(
            ids(&pool, "distractions_block_events", "id").await,
            ["block-1", "block-5"]
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn a_vault_failure_that_is_not_a_refusal_rolls_the_whole_batch_back() {
        let pool = vault_pool().await;
        // The trigger body names a missing table, which fails with a plain SQLite error.
        sqlx::raw_sql(
            "CREATE TRIGGER fail_block BEFORE INSERT ON distractions_block_events
             WHEN NEW.id = 'block-2' BEGIN SELECT * FROM missing_table; END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let batch = vec![event("usage-1"), block("block-1"), block("block-2")];
        assert!(
            import_journal_batch(&pool, batch, "vault", &window())
                .await
                .is_err()
        );
        for (table, column) in [
            ("distractions_usage_samples", "id"),
            ("distractions_block_events", "id"),
            ("distractions_block_event_rule_snapshots", "block_event_id"),
        ] {
            assert!(
                ids(&pool, table, column).await.is_empty(),
                "{table} must roll back with the failed batch"
            );
        }
        pool.close().await;
    }

    #[tokio::test]
    async fn events_from_another_vault_fail_the_batch_without_writing() {
        let pool = vault_pool().await;
        let mut foreign = event("usage-2");
        foreign.vault_id = "other".into();
        let batch = vec![event("usage-1"), foreign];
        assert!(
            import_journal_batch(&pool, batch, "vault", &window())
                .await
                .is_err()
        );
        assert!(
            ids(&pool, "distractions_usage_samples", "id")
                .await
                .is_empty()
        );
        pool.close().await;
    }
}
