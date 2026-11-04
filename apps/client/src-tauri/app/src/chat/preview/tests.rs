use super::approvals::{ModelNavigation, model_navigation};
use super::data_store::{is_generation_token, orphaned_entries, vault_preview_root};
use super::navigation::{loopback_approval_needed, loopback_origin_key, preview_label};
use super::repository::{persist_tab, require_tab_owner};
use super::{
    BrowserTabBounds, BrowserTabRead, loopback_urls, navigation_allowed, validate_bounds,
    validate_navigation,
};
use ganbaru_chat_contracts::models::{ChatErrorCode, ChatThreadId};
use std::collections::BTreeSet;

fn url(value: &str) -> reqwest::Url {
    value.parse().expect("test URL should parse")
}

fn granted(origins: &[&str]) -> BTreeSet<String> {
    origins.iter().map(|origin| (*origin).to_string()).collect()
}

#[test]
fn loopback_navigation_stays_on_its_port_unless_the_thread_allowed_another() {
    let current = url("http://localhost:5173/");
    let other_port = url("http://localhost:8888/");
    assert!(navigation_allowed(
        &current,
        &url("http://localhost:5173/next"),
        &BTreeSet::new()
    ));
    assert!(!navigation_allowed(&current, &other_port, &BTreeSet::new()));
    assert!(!navigation_allowed(
        &current,
        &other_port,
        &granted(&["http://localhost:9999"])
    ));
    assert!(navigation_allowed(
        &current,
        &other_port,
        &granted(&["http://localhost:8888"])
    ));
    assert!(!navigation_allowed(
        &current,
        &url("https://localhost:8888/"),
        &granted(&["http://localhost:8888"])
    ));
    assert!(!navigation_allowed(
        &current,
        &url("https://example.com/"),
        &granted(&["https://example.com"])
    ));
}

#[test]
fn loopback_host_spellings_share_one_origin_per_scheme_and_port() {
    for value in [
        "http://localhost:5173/a",
        "http://127.0.0.1:5173/b",
        "http://127.8.9.10:5173/",
        "http://[::1]:5173/",
        "http://LOCALHOST:5173/",
    ] {
        assert_eq!(
            loopback_origin_key(&url(value)).as_deref(),
            Some("http://localhost:5173"),
            "{value}"
        );
    }
    assert_eq!(
        loopback_origin_key(&url("http://localhost/")).as_deref(),
        Some("http://localhost:80")
    );
    assert_eq!(loopback_origin_key(&url("https://example.com/")), None);
    assert!(navigation_allowed(
        &url("http://localhost:5173/"),
        &url("http://127.0.0.1:5173/"),
        &BTreeSet::new()
    ));
}

#[test]
fn model_navigation_to_a_loopback_origin_the_thread_did_not_allow_waits_for_approval() {
    let grants = granted(&["http://localhost:5173"]);
    assert_eq!(
        loopback_approval_needed(&url("http://127.0.0.1:8888/admin"), &grants).as_deref(),
        Some("http://localhost:8888")
    );
    assert_eq!(
        loopback_approval_needed(&url("http://127.0.0.1:5173/"), &grants),
        None
    );
    assert_eq!(
        model_navigation("http://localhost:5173/page", &grants).unwrap(),
        ModelNavigation::Ready(url("http://localhost:5173/page"))
    );
    assert_eq!(
        model_navigation("http://localhost:8888/", &grants).unwrap(),
        ModelNavigation::NeedsApproval {
            url: url("http://localhost:8888/"),
            origin: "http://localhost:8888".into(),
        }
    );
    let external = model_navigation("https://example.com/", &grants).unwrap_err();
    assert_eq!(external.code, ChatErrorCode::Permission);
    let pending = super::approvals::approval_pending_error("Navigation to http://localhost:8888");
    assert_eq!(pending.code, ChatErrorCode::Permission);
    assert!(pending.message.contains("Browser panel"));
}

#[test]
fn navigation_requires_external_confirmation_and_stays_within_origin() {
    assert!(validate_navigation("http://127.0.0.1:5173", false).is_ok());
    assert!(validate_navigation("https://example.com", false).is_err());
    let external = validate_navigation("https://example.com/path", true).unwrap();
    assert!(navigation_allowed(
        &external,
        &"https://example.com/next".parse().unwrap(),
        &BTreeSet::new()
    ));
    assert!(!navigation_allowed(
        &external,
        &"https://other.example/".parse().unwrap(),
        &BTreeSet::new()
    ));
    assert!(!navigation_allowed(
        &external,
        &"file:///tmp/secret".parse().unwrap(),
        &BTreeSet::new()
    ));
}

