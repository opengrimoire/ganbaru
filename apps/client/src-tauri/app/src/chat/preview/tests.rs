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
