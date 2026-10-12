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
    sqlx::query("CREATE TABLE distractions_usage_samples (source_type TEXT, source_key TEXT, local_date TEXT, elapsed_seconds INTEGER)")
        .execute(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    for _ in 0..250 {
        sqlx::query("INSERT INTO distractions_usage_samples VALUES ('website', 'example.com', '2026-10-02', 2)")
            .execute(&mut *tx).await.unwrap();
    }
    sqlx::query("INSERT INTO distractions_usage_samples VALUES ('website', 'example.com', '2026-09-28', 50), ('website', 'example.com', '2026-09-27', 99)")
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let rows = crate::limits::store::read_source_days(&mut tx, "2026-09-28", "2026-10-02")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(rows.len(), 2);
    let result = totals(&parse_config(&root()).unwrap(), &rows, "2026-10-02").unwrap();
    assert_eq!(result[0].used_seconds, 500);
    assert_eq!(result[1].used_seconds, 550);
    pool.close().await;
}