#[test]
fn preview_rejects_non_web_schemes_even_when_confirmed() {
    let allowed = validate_navigation("https://example.com/", true).unwrap();
    for value in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,<p>x</p>",
        "mailto:someone@example.com",
        "vscode://file/tmp/x",
        "tauri://localhost/",
    ] {
        assert!(validate_navigation(value, true).is_err(), "{value}");
        if let Ok(candidate) = value.parse() {
            assert!(
                !navigation_allowed(&allowed, &candidate, &BTreeSet::new()),
                "{value}"
            );
        }
    }
}

#[test]
fn preview_navigation_rejects_tauri_protocol_hosts_even_when_confirmed() {
    let loopback = validate_navigation("http://localhost:5173/", false).unwrap();
    let external = validate_navigation("https://example.com/", true).unwrap();
    for value in [
        "http://tauri.localhost/",
        "http://ipc.localhost/",
        "http://asset.localhost/file",
        "https://tauri.localhost/",
        "http://TAURI.localhost/",
        "http://tauri.localhost./",
        "http://tauri.localhost:80/",
        "http://foo.localhost/",
    ] {
        assert!(validate_navigation(value, true).is_err(), "{value}");
        let candidate = value.parse().unwrap();
        assert!(
            !navigation_allowed(&loopback, &candidate, &BTreeSet::new()),
            "{value}"
        );
        assert!(
            !navigation_allowed(&external, &candidate, &BTreeSet::new()),
            "{value}"
        );
    }

    // WebView2 serves app protocols only on the default port, so explicit
    // ports remain ordinary dev servers behind the external confirmation.
    assert!(validate_navigation("http://app.localhost:3000/", true).is_ok());
    assert!(validate_navigation("http://app.localhost:3000/", false).is_err());
    assert!(validate_navigation("http://localhost/", false).is_ok());
}

#[test]
fn preview_bounds_are_finite_and_bounded() {
    assert!(
        validate_bounds(&BrowserTabBounds {
            x: 0.0,
            y: 0.0,
            width: 320.0,
            height: 240.0
        })
        .is_ok()
    );
    assert!(
        validate_bounds(&BrowserTabBounds {
            x: -1.0,
            y: 0.0,
            width: 320.0,
            height: 240.0
        })
        .is_err()
    );
}

#[test]
fn preview_server_candidates_accept_only_loopback_urls() {
    assert_eq!(
        loopback_urls(
            b"ready at http://localhost:5173/path and http://127.0.0.1:3000, remote https://example.com"
        ),
        vec![
            "http://127.0.0.1:3000".to_string(),
            "http://localhost:5173".to_string()
        ]
    );
}

