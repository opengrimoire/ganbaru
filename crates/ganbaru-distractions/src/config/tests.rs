use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::codec::{self, OwnedRow};
use ganbaru_sync::manifest::vault::distractions::{
    HOST_RULES_TABLE, LIMIT_ENTRIES, custom_category_group, host_group, limit_group,
};
use ganbaru_sync_contracts::{Field, Value};
use serde_json::json;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

use super::recovery::{restore_recovered_category, restore_recovered_limit};
use super::store::{
    HostList, LimitDraft, LimitEntryDraft, RuleEdit, RuleRefusal, RuleTarget, RulesError, apply,
    read,
};
use super::{DistractionsConfig, MAX_HOST_CHARS, MAX_LIMIT_ENTRIES, MAX_USAGE_LIMITS};
use crate::test_support::block_on;

const NOW_MS: i64 = 1_700_000_000_000;

async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory database");
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .expect("apply migrations");
    pool
}

fn entry(id: &str, website_host: &str, desktop_app_name: &str) -> LimitEntryDraft {
    LimitEntryDraft {
        id: id.to_string(),
        name: String::new(),
        color: None,
        website_host: website_host.to_string(),
        mobile_app_name: String::new(),
        mobile_app_package: String::new(),
        desktop_app_name: desktop_app_name.to_string(),
        desktop_app_match_names: Vec::new(),
    }
}

fn limit(name: &str, entries: Vec<LimitEntryDraft>) -> RuleEdit {
    RuleEdit::SaveUsageLimit {
        id: None,
        draft: LimitDraft {
            name: name.to_string(),
            minutes_per_day: Some(30),
            minutes_per_week: None,
            entries,
        },
    }
}

