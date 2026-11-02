use super::*;

fn root() -> Value {
    serde_json::json!({"distractions": {"limits": {"enabled": true, "items": [{
        "id": "habit", "name": "Habit", "minutesPerDay": 10, "minutesPerWeek": 20,
        "entries": [
            {"id": "web", "websiteHost": "example.com"},
            {"id": "nested", "websiteHost": "video.example.com"},
            {"id": "desktop", "desktopAppName": "Game", "desktopAppMatchNames": ["game.exe"]},
            {"id": "phone", "mobileAppName": "Old label", "mobileAppPackage": "com.example.video"}
        ]
    }]}}})
}

fn source(kind: &str, key: &str, date: &str, seconds: i64) -> UsageSourceDay {
    UsageSourceDay {
        source_type: kind.into(),
        source_key: key.into(),
        local_date: date.into(),
        elapsed_seconds: seconds,
    }
}

#[test]
fn daily_and_monday_weekly_budgets_allocate_overlapping_hosts_once() {
    let config = parse_config(&root()).unwrap();
    let samples = vec![
        source("website", "video.example.com", "2026-10-02", 400),
        source("desktop-app", "GAME.EXE", "2026-10-02", 150),
        source("mobile-app", "COM.EXAMPLE.VIDEO", "2026-10-02", 50),
        source("website", "example.com", "2026-09-28", 650),
        source("website", "example.com", "2026-09-27", 900),
        source("website", "example.com", "2026-10-03", 900),
        source("website", "otherexample.com", "2026-10-02", 900),
        source("mobile-app", "Old label", "2026-10-02", 900),
    ];
    let totals = totals(&config, &samples, "2026-10-02").unwrap();
    assert_eq!(totals.len(), 2);
    assert_eq!(totals[0].used_seconds, 600);
    assert_eq!(totals[0].remaining_seconds, 0);
    assert!(totals[0].exhausted);
    assert_eq!(totals[0].entries[0].used_seconds, 400);
    assert_eq!(totals[0].entries[1].used_seconds, 0);
    assert_eq!(totals[1].used_seconds, 1250);
    assert_eq!(totals[1].window_start_local_date, "2026-09-28");
    assert_eq!(totals[1].remaining_seconds, 0);
    assert!(totals[1].exhausted);
}

#[test]
fn disabled_limits_remain_visible_and_deleted_limits_leave_history_reusable() {
    let mut value = root();
    value["distractions"]["limits"]["enabled"] = Value::Bool(false);
    value["distractions"]["limits"]["items"][0]["enabled"] = Value::Bool(false);
    let rows = [source("website", "example.com", "2026-10-02", 120)];
    assert_eq!(
        totals(&parse_config(&value).unwrap(), &rows, "2026-10-02").unwrap()[0].used_seconds,
        120
    );
    value["distractions"]["limits"]["items"] = serde_json::json!([]);
    assert!(
        totals(&parse_config(&value).unwrap(), &rows, "2026-10-02")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        totals(&parse_config(&root()).unwrap(), &rows, "2026-10-02").unwrap()[0].used_seconds,
        120
    );
}

#[test]
fn manually_entered_app_names_match_when_no_package_or_process_aliases_are_selected() {
    let value = serde_json::json!({"distractions": {"limits": {"items": [{
        "id": "manual-name", "minutesPerDay": 1,
        "entries": [{"id": "source", "mobileAppName": "Video", "desktopAppName": "Game"}]
    }]}}});
    let config = parse_config(&value).unwrap();
    let rows = [
        source("mobile-app", "VIDEO", "2026-10-02", 20),
        source("desktop-app", "GAME", "2026-10-02", 25),
    ];
    let total = totals(&config, &rows, "2026-10-02").unwrap().remove(0);
    assert_eq!(total.used_seconds, 45);
    assert_eq!(total.remaining_seconds, 15);
    assert!(!total.exhausted);
}

#[test]
fn budgets_and_source_changes_revoke_the_configuration_fingerprint() {
    let value = root();
    let digest = configuration_digest(&value).unwrap();
    let mut changed = value.clone();
    changed["distractions"]["limits"]["items"][0]["minutesPerDay"] = Value::from(20);
    assert_ne!(configuration_digest(&changed).unwrap(), digest);
    changed = value.clone();
    changed["distractions"]["limits"]["items"][0]["entries"][0]["websiteHost"] =
        Value::from("other.com");
    assert_ne!(configuration_digest(&changed).unwrap(), digest);
    changed = value;
    changed["theme"] = Value::from("light");
    assert_eq!(configuration_digest(&changed).unwrap(), digest);
}