#[test]
fn preview_data_roots_are_per_vault_under_the_config_directory_and_filesystem_safe() {
    let config = std::path::Path::new("/config");
    let first = vault_preview_root(config, "vault-a");
    let second = vault_preview_root(config, "../vault-b");
    assert!(first.starts_with("/config/chat-previews"));
    assert!(second.starts_with("/config/chat-previews"));
    assert_ne!(first, second);
    assert_eq!(first, vault_preview_root(config, "vault-a"));
    let name = second.file_name().and_then(|name| name.to_str()).unwrap();
    assert_eq!(name.len(), 32);
    assert!(name.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn only_random_hex_tokens_name_preview_data_directories() {
    assert!(is_generation_token("0123456789abcdef0123456789abcdef"));
    assert!(!is_generation_token(""));
    assert!(!is_generation_token("0123456789ABCDEF0123456789ABCDEF"));
    assert!(!is_generation_token("../../../../../../../../../../.."));
    assert!(!is_generation_token("0123456789abcdef0123456789abcde"));
}

#[test]
fn sweeping_removes_only_directories_no_working_folder_references() {
    let referenced = granted(&["aaaa", "bbbb"]);
    let entries = ["aaaa", "cccc", "bbbb", "dddd"].map(String::from);
    assert_eq!(
        orphaned_entries(entries, &referenced),
        vec!["cccc".to_string(), "dddd".to_string()]
    );
}

fn stored_tab(thread_id: &str, url: &str) -> BrowserTabRead {
    BrowserTabRead {
        thread_id: ChatThreadId::new(thread_id).expect("thread id should be valid"),
        tab_id: "tab-shared".to_string(),
        current_url: url.to_string(),
        title: "Preview".to_string(),
        visible: false,
        loading: false,
        viewport_width: 800,
        viewport_height: 600,
        external_origin: false,
    }
}

#[tokio::test]
async fn a_thread_cannot_overwrite_a_stored_tab_that_another_thread_owns() {
    let pool = crate::chat::tests::repository::pool_with_thread().await;
    sqlx::query(
        "INSERT INTO chat_threads
            (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
             continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at)
         SELECT 'thread-2', project_id, working_folder_id, 'Other', provider_family_id,
             provider_instance_id, continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at
         FROM chat_threads WHERE id = 'thread-1'",
    )
    .execute(&pool)
    .await
    .expect("second thread should insert");
    let owner = stored_tab("thread-1", "http://127.0.0.1:5173/");
    persist_tab(&pool, &owner)
        .await
        .expect("owner should store its tab");
    persist_tab(&pool, &stored_tab("thread-1", "http://127.0.0.1:5173/next"))
        .await
        .expect("owner should update its own tab");

    let intruder = stored_tab("thread-2", "http://127.0.0.1:9999/");
    let error = persist_tab(&pool, &intruder)
        .await
        .expect_err("another thread must not overwrite the tab");
    assert_eq!(error.code, ChatErrorCode::Permission);
    let error = require_tab_owner(&pool, &intruder.tab_id, &intruder.thread_id)
        .await
        .expect_err("opening the tab id from another thread must be refused");
    assert_eq!(error.code, ChatErrorCode::Permission);
    require_tab_owner(&pool, &owner.tab_id, &owner.thread_id)
        .await
        .expect("the owner keeps access");
    require_tab_owner(&pool, "tab-unused", &intruder.thread_id)
        .await
        .expect("an unused tab id is free");

    let stored: (String, String) =
        sqlx::query_as("SELECT thread_id, current_url FROM chat_preview_tabs WHERE id = ?")
            .bind(&owner.tab_id)
            .fetch_one(&pool)
            .await
            .expect("stored tab should load");
    assert_eq!(
        stored,
        (
            "thread-1".to_string(),
            "http://127.0.0.1:5173/next".to_string()
        )
    );
}

/// Matches a Tauri capability label pattern, where `*` stands for any run of characters.
fn label_pattern_matches(pattern: &str, label: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = label.strip_prefix(first) else {
        return false;
    };
    let mut segments = parts.collect::<Vec<_>>();
    let Some(last) = segments.pop() else {
        return rest.is_empty();
    };
    for segment in segments {
        match rest.find(segment) {
            Some(index) => rest = &rest[index + segment.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

#[test]
fn capability_label_patterns_match_like_tauri_globs() {
    assert!(label_pattern_matches("main", "main"));
    assert!(!label_pattern_matches("main", "main-2"));
    assert!(label_pattern_matches("view-*", "view-0f1e"));
    assert!(!label_pattern_matches("view-*", "chat-preview-view-0f1e"));
    assert!(label_pattern_matches("*-overlay-*", "pomodoro-overlay-1"));
    assert!(label_pattern_matches("*", "chat-preview-1"));
}

#[test]
fn preview_webviews_get_no_capability_even_on_the_app_dev_origin() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../capabilities");
    let labels = ["tab-1", "main", "view-1", "pomodoro-overlay-1"]
        .map(preview_label)
        .to_vec();
    let mut checked = 0;
    for entry in std::fs::read_dir(&directory).expect("capabilities should be readable") {
        let path = entry.expect("capability entry should read").path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let capability: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("capability should read"))
                .expect("capability should parse");
        let desktop = capability["platforms"].as_array().is_none_or(|platforms| {
            platforms
                .iter()
                .any(|platform| matches!(platform.as_str(), Some("linux" | "macOS" | "windows")))
        });
        if !desktop {
            continue;
        }
        checked += 1;
        assert!(
            capability.get("windows").is_none(),
            "{} selects windows, which would cover preview children of the main window",
            path.display()
        );
        assert!(
            capability.get("remote").is_none(),
            "{} grants remote origins",
            path.display()
        );
        for pattern in capability["webviews"]
            .as_array()
            .expect("desktop capabilities select webviews")
            .iter()
            .filter_map(serde_json::Value::as_str)
        {
            for label in &labels {
                assert!(
                    !label_pattern_matches(pattern, label),
                    "{} pattern {pattern} grants preview webview {label}",
                    path.display()
                );
            }
        }
    }
    assert!(checked >= 2, "desktop capabilities should be checked");
}

#[test]
fn loopback_hosts_are_localhost_127_slash_8_and_ipv6_loopback_only() {
    for value in [
        "http://localhost:3000/",
        "http://LocalHost/",
        "http://127.0.0.2:8080/",
        "http://127.255.255.254/",
        "http://[::1]:5173/",
    ] {
        assert!(super::is_loopback(&url(value)), "{value}");
    }
    for value in [
        "http://localhost.example.com/",
        "http://app.localhost:3000/",
        "http://128.0.0.1/",
        "http://[::ffff:127.0.0.1]/",
        "http://0.0.0.0:3000/",
        "https://example.com/",
    ] {
        assert!(!super::is_loopback(&url(value)), "{value}");
    }
}
