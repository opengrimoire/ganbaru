use super::{
    BrowserTabBounds, loopback_urls, navigation_allowed, validate_bounds, validate_navigation,
};

#[test]
fn navigation_requires_external_confirmation_and_stays_within_origin() {
    assert!(validate_navigation("http://127.0.0.1:5173", false).is_ok());
    assert!(validate_navigation("https://example.com", false).is_err());
    let external = validate_navigation("https://example.com/path", true).unwrap();
    assert!(navigation_allowed(
        &external,
        &"https://example.com/next".parse().unwrap()
    ));
    assert!(!navigation_allowed(
        &external,
        &"https://other.example/".parse().unwrap()
    ));
    assert!(!navigation_allowed(
        &external,
        &"file:///tmp/secret".parse().unwrap()
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
            assert!(!navigation_allowed(&allowed, &candidate), "{value}");
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
        assert!(!navigation_allowed(&loopback, &candidate), "{value}");
        assert!(!navigation_allowed(&external, &candidate), "{value}");
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