#[test]
fn malformed_dates_duplicate_identities_and_invalid_budgets_fail_explicitly() {
    for date in ["2026-02-29", "2026-99-01", "0000-01-01", "2026-1-01"] {
        assert!(week_start(date).is_err());
    }
    assert_eq!(week_start("2024-02-29").unwrap(), "2024-02-26");
    assert_eq!(week_start("2026-01-04").unwrap(), "2025-12-29");
    for minutes in [0, -1, 1_441] {
        let mut value = root();
        value["distractions"]["limits"]["items"][0]["minutesPerDay"] = Value::from(minutes);
        assert!(parse_config(&value).is_err());
    }
    let mut value = root();
    let duplicate = value["distractions"]["limits"]["items"][0].clone();
    value["distractions"]["limits"]["items"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert!(parse_config(&value).is_err());
    let mut value = root();
    value["distractions"]["limits"]["items"][0]["entries"][1]["id"] = Value::from("web");
    assert!(parse_config(&value).is_err());
}

#[test]
fn corrupted_and_oversized_usage_cannot_publish_truncated_or_overflowed_totals() {
    let config = parse_config(&root()).unwrap();
    assert!(
        totals(
            &config,
            &[source("website", "example.com", "2026-10-02", -1)],
            "2026-10-02"
        )
        .is_err()
    );
    let rows = [
        source("website", "example.com", "2026-10-02", MAX_SAFE_SECONDS),
        source("website", "example.com", "2026-10-02", 1),
    ];
    assert!(totals(&config, &rows, "2026-10-02").is_err());
    let rows = vec![source("website", "example.com", "2026-10-02", 1); MAX_SOURCE_GROUPS + 1];
    assert!(totals(&config, &rows, "2026-10-02").is_err());
}

#[tokio::test]
async fn canonical_sqlite_read_groups_the_complete_window_and_retains_recorded_dates() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE distractions_usage_samples (source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);
         CREATE TABLE distractions_usage_totals (device_id TEXT, source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    for _ in 0..250 {
        sqlx::query("INSERT INTO distractions_usage_samples VALUES ('website', 'example.com', '2026-10-02', 2)")
            .execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO distractions_usage_samples VALUES ('website', 'example.com', '2026-09-28', 50), ('website', 'example.com', '2026-09-27', 99)")
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let rows = read_window(&pool).await.unwrap().sources;
    assert_eq!(rows.len(), 2);
    let result = totals(&parse_config(&root()).unwrap(), &rows, "2026-10-02").unwrap();
    assert_eq!(result[0].used_seconds, 500);
    assert_eq!(result[1].used_seconds, 550);
    pool.close().await;
}

#[tokio::test]
async fn budgets_count_other_devices_totals_with_own_samples_but_not_own_totals() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE distractions_usage_samples (source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);
         CREATE TABLE distractions_usage_totals (device_id TEXT, source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);
         INSERT INTO distractions_usage_samples VALUES
            ('website', 'video.example.com', '2026-10-02', 120),
            ('website', 'video.example.com', '2026-10-02', 60);
         INSERT INTO distractions_usage_totals VALUES
            ('device-own', 'website', 'video.example.com', '2026-10-02', 150),
            ('device-laptop', 'website', 'video.example.com', '2026-10-02', 200),
            ('device-phone', 'mobile-app', 'com.example.video', '2026-10-02', 40),
            ('device-phone', 'website', 'example.com', '2026-09-28', 300),
            ('device-phone', 'website', 'example.com', '2026-09-27', 900);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let rows = read_window(&pool).await.unwrap().sources;
    assert_eq!(
        rows,
        vec![
            source("website", "video.example.com", "2026-10-02", 380),
            source("mobile-app", "com.example.video", "2026-10-02", 40),
            source("website", "example.com", "2026-09-28", 300),
        ]
    );
    let result = totals(&parse_config(&root()).unwrap(), &rows, "2026-10-02").unwrap();
    assert_eq!(result[0].used_seconds, 420);
    assert_eq!(result[1].used_seconds, 720);
    pool.close().await;
}

async fn usage_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE distractions_usage_samples (source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);
         CREATE TABLE distractions_usage_totals (device_id TEXT, source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

/// Reads the test window under the budgets of `root`.
async fn read_window(pool: &sqlx::SqlitePool) -> Result<UsageWindow, String> {
    let scope = UsageBudgetScope::from_rules(&parse_config(&root()).unwrap());
    let mut tx = pool.begin().await.unwrap();
    let window = crate::limits::store::read_source_days(
        &mut tx,
        "device-own",
        "2026-09-28",
        "2026-10-02",
        &scope,
    )
    .await;
    tx.commit().await.unwrap();
    window
}

/// Stores `count` totals dated today whose keys follow `pattern`, one distinct key per row.
async fn flood_totals(pool: &sqlx::SqlitePool, pattern: &str, count: usize) {
    sqlx::query(
        "INSERT INTO distractions_usage_totals
         WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
         SELECT 'device-attacker', 'website', printf(?1, i), '2026-10-02', 60 FROM n",
    )
    .bind(pattern)
    .bind(count as i64)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn stored_impossible_date_leaves_the_window_readable() {
    let pool = usage_pool().await;
    sqlx::raw_sql(
        "INSERT INTO distractions_usage_totals VALUES
            ('device-attacker', 'website', 'video.example.com', '2026-09-31', 600),
            ('device-phone', 'website', 'video.example.com', '2026-09-30', 60);
         INSERT INTO distractions_usage_samples VALUES
            ('website', 'video.example.com', '2026-09-31', 600);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let window = read_window(&pool).await.unwrap();
    assert_eq!(
        window.sources,
        vec![source("website", "video.example.com", "2026-09-30", 60)]
    );
    assert!(!window.truncated);
    let config = parse_config(&root()).unwrap();
    assert!(totals(&config, &window.sources, "2026-10-02").is_ok());
    pool.close().await;
}

#[tokio::test]
async fn usage_flood_outside_budgets_reads_no_rows() {
    let pool = usage_pool().await;
    flood_totals(&pool, "flood-%d.attacker.test", MAX_SOURCE_GROUPS + 1).await;
    sqlx::raw_sql(
        "INSERT INTO distractions_usage_totals VALUES
            ('device-phone', 'website', 'video.example.com', '2026-10-02', 60);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let window = read_window(&pool).await;
    assert!(window.is_ok(), "the flood failed the read: {window:?}");
    let window = window.unwrap();
    assert_eq!(
        window.sources,
        vec![source("website", "video.example.com", "2026-10-02", 60)]
    );
    assert!(!window.truncated);
    pool.close().await;
}

#[tokio::test]
async fn usage_flood_inside_budget_truncates_keeping_local_samples() {
    let pool = usage_pool().await;
    flood_totals(&pool, "flood-%d.example.com", MAX_SOURCE_GROUPS + 1).await;
    sqlx::raw_sql(
        "INSERT INTO distractions_usage_samples VALUES
            ('website', 'video.example.com', '2026-09-28', 30);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let window = read_window(&pool).await;
    assert!(window.is_ok(), "the flood failed the read: {window:?}");
    let window = window.unwrap();
    assert!(window.truncated);
    assert_eq!(window.sources.len(), MAX_SOURCE_GROUPS);
    assert_eq!(
        window.sources[0],
        source("website", "video.example.com", "2026-09-28", 30)
    );
    assert_eq!(read_window(&pool).await.unwrap(), window);
    let result = totals(
        &parse_config(&root()).unwrap(),
        &window.sources,
        "2026-10-02",
    )
    .unwrap();
    assert_eq!(result[1].used_seconds, 30 + 60 * 9_999);
    pool.close().await;
}

#[tokio::test]
async fn usage_flood_past_the_sample_bound_keeps_local_samples() {
    let pool = usage_pool().await;
    flood_totals(&pool, "flood-%d.example.com", 50).await;
    sqlx::raw_sql(
        "INSERT INTO distractions_usage_samples VALUES
            ('website', 'video.example.com', '2026-09-28', 30);",
    )
    .execute(&pool)
    .await
    .unwrap();
    let scope = UsageBudgetScope::from_rules(&parse_config(&root()).unwrap());
    let mut connection = pool.acquire().await.unwrap();
    let window = crate::limits::store::read_bounded(
        &mut connection,
        "device-own",
        "2026-09-28",
        "2026-10-02",
        &scope,
        crate::limits::store::WindowBounds {
            samples: 3,
            groups: 100,
        },
    )
    .await
    .unwrap();
    drop(connection);
    assert!(window.truncated);
    assert_eq!(window.sources.len(), 3);
    assert_eq!(
        window.sources[0],
        source("website", "video.example.com", "2026-09-28", 30)
    );
    pool.close().await;
}

#[tokio::test]
async fn usage_window_matches_budgets_the_way_totals_do() {
    let pool = usage_pool().await;
    let long_key = format!("{}.example.com", "a".repeat(MAX_SOURCE_KEY_BYTES));
    sqlx::query(
        "INSERT INTO distractions_usage_totals VALUES
            ('device-phone', 'website', 'VIDEO.example.com', '2026-10-02', 10),
            ('device-phone', 'website', 'sub.video.example.com', '2026-10-02', 11),
            ('device-phone', 'website', 'notexample.com', '2026-10-02', 12),
            ('device-phone', 'website', 'a%b.example_com', '2026-10-02', 13),
            ('device-phone', 'website', ?1, '2026-10-02', 14),
            ('device-phone', 'desktop-app', 'GAME.EXE', '2026-10-02', 15),
            ('device-phone', 'desktop-app', 'game', '2026-10-02', 16),
            ('device-phone', 'mobile-app', 'COM.EXAMPLE.VIDEO', '2026-10-02', 17),
            ('device-phone', 'mobile-app', 'Old label', '2026-10-02', 18),
            ('device-phone', 'other', 'example.com', '2026-10-02', 19);",
    )
    .bind(long_key)
    .execute(&pool)
    .await
    .unwrap();
    let window = read_window(&pool).await.unwrap();
    assert!(!window.truncated);
    let mut kept: Vec<i64> = window
        .sources
        .iter()
        .map(|row| row.elapsed_seconds)
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, vec![10, 11, 15, 17]);
    pool.close().await;
}

#[tokio::test]
async fn usage_window_without_budget_sources_reads_nothing() {
    let pool = usage_pool().await;
    sqlx::raw_sql("DROP TABLE distractions_usage_totals;")
        .execute(&pool)
        .await
        .unwrap();
    let config = parse_config(&serde_json::json!({"distractions": {"limits": {"items": []}}}));
    let scope = UsageBudgetScope::from_rules(&config.unwrap());
    assert!(scope.is_empty());
    let mut connection = pool.acquire().await.unwrap();
    let window = crate::limits::store::read_source_days(
        &mut connection,
        "device-own",
        "2026-09-28",
        "2026-10-02",
        &scope,
    )
    .await
    .unwrap();
    drop(connection);
    assert_eq!(window, UsageWindow::default());
    pool.close().await;
}

#[test]
fn budget_scope_includes_disabled_budgets_and_every_written_form() {
    let config = parse_config(&serde_json::json!({"distractions": {"limits": {"items": [{
        "id": "paused", "name": "Paused", "enabled": false, "minutesPerDay": 10,
        "entries": [
            {"id": "web", "websiteHost": "*.Example.com."},
            {"id": "named", "desktopAppName": "Éditeur"},
            {"id": "listed", "desktopAppName": "Label", "desktopAppMatchNames": [" Game.exe "]},
            {"id": "phone", "mobileAppName": "Label", "mobileAppPackage": "Com.Example"}
        ]
    }]}}}))
    .unwrap();
    let scope = UsageBudgetScope::from_rules(&config);
    assert_eq!(
        scope.websites.iter().collect::<Vec<_>>(),
        vec!["example.com"]
    );
    assert_eq!(
        scope
            .desktop_apps
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Éditeur", "éditeur", " Game.exe ", "game.exe"])
    );
    assert_eq!(
        scope
            .mobile_apps
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Com.Example", "com.example"])
    );
}

#[test]
fn usage_read_diagnostics_log_once_per_problem() {
    let mut diagnostics = UsageReadDiagnostics::default();
    assert!(diagnostics.record_failure("locked"));
    assert!(!diagnostics.record_failure("locked"));
    assert!(diagnostics.record_failure("busy"));
    diagnostics.record_success();
    assert!(diagnostics.record_failure("busy"));
    assert!(diagnostics.record_truncation("2026-10-02"));
    assert!(!diagnostics.record_truncation("2026-10-02"));
    assert!(diagnostics.record_truncation("2026-10-03"));
}

#[test]
fn usage_flood_merge_still_sums_groups_kept_at_the_cap() {
    let rows = (0..MAX_SOURCE_GROUPS)
        .map(|index| source("website", &format!("flood-{index}.test"), "2026-10-02", 60))
        .chain([
            source("website", "flood-0.test", "2026-10-02", 40),
            source("website", "late.test", "2026-10-02", 60),
        ]);
    let window = merge_source_groups(rows).unwrap();
    assert!(window.truncated);
    assert_eq!(window.sources.len(), MAX_SOURCE_GROUPS);
    let kept = window
        .sources
        .iter()
        .find(|row| row.source_key == "flood-0.test")
        .unwrap();
    assert_eq!(kept.elapsed_seconds, 100);
}

#[test]
fn usage_flood_stops_a_merge_at_the_cap_keeping_earlier_rows() {
    let local = source("mobile-app", "com.example.video", "2026-09-28", 30);
    let flood = (0..MAX_SOURCE_GROUPS).map(|index| {
        source(
            "mobile-app",
            &format!("com.flood.app{index}"),
            "2026-10-02",
            60,
        )
    });
    let window = merge_source_groups(std::iter::once(local.clone()).chain(flood));
    assert!(window.is_ok(), "the flood failed the merge: {window:?}");
    let window = window.unwrap();
    assert!(window.truncated);
    assert_eq!(window.sources.len(), MAX_SOURCE_GROUPS);
    assert!(window.sources.contains(&local));
}