fn refusal(result: Result<DistractionsConfig, RulesError>) -> RuleRefusal {
    match result {
        Err(RulesError::Refused(refusal)) => refusal,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn fresh_vault_assembles_the_settings_defaults() {
    block_on(async {
        let pool = migrated_pool().await;
        let config = read(&pool).await.expect("read rules");
        assert_eq!(config, DistractionsConfig::default());
        let categories = [
            "social-media",
            "streaming",
            "news",
            "sports",
            "porn",
            "gambling",
            "gaming",
            "shopping",
            "dating",
            "trading",
        ]
        .map(|id| json!({ "id": id, "enabled": id != "news" }));
        let switches = json!({
            "enabled": true,
            "blockDuringFocus": true,
            "blockDuringShortBreaks": true,
            "blockDuringLongBreaks": true,
            "pauseDuringFocusPause": true,
            "blockedApps": [],
        });
        assert_eq!(
            serde_json::to_value(&config).expect("encode rules"),
            json!({
                "mode": "blacklist",
                "enabled": true,
                "blockDuringFocus": true,
                "blockDuringShortBreaks": true,
                "blockDuringLongBreaks": true,
                "pauseDuringFocusPause": true,
                "blockedCategories": categories,
                "customCategoryStacks": [],
                "blockedHosts": [],
                "exceptionHosts": [],
                "allowedHosts": [],
                "mobile": switches,
                "desktop": switches,
                "limits": { "enabled": true, "items": [] },
            })
        );
    });
}

#[test]
fn edits_round_trip_and_reset_restores_defaults() {
    block_on(async {
        let pool = migrated_pool().await;
        apply(
            &pool,
            &RuleEdit::SetEnabled {
                target: RuleTarget::Category {
                    id: "news".to_string(),
                },
                enabled: true,
            },
            NOW_MS,
        )
        .await
        .expect("enable news");
        let config = apply(
            &pool,
            &RuleEdit::AddHosts {
                list: HostList::Blocked,
                hosts: vec!["*.Example.COM.".to_string(), "example.com".to_string()],
            },
            NOW_MS,
        )
        .await
        .expect("block host");
        assert!(config.blocked_categories.iter().all(|rule| rule.enabled));
        assert_eq!(config.blocked_hosts.len(), 1);
        assert_eq!(config.blocked_hosts[0].host, "example.com");

        let config = apply(
            &pool,
            &RuleEdit::SaveCustomCategory {
                id: None,
                name: "  Games   and fun ".to_string(),
                hosts: vec!["play.example.com".to_string()],
            },
            NOW_MS,
        )
        .await
        .expect("add custom category");
        assert_eq!(config.custom_category_stacks[0].name, "Games and fun");
        let duplicate = apply(
            &pool,
            &RuleEdit::SaveCustomCategory {
                id: None,
                name: "games AND fun".to_string(),
                hosts: vec!["other.example.com".to_string()],
            },
            NOW_MS,
        )
        .await;
        assert_eq!(refusal(duplicate), RuleRefusal::DuplicateName);

        let config = apply(&pool, &RuleEdit::Reset, NOW_MS)
            .await
            .expect("reset rules");
        assert_eq!(config, DistractionsConfig::default());
    });
}

#[test]
fn removing_and_adding_a_host_again_writes_the_same_row() {
    block_on(async {
        let pool = migrated_pool().await;
        let add = RuleEdit::AddHosts {
            list: HostList::Exception,
            hosts: vec!["docs.example.com".to_string()],
        };
        let target = RuleTarget::Host {
            list: HostList::Exception,
            host: "docs.example.com".to_string(),
        };
        let id_of = |pool: SqlitePool| async move {
            sqlx::query_scalar::<_, String>("SELECT id FROM distractions_host_rules")
                .fetch_one(&pool)
                .await
                .expect("read host rule id")
        };
        apply(&pool, &add, NOW_MS).await.expect("add host");
        let first = id_of(pool.clone()).await;
        apply(&pool, &RuleEdit::Remove(target), NOW_MS)
            .await
            .expect("remove host");
        let config = apply(&pool, &add, NOW_MS + 1)
            .await
            .expect("add host again");
        assert_eq!(config.exception_hosts.len(), 1);
        assert_eq!(id_of(pool).await, first);
    });
}

#[test]
fn rule_writers_refuse_what_blockers_cannot_enforce() {
    block_on(async {
        let pool = migrated_pool().await;
        let protected = apply(
            &pool,
            &RuleEdit::AddDesktopApp {
                name: "Ganbaru".to_string(),
                match_names: Vec::new(),
            },
            NOW_MS,
        )
        .await;
        assert_eq!(refusal(protected), RuleRefusal::ProtectedSource);

        let long_host = format!("{}.com", "a.".repeat(MAX_HOST_CHARS / 2));
        let invalid_host = apply(
            &pool,
            &RuleEdit::AddHosts {
                list: HostList::Blocked,
                hosts: vec!["fine.example.com".to_string(), long_host],
            },
            NOW_MS,
        )
        .await;
        assert_eq!(refusal(invalid_host), RuleRefusal::InvalidHosts);

        let mut no_budget = limit("", vec![entry("e1", "example.com", "")]);
        if let RuleEdit::SaveUsageLimit { draft, .. } = &mut no_budget {
            draft.minutes_per_day = None;
        }
        assert_eq!(
            refusal(apply(&pool, &no_budget, NOW_MS).await),
            RuleRefusal::InvalidBudget
        );
        let shared_source = limit(
            "Video",
            vec![
                entry("e1", "video.example.com", ""),
                entry("e2", "VIDEO.example.com", ""),
            ],
        );
        assert_eq!(
            refusal(apply(&pool, &shared_source, NOW_MS).await),
            RuleRefusal::DuplicateSource
        );
        let unnamed = limit(
            "",
            vec![
                entry("e1", "a.example.com", ""),
                entry("e2", "b.example.com", ""),
            ],
        );
        assert_eq!(
            refusal(apply(&pool, &unnamed, NOW_MS).await),
            RuleRefusal::InvalidName
        );

        let config = read(&pool).await.expect("read rules");
        assert_eq!(config, DistractionsConfig::default());
    });
}

#[test]
fn usage_limit_entries_stay_within_one_replicated_value() {
    block_on(async {
        let pool = migrated_pool().await;
        let long_name = "N".repeat(80);
        let entries = |count: usize| {
            (0..count)
                .map(|index| LimitEntryDraft {
                    name: long_name.clone(),
                    desktop_app_match_names: (0..16)
                        .map(|name| format!("{index}-{name}-{long_name}"))
                        .collect(),
                    ..entry(&format!("entry-{index}"), "", &format!("App {index}"))
                })
                .collect::<Vec<_>>()
        };
        let oversized = apply(&pool, &limit("Apps", entries(400)), NOW_MS).await;
        assert_eq!(refusal(oversized), RuleRefusal::TooLarge);

        let config = apply(&pool, &limit("Apps", entries(40)), NOW_MS)
            .await
            .expect("save limit");
        let saved = &config.limits.items[0];
        assert_eq!(saved.entries.len(), 40);
        assert_eq!(saved.entries[3].id, "entry-3");
        let id = saved.id.clone();

        let config = apply(
            &pool,
            &RuleEdit::Remove(RuleTarget::UsageLimit { id }),
            NOW_MS,
        )
        .await
        .expect("remove limit");
        assert!(config.limits.items.is_empty());
        let orphans: i64 =
            sqlx::query_scalar("SELECT count(*) FROM distractions_usage_limit_entries")
                .fetch_one(&pool)
                .await
                .expect("count entries");
        assert_eq!(orphans, 0);
    });
}

#[test]
fn assembly_skips_rows_another_device_left_incomplete() {
    block_on(async {
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO distractions_usage_limits (id, created_at_ms, name, minutes_per_day)
             VALUES ('lonely', 1, 'Lonely', 30);
             INSERT INTO distractions_custom_categories (id, created_at_ms, name)
             VALUES ('empty', 1, 'Empty');
             INSERT INTO distractions_category_rules (id, enabled) VALUES ('retired', 1);",
        )
        .execute(&pool)
        .await
        .expect("insert partial rows");
        let config = read(&pool).await.expect("read rules");
        assert_eq!(config, DistractionsConfig::default());
    });
}

fn sync_value(fields: Vec<Field>) -> Value {
    Value::new(fields).expect("valid sync value")
}

fn sync_text(text: &str) -> Field {
    Field::Text(text.to_string())
}

fn recovered_host(host: &str, created_at_ms: i64) -> RecoveryDescendant {
    RecoveryDescendant {
        table: HOST_RULES_TABLE,
        row_key: format!("old:{host}"),
        values: vec![
            (
                host_group::RULE,
                sync_value(vec![
                    sync_text("category"),
                    sync_text("deleted"),
                    sync_text(host),
                ]),
            ),
            (
                host_group::CREATED,
                sync_value(vec![Field::Integer(created_at_ms)]),
            ),
        ],
    }
}

#[test]
fn recovery_restores_a_category_under_a_free_name_and_a_limit_with_its_entries() {
    block_on(async {
        let pool = migrated_pool().await;
        let edit = RuleEdit::SaveCustomCategory {
            id: None,
            name: "Social".to_string(),
            hosts: vec!["example.org".to_string()],
        };
        apply(&pool, &edit, NOW_MS).await.expect("save category");
        let mut conn = pool.acquire().await.expect("acquire connection");

        let category = vec![
            (
                custom_category_group::NAME,
                sync_value(vec![sync_text("social")]),
            ),
            (
                custom_category_group::ENABLED,
                sync_value(vec![Field::Integer(0)]),
            ),
        ];
        let hosts = vec![
            recovered_host("b.example", 20),
            recovered_host("a.example", 10),
        ];
        let category_id = restore_recovered_category(&mut conn, &category, &hosts, NOW_MS)
            .await
            .expect("restore category");

        let entries = vec![OwnedRow::flat(vec![
            sync_text("entry"),
            Field::Null,
            Field::Integer(3),
            sync_text("video.example"),
            Field::Null,
            Field::Null,
            sync_text("Player"),
            sync_text(r#"["player"]"#),
        ])];
        let limit = vec![
            (limit_group::NAME, sync_value(vec![sync_text("Video")])),
            (
                limit_group::BUDGET,
                sync_value(vec![Field::Integer(45), Field::Null]),
            ),
            (
                limit_group::ENTRIES,
                sync_value(vec![Field::Blob(
                    codec::encode(&LIMIT_ENTRIES, &entries).expect("encode entries"),
                )]),
            ),
        ];
        let limit_id = restore_recovered_limit(&mut conn, &limit, NOW_MS)
            .await
            .expect("restore limit");
        drop(conn);

        let config = read(&pool).await.expect("read rules");
        let restored = config
            .custom_category_stacks
            .iter()
            .find(|stack| stack.id == category_id)
            .expect("restored category");
        assert_eq!(restored.name, "social 2");
        assert!(!restored.enabled);
        let restored_hosts: Vec<&str> = restored
            .hosts
            .iter()
            .map(|rule| rule.host.as_str())
            .collect();
        assert_eq!(restored_hosts, vec!["a.example", "b.example"]);

        let [restored_limit] = config.limits.items.as_slice() else {
            panic!("expected one limit, got {:?}", config.limits.items);
        };
        assert_eq!(restored_limit.id, limit_id);
        assert_eq!(restored_limit.name, "Video");
        assert!(restored_limit.enabled);
        assert_eq!(restored_limit.minutes_per_day, Some(45));
        assert_eq!(restored_limit.minutes_per_week, None);
        let [entry] = restored_limit.entries.as_slice() else {
            panic!("expected one entry, got {:?}", restored_limit.entries);
        };
        assert_eq!(entry.website_host.as_deref(), Some("video.example"));
        assert_eq!(entry.desktop_app_name.as_deref(), Some("Player"));
        assert_eq!(entry.desktop_app_match_names, vec!["Player"]);
        assert_eq!(entry.color, Some(3));
    });
}

async fn insert_desktop_rule(pool: &SqlitePool, identity: &str, name: &str, match_names: &str) {
    sqlx::query(
        "INSERT INTO distractions_app_rules
             (id, platform, identity, display_name, match_names_json, enabled, created_at_ms)
         VALUES (?, 'desktop', ?, ?, ?, 1, ?)",
    )
    .bind(format!("rule-{identity}"))
    .bind(identity)
    .bind(name)
    .bind(match_names)
    .bind(NOW_MS)
    .execute(pool)
    .await
    .expect("insert desktop rule");
}

/// Switches the database in or out of the state apply runs in, past the local value checks.
async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(applying)
        .execute(pool)
        .await
        .expect("set applying");
}

async fn insert_replicated_limit(pool: &SqlitePool, id: &str, name: &str, enabled: bool) {
    sqlx::query(
        "INSERT INTO distractions_usage_limits (id, created_at_ms, name, enabled, minutes_per_day)
         VALUES (?, ?, ?, ?, 30)",
    )
    .bind(id)
    .bind(NOW_MS)
    .bind(name)
    .bind(enabled)
    .execute(pool)
    .await
    .expect("insert usage limit");
}

/// Sources of a replicated limit entry row.
#[derive(Clone, Copy, Default)]
struct ReplicatedSources<'a> {
    website_host: Option<&'a str>,
    mobile_app_name: Option<&'a str>,
    mobile_app_package: Option<&'a str>,
    desktop_app_name: Option<&'a str>,
    match_names: Option<&'a str>,
}

