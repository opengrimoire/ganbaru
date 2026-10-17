use super::*;
use sqlx::Row;

fn state(phase: &str) -> DistractionsRuntimeState {
    DistractionsRuntimeState {
        active: phase != "inactive",
        paused: false,
        pause_reason: None,
        phase: phase.to_string(),
        active_run_id: None,
        active_occurrence_id: None,
        remaining_seconds: Some(30),
        updated_at: "2026-05-26T00:00:00.000Z".to_string(),
        valid_until_ms: None,
    }
}

#[test]
fn usage_samples_reject_protected_desktop_apps() {
    let sample = DistractionsUsageSampleInput {
        id: Some("sample-1".to_string()),
        source_type: "desktop-app".to_string(),
        source_key: "Terminal".to_string(),
        display_name: Some("Terminal".to_string()),
        started_at_ms: 1_779_923_600_000,
        elapsed_seconds: 30,
        local_date: "2026-05-28".to_string(),
    };

    assert!(super::normalize_usage_sample(sample, "test").is_err());
}

#[test]
fn desktop_block_events_reject_protected_apps() {
    let event = super::DistractionsDesktopBlockEventInput {
        app_name: "Terminal".to_string(),
        process_name: Some("gnome-terminal".to_string()),
        process_id: Some(123),
    };

    assert!(super::normalize_desktop_block_event(event).is_err());
}

#[test]
fn records_desktop_block_event_to_sqlite_without_process_id() {
    crate::test_support::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(
                "CREATE TABLE pomodoro_runs (id TEXT PRIMARY KEY);
                 CREATE TABLE pomodoro_segments (
                   id TEXT PRIMARY KEY,
                   run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE CASCADE
                 );
                 CREATE TABLE distractions_block_events (
                   id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
                   run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
                   segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
                   occurred_at TEXT NOT NULL CHECK (trim(occurred_at) <> ''),
                   source_type TEXT NOT NULL CHECK (source_type IN ('browser', 'desktop_app', 'mobile_app')),
                   source_key TEXT NOT NULL CHECK (trim(source_key) <> '' AND instr(source_key, '://') = 0),
                   display_name TEXT,
                   phase TEXT CHECK (
                     phase IS NULL OR
                     phase IN ('focus', 'short_break', 'long_break', 'manual_pause', 'idle_pause', 'suspend_pause')
                   ),
                   decision TEXT NOT NULL CHECK (
                     decision IN ('blocked', 'temporary_allowed', 'false_positive_reported', 'limit_exhausted')
                   ),
                   rule_id TEXT,
                   category_id TEXT,
                   created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 CREATE TABLE distractions_block_event_rule_snapshots (
                   block_event_id TEXT PRIMARY KEY REFERENCES distractions_block_events(id) ON DELETE CASCADE,
                   rule_id TEXT,
                   rule_kind TEXT CHECK (
                     rule_kind IS NULL OR
                     rule_kind IN ('domain', 'url_pattern', 'category', 'custom_category', 'usage_limit', 'desktop_app')
                   ),
                   rule_label TEXT,
                   environment_id TEXT,
                   blocker_mode TEXT CHECK (
                     blocker_mode IS NULL OR
                     blocker_mode IN ('blacklist', 'whitelist', 'limit')
                   )
                 );",
            )
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO pomodoro_runs (id) VALUES ('run-1')")
            .execute(&pool)
            .await
            .unwrap();
        let mut runtime = state("focus");
        runtime.active_run_id = Some("run-1".to_string());
        let event =
            super::normalize_desktop_block_event(super::DistractionsDesktopBlockEventInput {
                app_name: "Steam".to_string(),
                process_name: Some("steam".to_string()),
                process_id: Some(42),
            })
            .unwrap();

        super::insert_desktop_block_event(&pool, event, Some(&runtime), "2026-06-10T12:00:00.000Z")
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT run_id, source_type, source_key, display_name, phase, decision
                 FROM distractions_block_events",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let source_key: String = row.try_get("source_key").unwrap();
        assert_eq!(row.try_get::<String, _>("run_id").unwrap(), "run-1");
        assert_eq!(
            row.try_get::<String, _>("source_type").unwrap(),
            "desktop_app"
        );
        assert_eq!(source_key, "steam");
        assert!(!source_key.contains("://"));
        assert_eq!(row.try_get::<String, _>("display_name").unwrap(), "Steam");
        assert_eq!(row.try_get::<String, _>("phase").unwrap(), "focus");
        assert_eq!(row.try_get::<String, _>("decision").unwrap(), "blocked");

        let snapshot = sqlx::query(
            "SELECT rule_kind, rule_label, blocker_mode
                 FROM distractions_block_event_rule_snapshots",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            snapshot.try_get::<String, _>("rule_kind").unwrap(),
            "desktop_app"
        );
        assert_eq!(
            snapshot.try_get::<String, _>("rule_label").unwrap(),
            "Steam"
        );
        assert_eq!(
            snapshot.try_get::<String, _>("blocker_mode").unwrap(),
            "blacklist"
        );
    });
}

#[test]
fn usage_samples_normalize_website_hosts() {
    let sample = DistractionsUsageSampleInput {
        id: Some("sample-1".to_string()),
        source_type: "website".to_string(),
        source_key: "YouTube.com.".to_string(),
        display_name: Some("YouTube".to_string()),
        started_at_ms: 1_779_923_600_000,
        elapsed_seconds: 30,
        local_date: "2026-05-28".to_string(),
    };

    let normalized = super::normalize_usage_sample(sample, "test").unwrap();
    assert_eq!(normalized.source_key, "youtube.com");
    assert_eq!(normalized.elapsed_seconds, 30);
}

#[test]
fn usage_sample_fallback_ids_are_stable_for_exactly_once_batch_retries() {
    let sample = DistractionsUsageSampleInput {
        id: None,
        source_type: "desktop-app".to_string(),
        source_key: "Steam".to_string(),
        display_name: Some("Steam".to_string()),
        started_at_ms: 1_720_000_000_000,
        elapsed_seconds: 5,
        local_date: "2026-07-11".to_string(),
    };
    let first = super::normalize_usage_sample(sample.clone(), "app").unwrap();
    let retry = super::normalize_usage_sample(sample, "app").unwrap();
    assert_eq!(first.id, retry.id);
}

#[test]
fn usage_sample_batch_rolls_back_when_a_late_insert_fails() {
    crate::test_support::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE distractions_usage_samples (
                id TEXT PRIMARY KEY,
                source_type TEXT NOT NULL,
                source_key TEXT NOT NULL,
                display_name TEXT,
                started_at_ms INTEGER NOT NULL,
                elapsed_seconds INTEGER NOT NULL CHECK (elapsed_seconds < 100),
                local_date TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_second_usage_sample
             BEFORE INSERT ON distractions_usage_samples
             WHEN NEW.id = 'second'
             BEGIN
               SELECT RAISE(ABORT, 'late batch failure');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();
        let sample = |id: &str, elapsed_seconds| {
            normalize_usage_sample(
                DistractionsUsageSampleInput {
                    id: Some(id.to_string()),
                    source_type: "website".to_string(),
                    source_key: "example.com".to_string(),
                    display_name: None,
                    started_at_ms: 1,
                    elapsed_seconds,
                    local_date: "2026-05-26".to_string(),
                },
                "test",
            )
            .unwrap()
        };

        assert!(
            insert_usage_samples(&pool, vec![sample("first", 10), sample("second", 200)])
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM distractions_usage_samples")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });
}
