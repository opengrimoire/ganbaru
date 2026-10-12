use super::{
    NativeResponse,
    config::{DistractionsConfig, DistractionsMode, UsageLimitsConfig},
    events::{block_event_phase, record_block_event_in_database},
    rules::{HostDecision, decide_url, host_from_url, host_matches_rule},
    snapshot::{RuntimeState, StateSnapshot, runtime_status_at, should_enforce},
};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::Row;

static APP_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../ganbaru-db/migrations");

fn config() -> DistractionsConfig {
    DistractionsConfig {
        mode: DistractionsMode::Blacklist,
        enabled: true,
        block_during_focus: true,
        block_during_short_breaks: true,
        block_during_long_breaks: true,
        pause_during_focus_pause: true,
        blocked_category_ids: Vec::new(),
        custom_category_stacks: Vec::new(),
        blocked_hosts: vec!["reddit.com".to_string(), "youtube.com".to_string()],
        exception_hosts: vec!["music.youtube.com".to_string()],
        allowed_hosts: vec!["github.com".to_string()],
        limits: UsageLimitsConfig {
            enabled: true,
            items: Vec::new(),
        },
        limit_configuration_digest: Some("native-test-digest".into()),
    }
}

fn response_for_phase(phase: &str) -> NativeResponse {
    NativeResponse {
        message_type: "decision",
        host_name: super::HOST_NAME.to_string(),
        connected: true,
        active: true,
        paused: false,
        pause_reason: None,
        phase: phase.to_string(),
        remaining_seconds: Some(60),
        rules_fingerprint: "test".to_string(),
        blocked: false,
        host: None,
        matched_rule_name: None,
        reason: None,
        environment_name: "Ganbaru AI",
    }
}