async fn insert_replicated_entry(
    pool: &SqlitePool,
    limit_id: &str,
    ordinal: i64,
    id: &str,
    sources: ReplicatedSources<'_>,
) {
    sqlx::query(
        "INSERT INTO distractions_usage_limit_entries
             (limit_id, ordinal, id, website_host, mobile_app_name, mobile_app_package,
              desktop_app_name, desktop_app_match_names_json)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(limit_id)
    .bind(ordinal)
    .bind(id)
    .bind(sources.website_host)
    .bind(sources.mobile_app_name)
    .bind(sources.mobile_app_package)
    .bind(sources.desktop_app_name)
    .bind(sources.match_names.unwrap_or("[]"))
    .execute(pool)
    .await
    .expect("insert usage limit entry");
}

#[test]
fn read_entries_always_pass_parse_config() {
    block_on(async {
        let pool = migrated_pool().await;
        let valid = ReplicatedSources {
            website_host: Some("video.example.com"),
            ..ReplicatedSources::default()
        };
        let blank_host = ReplicatedSources {
            website_host: Some(" "),
            ..ReplicatedSources::default()
        };
        let blank_match_name = ReplicatedSources {
            desktop_app_name: Some("Steam"),
            match_names: Some(r#"[""]"#),
            ..ReplicatedSources::default()
        };
        let invalid_package = ReplicatedSources {
            mobile_app_name: Some("Video"),
            mobile_app_package: Some("not a package"),
            ..ReplicatedSources::default()
        };
        set_applying(&pool, true).await;
        for (id, enabled) in [("disabled-limit", false), ("enabled-limit", true)] {
            insert_replicated_limit(&pool, id, "Video", enabled).await;
            insert_replicated_entry(&pool, id, 0, "blank-host", blank_host).await;
            insert_replicated_entry(&pool, id, 1, "blank-match", blank_match_name).await;
            insert_replicated_entry(&pool, id, 2, "bad-package", invalid_package).await;
            insert_replicated_entry(&pool, id, 3, "valid", valid).await;
        }
        insert_replicated_limit(&pool, "only-blank", "Video", true).await;
        insert_replicated_entry(&pool, "only-blank", 0, "blank-host", blank_host).await;
        let long_name = format!("{}Video", " ".repeat(600));
        insert_replicated_limit(&pool, "long-name", &long_name, true).await;
        insert_replicated_entry(&pool, "long-name", 0, "valid", valid).await;
        set_applying(&pool, false).await;

        let config = read(&pool).await.expect("read rules");
        let root = config.to_root_value().expect("encode rules");
        let parsed = crate::limits::parse_config(&root);
        assert!(
            parsed.is_ok(),
            "one row failed the whole configuration: {parsed:?}"
        );
        let limits = parsed.unwrap().items;
        let kept: Vec<(&str, bool, Vec<&str>)> = limits
            .iter()
            .map(|limit| {
                let entries = limit.entries.iter().map(|entry| entry.id.as_str());
                (limit.id.as_str(), limit.enabled, entries.collect())
            })
            .collect();
        assert_eq!(
            kept,
            [
                ("disabled-limit", false, vec!["valid"]),
                ("enabled-limit", true, vec!["valid"]),
            ]
        );
    });
}

#[test]
fn hidden_limit_rows_leave_the_limit_cap_free() {
    block_on(async {
        let pool = migrated_pool().await;
        let website = ReplicatedSources {
            website_host: Some("video.example.com"),
            ..ReplicatedSources::default()
        };
        set_applying(&pool, true).await;
        for index in 0..MAX_USAGE_LIMITS {
            insert_replicated_limit(&pool, &format!("hidden-{index}"), "Hidden", true).await;
        }
        insert_replicated_limit(&pool, "misnamed-entries", "Hidden", true).await;
        for ordinal in 0..MAX_LIMIT_ENTRIES {
            let ordinal = i64::try_from(ordinal).expect("ordinal fits");
            let id = format!("-hidden-{ordinal}");
            insert_replicated_entry(&pool, "misnamed-entries", ordinal, &id, website).await;
        }
        set_applying(&pool, false).await;
        assert!(read(&pool).await.expect("read rules").limits.items.is_empty());

        let config = apply(
            &pool,
            &limit("Video", vec![entry("video", "video.example.com", "")]),
            NOW_MS,
        )
        .await
        .expect("hidden rows leave room for a new limit");
        let saved: Vec<&str> = config
            .limits
            .items
            .iter()
            .map(|limit| limit.name.as_str())
            .collect();
        assert_eq!(saved, ["Video"]);
    });
}

async fn desktop_rule_enabled(pool: &SqlitePool, identity: &str) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT enabled FROM distractions_app_rules
         WHERE platform = 'desktop' AND identity = ?",
    )
    .bind(identity)
    .fetch_optional(pool)
    .await
    .expect("read desktop rule")
}

