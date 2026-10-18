use super::approvals::{ModelNavigation, model_navigation};
use super::navigation::{loopback_approval_needed, loopback_origin_key};
use super::{
    BrowserTabBounds, loopback_urls, navigation_allowed, validate_bounds, validate_navigation,
};
use ganbaru_chat_contracts::models::ChatErrorCode;
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