fn runtime_for_phase(phase: &str, paused: bool) -> RuntimeState {
    runtime_for_phase_updated_at(
        phase,
        paused,
        super::now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

fn runtime_for_phase_updated_at(phase: &str, paused: bool, updated_at: String) -> RuntimeState {
    RuntimeState {
        active: true,
        paused,
        pause_reason: paused.then(|| "manual".to_string()),
        active_run_id: Some("run-1".to_string()),
        phase: phase.to_string(),
        remaining_seconds: Some(60),
        updated_at,
        valid_until_ms: None,
    }
}

#[test]
fn maps_block_event_metadata() {
    let focus = runtime_for_phase("focus", false);
    let idle_pause = RuntimeState {
        paused: true,
        pause_reason: Some("idle".to_string()),
        ..runtime_for_phase("focus", false)
    };

    assert_eq!(block_event_phase(Some(&focus)).as_deref(), Some("focus"));
    assert_eq!(
        block_event_phase(Some(&idle_pause)).as_deref(),
        Some("idle_pause")
    );
    assert_eq!(
        HostDecision::DailyLimit("YouTube".into()).event_decision(),
        "limit_exhausted"
    );
    assert_eq!(
        HostDecision::CustomCategory("Research traps".into()).rule_kind(),
        Some("custom_category")
    );
    assert_eq!(
        HostDecision::BlockedHost("youtube.com".into()).rule_kind(),
        Some("domain")
    );
}

#[test]
fn decision_reasons_preserve_wire_labels_and_typed_event_metadata() {
    let cases = [
        (HostDecision::Allowed, false, None, None),
        (
            HostDecision::SafetyAllowlist,
            false,
            Some("browser safety allowlist"),
            None,
        ),
        (
            HostDecision::WhitelistedHost("example.com".into()),
            false,
            Some("whitelist: example.com"),
            None,
        ),
        (
            HostDecision::ExceptionHost("example.com".into()),
            false,
            Some("exception: example.com"),
            None,
        ),
        (
            HostDecision::OutsideWhitelist,
            true,
            Some("not in whitelist"),
            Some("domain"),
        ),
        (
            HostDecision::BlockedHost("example.com".into()),
            true,
            Some("blocked host: example.com"),
            Some("domain"),
        ),
        (
            HostDecision::CustomCategory("daily limit: Research".into()),
            true,
            Some("custom stack: daily limit: Research"),
            Some("custom_category"),
        ),
        (
            HostDecision::BuiltInCategory("Social media".into()),
            true,
            Some("category: Social media"),
            Some("category"),
        ),
        (
            HostDecision::DailyLimit("category: Videos".into()),
            true,
            Some("daily limit: category: Videos"),
            Some("usage_limit"),
        ),
        (
            HostDecision::WeeklyLimit("Videos".into()),
            true,
            Some("weekly limit: Videos"),
            Some("usage_limit"),
        ),
    ];
    for (decision, blocked, label, kind) in cases {
        assert_eq!(decision.is_blocked(), blocked);
        assert_eq!(decision.matched_rule_name().as_deref(), label);
        assert_eq!(decision.rule_kind(), kind);
        let is_limit = matches!(
            decision,
            HostDecision::DailyLimit(_) | HostDecision::WeeklyLimit(_)
        );
        assert_eq!(
            decision.event_decision(),
            if is_limit {
                "limit_exhausted"
            } else {
                "blocked"
            }
        );
        assert_eq!(
            decision.blocker_mode(&DistractionsMode::Blacklist),
            if is_limit { "limit" } else { "blacklist" }
        );
        assert_eq!(
            decision.blocker_mode(&DistractionsMode::Whitelist),
            if is_limit { "limit" } else { "whitelist" }
        );
    }
}

#[test]
fn runtime_state_requires_current_nullable_fields() {
    let current = serde_json::json!({
        "active": true,
        "paused": false,
        "pauseReason": null,
        "activeRunId": "run-1",
        "phase": "focus",
        "remainingSeconds": 60,
        "updatedAt": "2026-05-28T12:00:00Z"
    });
    assert!(serde_json::from_value::<super::snapshot::RuntimeState>(current.clone()).is_ok());

    for field in ["pauseReason", "activeRunId", "remainingSeconds"] {
        let mut missing = current.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<super::snapshot::RuntimeState>(missing).is_err());
    }
}

#[test]
fn records_block_event_to_sqlite_without_full_url() {
    let vault_path = std::env::temp_dir().join(format!(
        "ganbaru-ai-native-block-event-{}-{}",
        std::process::id(),
        super::now_epoch_ms()
    ));
    let _ = std::fs::remove_dir_all(&vault_path);
    std::fs::create_dir_all(&vault_path).unwrap();
    let db_path = vault_path.join("ganbaru-ai.sqlite");
    std::fs::File::create(&db_path).unwrap();
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());

    let mut runtime = runtime_for_phase("focus", false);
    runtime.active_run_id = None;
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: Some(vault_path.clone()),
        config: config(),
        runtime: Some(runtime),
        limit_state: None,
    };

    let rejected = record_block_event_in_database(
        &snapshot,
        "2026-06-10T10:00:00.000Z",
        "youtube.com",
        &HostDecision::BlockedHost("youtube.com".into()),
    );
    assert!(rejected.is_err());

    super::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();
        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema
             WHERE type = 'table' AND name = 'distractions_block_events'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(table_count, 0);
        APP_MIGRATOR.run(&pool).await.unwrap();
        pool.close().await;
    });

    let rejected = record_block_event_in_database(
        &snapshot,
        "2026-06-10T10:00:00.000Z",
        "https://youtube.com/watch",
        &HostDecision::BlockedHost("youtube.com".into()),
    );
    assert!(rejected.is_err());

    record_block_event_in_database(
        &snapshot,
        "2026-06-10T10:00:00.000Z",
        "youtube.com",
        &HostDecision::BlockedHost("youtube.com".into()),
    )
    .unwrap();

    super::block_on(async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await
            .unwrap();
        let row = sqlx::query(
            "SELECT e.source_key, e.decision, e.phase, s.rule_kind, s.blocker_mode
                 FROM distractions_block_events e
                 JOIN distractions_block_event_rule_snapshots s ON s.block_event_id = e.id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let source_key: String = row.try_get("source_key").unwrap();
        let decision: String = row.try_get("decision").unwrap();
        let phase: String = row.try_get("phase").unwrap();
        let rule_kind: String = row.try_get("rule_kind").unwrap();
        let blocker_mode: String = row.try_get("blocker_mode").unwrap();
        assert_eq!(source_key, "youtube.com");
        assert_eq!(decision, "blocked");
        assert_eq!(phase, "focus");
        assert_eq!(rule_kind, "domain");
        assert_eq!(blocker_mode, "blacklist");

        pool.close().await;
    });
    let _ = std::fs::remove_dir_all(&vault_path);
}

