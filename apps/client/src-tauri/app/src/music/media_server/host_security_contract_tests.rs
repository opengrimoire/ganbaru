use super::*;
use std::io::Cursor;

fn bytes_media(bytes: &[u8]) -> HostedMedia {
    HostedMedia::Bytes {
        content_type: "image/png".to_string(),
        bytes: Arc::new(bytes.to_vec()),
    }
}

const TEST_HOST: &str = "127.0.0.1:1234";

/// A host whose media token is the page token with a `-media` suffix.
fn test_shared(token: &str) -> MusicHostShared {
    MusicHostShared {
        host: TEST_HOST.to_string(),
        page_token: token.to_string(),
        media_token: format!("{token}-media"),
        registry: Arc::new(Mutex::new(MusicMediaRegistry::with_limits(8, 1024))),
        media_streams: Arc::new(MusicHostConnectionLimit::new(2)),
    }
}

fn test_state(token: &str) -> MusicHostState {
    MusicHostState {
        youtube_url: format!("http://{TEST_HOST}/youtube-player.html?token={token}"),
        media_base_url: format!("http://{TEST_HOST}"),
        page_token: token.to_string(),
        media_token: format!("{token}-media"),
        media_id_key: format!("{token}-key"),
        registry: Arc::new(Mutex::new(MusicMediaRegistry::with_limits(8, 1024))),
    }
}

fn request(path: &str, host: Option<&str>) -> HttpRequest {
    HttpRequest {
        method: "GET".to_string(),
        path: path.to_string(),
        host: host.map(str::to_string),
        range: None,
    }
}

fn respond(shared: &MusicHostShared, request: &HttpRequest) -> String {
    let mut response = Vec::new();
    handle_music_host_request(&mut response, request, shared);
    String::from_utf8(response).unwrap()
}

struct SlowHeaderReader {
    sent_prefix: bool,
}

impl Read for SlowHeaderReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.sent_prefix {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        self.sent_prefix = true;
        let prefix = b"GET /youtube-player.html HTTP/1.1\r\nHost: localhost\r\n";
        buffer[..prefix.len()].copy_from_slice(prefix);
        Ok(prefix.len())
    }
}

#[test]
fn music_host_security_contract_rejects_invalid_tokens() {
    assert!(constant_time_token_eq("aabbcc", "aabbcc"));
    assert!(!constant_time_token_eq("aabbcc", "aabbcd"));
    assert!(!constant_time_token_eq("aabbcc", "aabbcc00"));
}