/// The identity the assembled configuration reports for a rule, as Settings sends it back.
async fn desktop_target(pool: &SqlitePool, name: &str) -> RuleTarget {
    let config = read(pool).await.expect("read rules");
    let rule = config
        .desktop
        .blocked_apps
        .iter()
        .find(|rule| rule.name == name)
        .expect("assembled desktop rule");
    RuleTarget::DesktopApp {
        identity: rule.identity.clone(),
    }
}

#[test]
fn desktop_rule_with_divergent_identity_can_be_removed_and_toggled() {
    block_on(async {
        let pool = migrated_pool().await;
        insert_desktop_rule(&pool, "a", "b", "[]").await;
        let target = desktop_target(&pool, "b").await;

        apply(
            &pool,
            &RuleEdit::SetEnabled {
                target: target.clone(),
                enabled: false,
            },
            NOW_MS,
        )
        .await
        .expect("toggle rule");
        assert_eq!(desktop_rule_enabled(&pool, "a").await, Some(0));

        apply(&pool, &RuleEdit::Remove(target), NOW_MS)
            .await
            .expect("remove rule");
        assert_eq!(desktop_rule_enabled(&pool, "a").await, None);
    });
}

#[test]
fn added_desktop_rule_reports_the_derived_identity() {
    block_on(async {
        let pool = migrated_pool().await;
        let config = apply(
            &pool,
            &RuleEdit::AddDesktopApp {
                name: "Steam".to_string(),
                match_names: Vec::new(),
            },
            NOW_MS,
        )
        .await
        .expect("add rule");
        assert_eq!(config.desktop.blocked_apps[0].identity, "steam");
        assert_eq!(config.desktop.blocked_apps[0].name, "Steam");
    });
}