#[test]
fn usage_samples_are_spooled_outside_the_vault_until_the_app_acknowledges_them() {
    let vault_path = std::env::temp_dir().join(format!(
        "ganbaru-ai-native-usage-sample-{}-{}",
        std::process::id(),
        super::now_epoch_ms()
    ));
    let _ = std::fs::remove_dir_all(&vault_path);
    std::fs::create_dir_all(&vault_path).unwrap();
    let config_path = vault_path.join("device-state");
    std::fs::create_dir_all(&config_path).unwrap();
    std::fs::write(vault_path.join("vault.json"), r#"{"vaultId":"vault-test"}"#).unwrap();
    std::fs::write(
        config_path.join("app-state.json"),
        r#"{"deviceId":"desktop-test","activeVaultPath":null}"#,
    )
    .unwrap();
    let db_path = vault_path.join("ganbaru-ai.sqlite");
    std::fs::File::create(&db_path).unwrap();
    let sample = || super::UsageSample {
        id: "sample-1".to_string(),
        source_type: "website".to_string(),
        source_key: "example.com".to_string(),
        display_name: Some("Example".to_string()),
        started_at_ms: 1_700_000_000_000,
        elapsed_seconds: 30,
        local_date: "2026-06-10".to_string(),
        created_at_ms: 1_700_000_030_000,
    };

    assert!(super::record_usage_sample(None, Some(&vault_path), sample()).is_err());
    super::record_usage_sample(Some(&config_path), Some(&vault_path), sample()).unwrap();

    super::block_on(async {
        let spool_url = format!(
            "sqlite:{}",
            config_path
                .join("distractions-device-spool.sqlite")
                .to_string_lossy()
        );
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&spool_url)
            .await
            .unwrap();
        let row =
            sqlx::query("SELECT vault_id, device_id, elapsed_seconds FROM pending_usage_samples")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.try_get::<String, _>("vault_id").unwrap(), "vault-test");
        assert_eq!(
            row.try_get::<String, _>("device_id").unwrap(),
            "desktop-test"
        );
        assert_eq!(row.try_get::<i64, _>("elapsed_seconds").unwrap(), 30);
        pool.close().await;
    });
    let _ = std::fs::remove_dir_all(&vault_path);
}

#[test]
fn retried_browser_usage_keeps_the_same_device_sample_id() {
    let request = super::NativeRequest {
        message_type: "record_usage".to_string(),
        url: None,
        host: None,
        log_event: None,
        source_type: Some("website".to_string()),
        source_key: Some("example.com".to_string()),
        display_name: Some("Example".to_string()),
        elapsed_seconds: Some(30),
        started_at_ms: Some(1_700_000_000_000),
        local_date: Some("2026-06-10".to_string()),
    };
    let first = super::normalize_usage_sample(&request).unwrap();
    let retry = super::normalize_usage_sample(&request).unwrap();
    assert_eq!(first.id, retry.id);
}