#[test]
fn music_host_security_contract_generates_secure_token_shape() {
    let token = make_music_host_token().unwrap();
    assert_eq!(token.len(), MUSIC_HOST_TOKEN_BYTES * 2);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn music_host_security_contract_hides_invalid_token_and_returns_hardened_headers() {
    let shared = test_shared("correct-token");
    let request = HttpRequest {
        method: "GET".to_string(),
        path: "/youtube-player.html?token=attacker-token".to_string(),
        host: Some(TEST_HOST.to_string()),
        range: None,
    };
    let mut response = Vec::new();
    handle_music_host_request(&mut response, &request, &shared);
    let response = String::from_utf8(response).unwrap();

    assert!(response.starts_with("HTTP/1.1 404 Not Found\r\n"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("Content-Security-Policy: default-src 'none'"));
    assert!(response.contains("Referrer-Policy: strict-origin-when-cross-origin\r\n"));
    assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
    assert!(!response.contains("correct-token"));
    assert!(!response.contains("attacker-token"));
}

#[test]
fn music_host_security_contract_validates_lifecycle_urls_without_exposing_tokens() {
    let state = test_state("correct-token");
    let valid_url = hosted_media_url(&state, "media-id");
    assert_eq!(
        hosted_media_ids(&state, vec![valid_url]).unwrap(),
        HashSet::from(["media-id".to_string()]),
    );

    let error = hosted_media_ids(
        &state,
        vec!["http://127.0.0.1:1234/media/media-id?token=attacker-token".to_string()],
    )
    .unwrap_err();
    assert_eq!(error, "invalid hosted media URL");
    assert!(!error.contains("correct-token"));
    assert!(!error.contains("attacker-token"));
    assert!(
        hosted_media_ids(
            &state,
            vec![
                state
                    .youtube_url
                    .replace("youtube-player.html", "media/media-id")
            ],
        )
        .is_err()
    );
}

#[test]
fn music_host_security_contract_page_token_never_authorizes_media() {
    let shared = test_shared("page-token");
    shared
        .registry
        .lock()
        .unwrap()
        .register("media-id".to_string(), bytes_media(&[1, 2, 3]), 1)
        .unwrap();

    let page_token_media = respond(
        &shared,
        &request("/media/media-id?token=page-token", Some(TEST_HOST)),
    );
    assert!(page_token_media.starts_with("HTTP/1.1 404 Not Found\r\n"));
    let media_token_page = respond(
        &shared,
        &request(
            "/youtube-player.html?token=page-token-media",
            Some(TEST_HOST),
        ),
    );
    assert!(media_token_page.starts_with("HTTP/1.1 404 Not Found\r\n"));
    let media = respond(
        &shared,
        &request("/media/media-id?token=page-token-media", Some(TEST_HOST)),
    );
    assert!(media.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(!media.contains("Access-Control-Allow-Origin"));
}

#[test]
fn music_host_security_contract_rejects_every_other_host_header() {
    let shared = test_shared("page-token");
    for host in [
        None,
        Some("localhost:1234"),
        Some("127.0.0.1"),
        Some("127.0.0.1:1235"),
        Some("rebind.attacker.test:1234"),
    ] {
        let response = respond(
            &shared,
            &request("/youtube-player.html?token=page-token", host),
        );
        assert!(
            response.starts_with("HTTP/1.1 421 Misdirected Request\r\n"),
            "{host:?}"
        );
        assert!(!response.contains("<!doctype html>"));
    }
    assert_eq!(
        read_http_request(&mut Cursor::new(
            b"GET / HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nHost: evil.test\r\n\r\n".as_slice()
        )),
        Err(HttpRequestError::Malformed),
    );
    assert_eq!(
        read_http_request(&mut Cursor::new(
            b"GET / HTTP/1.1\r\nhost:  127.0.0.1:1234 \r\n\r\n".as_slice()
        ))
        .unwrap()
        .host
        .as_deref(),
        Some(TEST_HOST),
    );
}

#[test]
fn music_host_security_contract_media_ids_are_keyed_per_launch() {
    let state = test_state("page-token");
    let other_launch = test_state("other-token");
    let identity = b"/home/user/Music/song.mp3";
    let id = hosted_media_id(&state, identity);

    assert_eq!(id, hosted_media_id(&state, identity));
    assert_ne!(id, hosted_media_id(&other_launch, identity));
    assert_ne!(id, hosted_media_id(&state, b"/home/user/Music/other.mp3"));
    assert_eq!(id.len(), MUSIC_HOST_MEDIA_ID_BYTES * 2);
    assert!(id.bytes().all(|byte| byte.is_ascii_hexdigit()));
}

#[test]
fn music_host_security_contract_youtube_headers_allow_only_required_remote_sources() {
    let shared = test_shared("correct-token");
    let request = HttpRequest {
        method: "HEAD".to_string(),
        path: "/youtube-player.html?token=correct-token".to_string(),
        host: Some(TEST_HOST.to_string()),
        range: None,
    };
    let mut response = Vec::new();
    handle_music_host_request(&mut response, &request, &shared);
    let response = String::from_utf8(response).unwrap();

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.contains("script-src 'unsafe-inline' https://www.youtube.com"));
    assert!(
        response.contains("frame-src https://www.youtube.com https://www.youtube-nocookie.com")
    );
    assert!(response.ends_with("\r\n\r\n"));
    assert!(!response.contains("<!doctype html>"));
}

#[test]
fn music_host_security_contract_rejects_slow_oversized_and_incomplete_headers() {
    assert_eq!(
        read_http_request(&mut SlowHeaderReader { sent_prefix: false }),
        Err(HttpRequestError::Timeout),
    );
    let mut oversized = Cursor::new(vec![b'a'; MUSIC_HOST_MAX_REQUEST_HEADER_BYTES]);
    assert_eq!(
        read_http_request(&mut oversized),
        Err(HttpRequestError::TooLarge),
    );
    let mut incomplete = Cursor::new(b"GET / HTTP/1.1\r\nHost: localhost\r\n".to_vec());
    assert_eq!(
        read_http_request(&mut incomplete),
        Err(HttpRequestError::Incomplete),
    );
}

#[test]
fn music_host_security_contract_rejects_malformed_request_lines_and_duplicate_ranges() {
    for request in [
        b"GET http://localhost/ HTTP/1.1\r\n\r\n".as_slice(),
        b"GET / HTTP/2\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1 extra\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nRange: bytes=0-1\r\nRange: bytes=2-3\r\n\r\n".as_slice(),
    ] {
        assert_eq!(
            read_http_request(&mut Cursor::new(request)),
            Err(HttpRequestError::Malformed),
        );
    }
}

#[test]
fn music_host_security_contract_preserves_byte_ranges_and_media_headers() {
    let request = HttpRequest {
        method: "GET".to_string(),
        path: "/media/id?token=token".to_string(),
        host: Some(TEST_HOST.to_string()),
        range: Some("bytes=1-3".to_string()),
    };
    let mut response = Vec::new();
    write_media_bytes_response(&mut response, &request, "video/mp4", b"abcdef").unwrap();
    let response = String::from_utf8(response).unwrap();

    assert!(response.starts_with("HTTP/1.1 206 Partial Content\r\n"));
    assert!(response.contains("Content-Range: bytes 1-3/6\r\n"));
    assert!(response.contains("Accept-Ranges: bytes\r\n"));
    assert!(!response.contains("Access-Control-Allow-Origin"));
    assert!(response.contains("Cache-Control: no-store\r\n"));
    assert!(response.contains("Referrer-Policy: no-referrer\r\n"));
    assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
    assert!(response.ends_with("\r\n\r\nbcd"));

    let head_request = HttpRequest {
        method: "HEAD".to_string(),
        path: "/media/id?token=token".to_string(),
        host: Some(TEST_HOST.to_string()),
        range: None,
    };
    let mut head_response = Vec::new();
    write_media_bytes_response(&mut head_response, &head_request, "audio/mpeg", b"abcdef").unwrap();
    let head_response = String::from_utf8(head_response).unwrap();
    assert!(head_response.contains("Content-Length: 6\r\n"));
    assert!(head_response.ends_with("\r\n\r\n"));
}

#[test]
fn music_host_security_contract_caps_concurrent_workers() {
    let limit = Arc::new(MusicHostConnectionLimit::new(2));
    let first = limit.try_acquire().unwrap();
    let second = limit.try_acquire().unwrap();
    assert!(limit.try_acquire().is_none());
    drop(first);
    assert!(limit.try_acquire().is_some());
    drop(second);
}

#[test]
fn music_host_security_contract_reserves_media_streams_for_authorized_requests() {
    let shared = test_shared("page-token");
    shared
        .registry
        .lock()
        .unwrap()
        .register("media-id".to_string(), bytes_media(&[1, 2, 3]), 1)
        .unwrap();
    let media = request("/media/media-id?token=page-token-media", Some(TEST_HOST));
    let held = [
        shared.media_streams.try_acquire().unwrap(),
        shared.media_streams.try_acquire().unwrap(),
    ];

    assert!(respond(&shared, &media).starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    let unauthorized = respond(
        &shared,
        &request("/media/media-id?token=guess", Some(TEST_HOST)),
    );
    assert!(unauthorized.starts_with("HTTP/1.1 404 Not Found\r\n"));
    let page = respond(
        &shared,
        &request("/youtube-player.html?token=page-token", Some(TEST_HOST)),
    );
    assert!(page.starts_with("HTTP/1.1 200 OK\r\n"));

    drop(held);
    assert!(respond(&shared, &media).starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(shared.media_streams.try_acquire().is_some());
}

#[test]
fn music_host_security_contract_bounds_registry_entries() {
    let mut registry = MusicMediaRegistry::with_limits(3, 16);
    registry
        .register(
            "active".to_string(),
            HostedMedia::File(PathBuf::from("/active.mp4")),
            1,
        )
        .unwrap();
    registry
        .register(
            "queue".to_string(),
            HostedMedia::File(PathBuf::from("/queued.mp4")),
            1,
        )
        .unwrap();
    registry.retain(
        1,
        HashSet::from(["active".to_string(), "queue".to_string()]),
    );
    registry
        .register(
            "old".to_string(),
            HostedMedia::File(PathBuf::from("/old.mp4")),
            2,
        )
        .unwrap();
    registry
        .register(
            "new".to_string(),
            HostedMedia::File(PathBuf::from("/new.mp4")),
            2,
        )
        .unwrap();

    assert!(registry.contains("active"));
    assert!(registry.contains("queue"));
    assert!(!registry.contains("old"));
    assert!(registry.contains("new"));
    assert_eq!(registry.len(), 3);
}

#[test]
fn music_host_security_contract_bounds_artwork_bytes_and_retains_active_entry() {
    let mut registry = MusicMediaRegistry::with_limits(4, 6);
    registry
        .register("active".to_string(), bytes_media(&[1, 2, 3, 4]), 1)
        .unwrap();
    registry.retain(1, HashSet::from(["active".to_string()]));
    registry
        .register("old".to_string(), bytes_media(&[5]), 2)
        .unwrap();
    registry
        .register("new".to_string(), bytes_media(&[6, 7]), 2)
        .unwrap();

    assert!(registry.contains("active"));
    assert!(!registry.contains("old"));
    assert!(registry.contains("new"));
    assert_eq!(registry.resident_bytes(), 6);
}

#[test]
fn music_host_security_contract_unregisters_only_matching_generation() {
    let mut registry = MusicMediaRegistry::with_limits(4, 16);
    registry
        .register("media".to_string(), bytes_media(&[1, 2]), 2)
        .unwrap();
    let ids = HashSet::from(["media".to_string()]);
    registry.unregister(1, &ids);
    assert!(registry.contains("media"));
    registry.unregister(2, &ids);
    assert!(!registry.contains("media"));
}

#[test]
fn music_host_security_contract_ignores_stale_retention_requests() {
    let mut registry = MusicMediaRegistry::with_limits(4, 16);
    registry
        .register("current".to_string(), bytes_media(&[1, 2]), 2)
        .unwrap();
    registry.retain(2, HashSet::from(["current".to_string()]));
    registry.retain(1, HashSet::new());
    assert!(registry.contains("current"));
}

#[test]
fn music_host_security_contract_stop_releases_paths_and_artwork_bytes() {
    let mut registry = MusicMediaRegistry::with_limits(4, 16);
    registry
        .register(
            "file".to_string(),
            HostedMedia::File(PathBuf::from("/active.mp4")),
            1,
        )
        .unwrap();
    registry
        .register("artwork".to_string(), bytes_media(&[1, 2, 3, 4]), 1)
        .unwrap();
    registry.retain(
        1,
        HashSet::from(["file".to_string(), "artwork".to_string()]),
    );
    registry.retain(2, HashSet::new());

    assert_eq!(registry.len(), 0);
    assert_eq!(registry.resident_bytes(), 0);
}