#[test]
fn protected_names_refuse_an_enable_but_never_a_disable() {
    block_on(async {
        let pool = migrated_pool().await;
        insert_desktop_rule(&pool, "bash", "Steam", "[]").await;
        insert_desktop_rule(&pool, "steam-a", "tmux", "[]").await;
        insert_desktop_rule(&pool, "steam-b", "Steam B", r#"["bash", "steam"]"#).await;

        for identity in ["bash", "steam-a", "steam-b"] {
            let toggle = |enabled| RuleEdit::SetEnabled {
                target: RuleTarget::DesktopApp {
                    identity: identity.to_string(),
                },
                enabled,
            };
            apply(&pool, &toggle(false), NOW_MS)
                .await
                .expect("disable rule");
            assert_eq!(desktop_rule_enabled(&pool, identity).await, Some(0));

            apply(&pool, &toggle(true), NOW_MS)
                .await
                .expect("refused enable changes nothing");
            assert_eq!(desktop_rule_enabled(&pool, identity).await, Some(0));
        }
    });
}

#[test]
fn desktop_rules_sharing_a_display_name_stay_separately_addressable() {
    block_on(async {
        let pool = migrated_pool().await;
        insert_desktop_rule(&pool, "first", "Steam", "[]").await;
        insert_desktop_rule(&pool, "second", "Steam", "[]").await;

        apply(
            &pool,
            &RuleEdit::Remove(RuleTarget::DesktopApp {
                identity: "first".to_string(),
            }),
            NOW_MS,
        )
        .await
        .expect("remove one rule");
        assert_eq!(desktop_rule_enabled(&pool, "first").await, None);
        assert_eq!(desktop_rule_enabled(&pool, "second").await, Some(1));
    });
}