#[test]
fn native_vault_writes_require_stable_local_ownership() {
    let root = std::env::temp_dir().join(format!(
        "ganbaru-ai-native-ownership-{}-{}",
        std::process::id(),
        super::now_epoch_ms()
    ));
    let config = root.join("config");
    let vault = root.join("vault");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(
        config.join("app-state.json"),
        r#"{"deviceId":"desktop","activeVaultPath":null}"#,
    )
    .unwrap();
    std::fs::write(vault.join("vault.json"), r#"{"vaultId":"vault"}"#).unwrap();
    let ownership = |owner: &str| {
        format!(
            r#"{{"schemaVersion":1,"vaults":{{"vault":{{"vaultId":"vault","ownerDeviceId":"{owner}","generation":1,"transferPhase":{{"kind":"stable"}}}}}}}}"#
        )
    };
    std::fs::write(config.join("vault-ownership.json"), ownership("desktop")).unwrap();
    assert!(super::linked_usage::native_vault_is_writable(
        &config,
        Some(&vault)
    ));
    std::fs::write(config.join("vault-ownership.json"), ownership("phone")).unwrap();
    assert!(!super::linked_usage::native_vault_is_writable(
        &config,
        Some(&vault)
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn active_runtime_state_remains_fresh_inside_heartbeat_window() {
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime_for_phase_updated_at(
            "focus",
            false,
            "2026-05-26T00:00:00.000Z".to_string(),
        )),
        limit_state: None,
    };
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:00:44.000Z")
        .unwrap()
        .with_timezone(&Utc);

    let (active, phase, remaining_seconds, reason) = runtime_status_at(&snapshot, checked_at);

    assert!(active);
    assert_eq!(phase, "focus");
    assert_eq!(remaining_seconds, Some(16));
    assert!(reason.is_none());
}

#[test]
fn active_runtime_state_fails_open_after_missed_heartbeat() {
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime_for_phase_updated_at(
            "focus",
            false,
            "2026-05-26T00:00:00.000Z".to_string(),
        )),
        limit_state: None,
    };
    let checked_at = DateTime::parse_from_rfc3339("2026-05-26T00:00:46.000Z")
        .unwrap()
        .with_timezone(&Utc);

    let (active, phase, remaining_seconds, reason) = runtime_status_at(&snapshot, checked_at);

    assert!(!active);
    assert_eq!(phase, "inactive");
    assert_eq!(remaining_seconds, None);
    assert_eq!(reason.as_deref(), Some("runtime state is stale"));
}

#[test]
fn fresh_heartbeat_cannot_extend_accepted_phase_or_owner_validity() {
    let updated = "2026-05-26T00:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let mut snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        limit_state: None,
        runtime: Some(runtime_for_phase_updated_at(
            "focus",
            false,
            updated.to_rfc3339(),
        )),
    };
    snapshot.runtime.as_mut().unwrap().valid_until_ms = Some(updated.timestamp_millis() + 500);
    assert!(runtime_status_at(&snapshot, updated + chrono::Duration::milliseconds(499)).0);
    let expired = runtime_status_at(&snapshot, updated + chrono::Duration::milliseconds(500));
    assert!(!expired.0);
    assert_eq!(expired.1, "inactive");
    assert_eq!(
        expired.3.as_deref(),
        Some("accepted phase validity expired")
    );
    snapshot.runtime.as_mut().unwrap().paused = true;
    assert!(!runtime_status_at(&snapshot, updated + chrono::Duration::milliseconds(500)).0);
    let runtime = snapshot.runtime.as_mut().unwrap();
    runtime.valid_until_ms = None;
    runtime.paused = false;
    runtime.remaining_seconds = Some(1);
    assert!(runtime_status_at(&snapshot, updated + chrono::Duration::milliseconds(999)).0);
    assert!(!runtime_status_at(&snapshot, updated + chrono::Duration::seconds(1)).0);
}

#[test]
fn extracts_host_from_url() {
    assert_eq!(
        host_from_url("https://old.reddit.com/r/all?x=1").as_deref(),
        Some("old.reddit.com")
    );
}

#[test]
fn matches_subdomains_only_on_boundaries() {
    assert!(host_matches_rule("old.reddit.com", "reddit.com"));
    assert!(!host_matches_rule("badreddit.com", "reddit.com"));
}

#[test]
fn reads_enabled_host_rules() {
    let value = serde_json::json!([
        { "host": "Reddit.com", "enabled": true },
        { "host": "youtube.com", "enabled": false },
        { "host": "Docs.GitHub.com", "enabled": true },
        { "host": "reddit.com", "enabled": true }
    ]);
    assert_eq!(
        super::config::read_host_array(Some(&value)),
        vec!["reddit.com".to_string(), "docs.github.com".to_string()]
    );
}

#[test]
fn reads_only_enabled_built_in_categories() {
    let value = serde_json::json!([
        { "id": "social-media", "enabled": true },
        { "id": "streaming", "enabled": false },
        { "id": "news", "enabled": true },
        { "id": "unknown", "enabled": true }
    ]);
    assert_eq!(
        super::config::read_category_array(Some(&value)),
        vec!["social-media".to_string(), "news".to_string()]
    );
}

#[test]
fn reads_default_built_in_categories_without_news() {
    let categories = super::config::read_category_array(None);
    assert!(!categories.contains(&"news".to_string()));
    assert!(categories.contains(&"social-media".to_string()));
    assert!(categories.contains(&"streaming".to_string()));
}

#[test]
fn reads_enabled_custom_category_stacks() {
    let value = serde_json::json!([
        {
            "id": "research-traps",
            "name": "  Research traps  ",
            "enabled": true,
            "hosts": [
                { "host": "news.ycombinator.com", "enabled": true },
                { "host": "reddit.com", "enabled": false }
            ]
        },
        {
            "id": "disabled",
            "name": "Disabled",
            "enabled": false,
            "hosts": [{ "host": "example.com", "enabled": true }]
        }
    ]);
    let stacks = super::config::read_custom_category_stacks(Some(&value));
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].id, "research-traps");
    assert_eq!(stacks[0].name, "Research traps");
    assert_eq!(stacks[0].hosts, vec!["news.ycombinator.com".to_string()]);
}

#[test]
fn predecessor_string_rule_entries_are_not_loaded() {
    let hosts = serde_json::json!(["reddit.com"]);
    let categories = serde_json::json!(["social-media"]);
    let stack = serde_json::json!({
        "id": "old-stack",
        "name": "Old stack",
        "hosts": ["example.com"]
    });

    assert!(super::config::read_host_array(Some(&hosts)).is_empty());
    assert!(super::config::read_category_array(Some(&categories)).is_empty());
    assert!(super::config::read_custom_category_stack(&stack).is_none());
}

#[test]
fn distractions_mode_and_rule_enabled_flags_are_explicit() {
    assert!(super::config::read_mode(&serde_json::json!({})).is_none());
    assert!(super::config::read_host_rule(&serde_json::json!({ "host": "example.com" })).is_none());
    assert!(
        super::config::read_custom_category_stack(&serde_json::json!({
            "id": "stack",
            "name": "Stack",
            "hosts": [{ "host": "example.com", "enabled": true }]
        }))
        .is_none()
    );
}

#[test]
fn lets_exceptions_override_blocked_parent_domains() {
    let decision = decide_url("music.youtube.com", None, &config());
    assert!(!decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("exception: music.youtube.com")
    );
}

#[test]
fn blocks_matching_parent_domain() {
    let decision = decide_url("old.reddit.com", None, &config());
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("blocked host: reddit.com")
    );
}

#[test]
fn blocks_exhausted_daily_website_limits_without_active_pomodoro_rules() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.limits.items = vec![super::config::UsageLimit {
        id: "youtube".to_string(),
        name: "YouTube".to_string(),
        enabled: true,
        minutes_per_day: Some(10),
        minutes_per_week: None,
        entries: vec![super::config::UsageLimitEntry {
            id: "youtube-website".to_string(),
            name: None,
            website_host: Some("youtube.com".to_string()),
            mobile_app_name: None,
            desktop_app_name: None,
        }],
    }];
    let mut limit_state = super::snapshot::LimitState {
        local_date: "2026-05-28".to_string(),
        week_start_local_date: "2026-05-25".to_string(),
        updated_at: super::now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
        database_path: "/tmp/ganbaru-ai-vault/ganbaru-ai.sqlite".to_string(),
        configuration_digest: Some("native-test-digest".into()),
        limits: vec![super::snapshot::LimitStateItem {
            id: "youtube".to_string(),
            period: "day".to_string(),
            window_start_local_date: "2026-05-28".to_string(),
            window_end_local_date: "2026-05-28".to_string(),
            used_seconds: 600,
            limit_seconds: 600,
            remaining_seconds: 0,
            exhausted: true,
        }],
    };

    let decision = super::rules::decide_url_with_limits(
        "music.youtube.com",
        None,
        &config,
        Some(&limit_state),
        false,
    );

    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("daily limit: YouTube")
    );
    let mut increased_budget = config.clone();
    increased_budget.limits.items[0].minutes_per_day = Some(20);
    assert!(
        !super::rules::decide_url_with_limits(
            "music.youtube.com",
            None,
            &increased_budget,
            Some(&limit_state),
            false
        )
        .is_blocked()
    );
    limit_state.configuration_digest = Some("obsolete-configuration".into());
    assert!(
        !super::rules::decide_url_with_limits(
            "music.youtube.com",
            None,
            &config,
            Some(&limit_state),
            false
        )
        .is_blocked()
    );
    limit_state.configuration_digest = None;
    assert!(
        !super::rules::decide_url_with_limits(
            "music.youtube.com",
            None,
            &config,
            Some(&limit_state),
            false
        )
        .is_blocked()
    );
}

#[test]
fn active_focus_rules_win_over_limit_blocks() {
    let mut config = config();
    config.limits.items = vec![super::config::UsageLimit {
        id: "reddit".to_string(),
        name: "Reddit limit".to_string(),
        enabled: true,
        minutes_per_day: Some(10),
        minutes_per_week: None,
        entries: vec![super::config::UsageLimitEntry {
            id: "reddit-website".to_string(),
            name: None,
            website_host: Some("reddit.com".to_string()),
            mobile_app_name: None,
            desktop_app_name: None,
        }],
    }];
    let limit_state = super::snapshot::LimitState {
        local_date: "2026-05-28".to_string(),
        week_start_local_date: "2026-05-25".to_string(),
        updated_at: super::now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
        database_path: "/tmp/ganbaru-ai-vault/ganbaru-ai.sqlite".to_string(),
        configuration_digest: Some("native-test-digest".into()),
        limits: vec![super::snapshot::LimitStateItem {
            id: "reddit".to_string(),
            period: "day".to_string(),
            window_start_local_date: "2026-05-28".to_string(),
            window_end_local_date: "2026-05-28".to_string(),
            used_seconds: 600,
            limit_seconds: 600,
            remaining_seconds: 0,
            exhausted: true,
        }],
    };

    let decision = super::rules::decide_url_with_limits(
        "old.reddit.com",
        None,
        &config,
        Some(&limit_state),
        true,
    );

    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("blocked host: reddit.com")
    );
}

#[test]
fn uses_limit_state_database_path_for_usage_samples() {
    let vault_path = std::path::Path::new("/tmp/ganbaru-ai-vault");
    let limit_state = super::snapshot::LimitState {
        local_date: "2026-05-28".to_string(),
        week_start_local_date: "2026-05-25".to_string(),
        updated_at: super::now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
        database_path: "/tmp/ganbaru-ai-vault/ganbaru-ai.sqlite".to_string(),
        configuration_digest: Some("native-test-digest".into()),
        limits: Vec::new(),
    };

    assert_eq!(
        super::snapshot::usage_db_path(vault_path, Some(&limit_state)),
        vault_path.join("ganbaru-ai.sqlite")
    );
}

#[test]
fn uses_vault_database_path_without_limit_state() {
    let vault_path = std::path::Path::new("/tmp/ganbaru-ai-vault");

    assert_eq!(
        super::snapshot::usage_db_path(vault_path, None),
        vault_path.join("ganbaru-ai.sqlite")
    );
}

#[test]
fn limit_state_requires_the_current_on_disk_shape() {
    let current = serde_json::json!({
        "localDate": "2026-05-28",
        "weekStartLocalDate": "2026-05-25",
        "updatedAt": "2026-05-28T12:00:00Z",
        "databasePath": "/tmp/ganbaru-ai-vault/ganbaru-ai.sqlite",
        "limits": [{
            "id": "reddit",
            "period": "day",
            "windowStartLocalDate": "2026-05-28",
            "windowEndLocalDate": "2026-05-28",
            "usedSeconds": 60,
            "limitSeconds": 600,
            "remainingSeconds": 540,
            "exhausted": false
        }]
    });
    assert!(serde_json::from_value::<super::snapshot::LimitState>(current.clone()).is_ok());

    for field in ["weekStartLocalDate", "databasePath"] {
        let mut missing = current.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<super::snapshot::LimitState>(missing).is_err());
    }
    for field in ["period", "windowStartLocalDate", "windowEndLocalDate"] {
        let mut missing = current.clone();
        missing["limits"][0].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<super::snapshot::LimitState>(missing).is_err());
    }
}

#[test]
fn blocks_enabled_built_in_categories() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.blocked_category_ids = vec!["social-media".to_string()];
    let decision = decide_url("old.reddit.com", None, &config);
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("category: Social media")
    );
}

#[test]
fn blocks_streaming_category_keyword_matches_in_domains() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.blocked_category_ids = vec!["streaming".to_string()];
    let decision = decide_url(
        "watch-anime.example",
        Some("https://watch-anime.example/episode/1"),
        &config,
    );
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("category: Streaming")
    );
}

#[test]
fn blocks_built_in_category_keyword_matches_in_domains() {
    let cases = [
        (
            "local-news.example",
            "https://local-news.example/story",
            "news",
            "category: News",
        ),
        (
            "live-scores.example",
            "https://live-scores.example/game",
            "sports",
            "category: Sports",
        ),
        (
            "online-casino.example",
            "https://online-casino.example/table",
            "gambling",
            "category: Gambling",
        ),
        (
            "mini-game.example",
            "https://mini-game.example/play",
            "gaming",
            "category: Gaming",
        ),
        (
            "shopee.example",
            "https://shopee.example/deals",
            "shopping",
            "category: Shopping",
        ),
        (
            "best-hookup.example",
            "https://best-hookup.example/profile",
            "dating",
            "category: Dating",
        ),
        (
            "crypto-watch.example",
            "https://crypto-watch.example/chart",
            "trading",
            "category: Trading",
        ),
    ];
    for (host, url, category_id, matched_rule_name) in cases {
        let mut config = config();
        config.blocked_hosts.clear();
        config.blocked_category_ids = vec![category_id.to_string()];
        let decision = decide_url(host, Some(url), &config);
        assert!(decision.is_blocked());
        assert_eq!(
            decision.matched_rule_name().as_deref(),
            Some(matched_rule_name)
        );
    }
}

#[test]
fn blocks_porn_category_keyword_matches_in_domains() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.blocked_category_ids = vec!["porn".to_string()];
    let decision = decide_url(
        "example-porn-site.test",
        Some("https://example-porn-site.test/watch"),
        &config,
    );
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("category: Porn")
    );
}

#[test]
fn blocks_porn_category_keyword_matches_in_reddit_subreddits() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.blocked_category_ids = vec!["porn".to_string()];
    let decision = decide_url(
        "old.reddit.com",
        Some("https://old.reddit.com/r/gwstories/comments/123/title"),
        &config,
    );
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("category: Porn")
    );
}

#[test]
fn ignores_reddit_post_titles_for_porn_category_keyword_matching() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.blocked_category_ids = vec!["porn".to_string()];
    let decision = decide_url(
        "reddit.com",
        Some("https://reddit.com/r/productivity/comments/123/nsfw_post_title"),
        &config,
    );
    assert!(!decision.is_blocked());
    assert_eq!(decision.matched_rule_name(), None);
}

#[test]
fn blocks_enabled_custom_category_stacks() {
    let mut config = config();
    config.blocked_hosts.clear();
    config.custom_category_stacks = vec![super::config::CustomCategoryStack {
        id: "research-traps".to_string(),
        name: "Research traps".to_string(),
        hosts: vec!["news.ycombinator.com".to_string()],
    }];
    let decision = decide_url("news.ycombinator.com", None, &config);
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("custom stack: Research traps")
    );
}

#[test]
fn enforces_short_and_long_break_settings_independently() {
    let mut config = config();
    config.block_during_short_breaks = true;
    config.block_during_long_breaks = false;
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config,
        runtime: None,
        limit_state: None,
    };
    let short_break = response_for_phase("short_break");
    let long_break = response_for_phase("long_break");

    assert!(should_enforce(&snapshot, &short_break));
    assert!(!should_enforce(&snapshot, &long_break));
}

#[test]
fn enforces_focus_independently_from_break_toggles() {
    let mut config = config();
    config.block_during_focus = false;
    config.block_during_short_breaks = true;
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config,
        runtime: None,
        limit_state: None,
    };
    let focus = response_for_phase("focus");
    let short_break = response_for_phase("short_break");

    assert!(!should_enforce(&snapshot, &focus));
    assert!(should_enforce(&snapshot, &short_break));
}

#[test]
fn skips_paused_focus_when_pause_setting_enabled() {
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime_for_phase("focus", true)),
        limit_state: None,
    };
    let focus = response_for_phase("focus");

    assert!(!should_enforce(&snapshot, &focus));
}

#[test]
fn keeps_enforcing_idle_paused_focus() {
    let mut runtime = runtime_for_phase("focus", true);
    runtime.pause_reason = Some("idle".to_string());
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime),
        limit_state: None,
    };
    let focus = response_for_phase("focus");

    assert!(should_enforce(&snapshot, &focus));
}

#[test]
fn keeps_enforcing_suspend_paused_focus() {
    let mut runtime = runtime_for_phase("focus", true);
    runtime.pause_reason = Some("suspend".to_string());
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime),
        limit_state: None,
    };
    let focus = response_for_phase("focus");

    assert!(should_enforce(&snapshot, &focus));
}

#[test]
fn treats_missing_pause_reason_as_regular_pause() {
    let mut runtime = runtime_for_phase("focus", true);
    runtime.pause_reason = None;
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config: config(),
        runtime: Some(runtime),
        limit_state: None,
    };
    let focus = response_for_phase("focus");

    assert!(!should_enforce(&snapshot, &focus));
}

#[test]
fn enforces_paused_focus_when_pause_setting_disabled() {
    let mut config = config();
    config.pause_during_focus_pause = false;
    let snapshot = StateSnapshot {
        config_dir: None,
        vault_path: None,
        config,
        runtime: Some(runtime_for_phase("focus", true)),
        limit_state: None,
    };
    let focus = response_for_phase("focus");

    assert!(should_enforce(&snapshot, &focus));
}

#[test]
fn blocks_hosts_outside_whitelist_mode() {
    let mut config = config();
    config.mode = DistractionsMode::Whitelist;
    let decision = decide_url("reddit.com", None, &config);
    assert!(decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("not in whitelist")
    );
}

#[test]
fn allows_hosts_inside_whitelist_mode() {
    let mut config = config();
    config.mode = DistractionsMode::Whitelist;
    let decision = decide_url("docs.github.com", None, &config);
    assert!(!decision.is_blocked());
    assert_eq!(
        decision.matched_rule_name().as_deref(),
        Some("whitelist: github.com")
    );
}

#[test]
fn rules_fingerprint_changes_when_mode_or_rules_change() {
    let mut changed_config = config();
    let base = super::rules::rules_fingerprint(&changed_config, None);

    changed_config.mode = DistractionsMode::Whitelist;
    assert_ne!(super::rules::rules_fingerprint(&changed_config, None), base);

    changed_config.mode = DistractionsMode::Blacklist;
    changed_config
        .blocked_hosts
        .push("news.ycombinator.com".to_string());
    assert_ne!(super::rules::rules_fingerprint(&changed_config, None), base);

    let mut category_config = config();
    category_config
        .blocked_category_ids
        .push("news".to_string());
    assert_ne!(
        super::rules::rules_fingerprint(&category_config, None),
        base
    );
}
