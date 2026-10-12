use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(desktop)]
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    fs::File,
    hash::{Hash, Hasher},
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
};

use tauri::{Manager, State};

#[cfg(desktop)]
use super::require_absolute_file;
use super::youtube::player_page::{youtube_host_content_security_policy, youtube_host_html};
#[cfg(desktop)]
use ganbaru_music_library::artwork::{embedded_artwork_id, extract_embedded_artwork};

pub struct MusicHostState {
    pub youtube_url: String,
    #[cfg(desktop)]
    media_base_url: String,
    #[cfg(desktop)]
    token: String,
    #[cfg(desktop)]
    registry: Arc<Mutex<MusicMediaRegistry>>,
}

struct MusicHostShared {
    token: String,
    #[cfg(desktop)]
    registry: Arc<Mutex<MusicMediaRegistry>>,
}

#[cfg(desktop)]
#[derive(Clone)]
enum HostedMedia {
    File(PathBuf),
    Bytes {
        content_type: String,
        bytes: Arc<Vec<u8>>,
    },
}

#[cfg(desktop)]
impl HostedMedia {
    fn resident_bytes(&self) -> usize {
        match self {
            Self::File(_) => 0,
            Self::Bytes { bytes, .. } => bytes.len(),
        }
    }
}

#[cfg(desktop)]
#[derive(Clone)]
struct HostedMediaEntry {
    media: HostedMedia,
    generation: u64,
    last_used: u64,
}

#[cfg(desktop)]
struct MusicMediaRegistry {
    entries: HashMap<String, HostedMediaEntry>,
    retained: HashSet<String>,
    generation: u64,
    clock: u64,
    max_entries: usize,
    max_resident_bytes: usize,
}

#[cfg(desktop)]
impl MusicMediaRegistry {
    fn new() -> Self {
        Self::with_limits(
            MUSIC_HOST_MAX_REGISTRY_ENTRIES,
            MUSIC_HOST_MAX_REGISTRY_BYTES,
        )
    }

    fn with_limits(max_entries: usize, max_resident_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            retained: HashSet::new(),
            generation: 0,
            clock: 0,
            max_entries,
            max_resident_bytes,
        }
    }

    fn accepts_generation(&self, generation: u64) -> bool {
        generation >= self.generation
    }

    fn register(&mut self, id: String, media: HostedMedia, generation: u64) -> Result<(), String> {
        if !self.accepts_generation(generation) {
            return Err("hosted media registration is stale".to_string());
        }
        self.generation = generation;
        self.clock = self.clock.saturating_add(1);
        let old_entry = self.entries.insert(
            id.clone(),
            HostedMediaEntry {
                media,
                generation,
                last_used: self.clock,
            },
        );
        self.evict_unretained_to_limits();
        if self.is_over_limit() || !self.entries.contains_key(&id) {
            self.entries.remove(&id);
            if let Some(old_entry) = old_entry {
                self.entries.insert(id, old_entry);
            } else {
                self.retained.remove(&id);
            }
            self.evict_unretained_to_limits();
            return Err("media host registry is full".to_string());
        }
        Ok(())
    }

    fn retain(&mut self, generation: u64, retained: HashSet<String>) {
        if generation < self.generation {
            return;
        }
        self.generation = generation;
        self.retained = retained
            .into_iter()
            .filter(|id| self.entries.contains_key(id))
            .collect();
        self.entries.retain(|id, _| self.retained.contains(id));
    }

    fn unregister(&mut self, generation: u64, ids: &HashSet<String>) {
        self.entries.retain(|id, entry| {
            let remove = ids.contains(id) && entry.generation == generation;
            if remove {
                self.retained.remove(id);
            }
            !remove
        });
    }

    fn get(&mut self, id: &str) -> Option<HostedMedia> {
        self.clock = self.clock.saturating_add(1);
        let entry = self.entries.get_mut(id)?;
        entry.last_used = self.clock;
        Some(entry.media.clone())
    }

    fn resident_bytes(&self) -> usize {
        self.entries
            .values()
            .map(|entry| entry.media.resident_bytes())
            .fold(0, usize::saturating_add)
    }

    fn is_over_limit(&self) -> bool {
        self.entries.len() > self.max_entries || self.resident_bytes() > self.max_resident_bytes
    }

    fn evict_unretained_to_limits(&mut self) {
        while self.is_over_limit() {
            let candidate = self
                .entries
                .iter()
                .filter(|(id, _)| !self.retained.contains(*id))
                .min_by(|(left_id, left), (right_id, right)| {
                    left.last_used
                        .cmp(&right.last_used)
                        .then_with(|| left_id.cmp(right_id))
                })
                .map(|(id, _)| id.clone());
            let Some(candidate) = candidate else {
                break;
            };
            self.entries.remove(&candidate);
        }
    }

    #[cfg(test)]
    fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    path: String,
    #[cfg(desktop)]
    range: Option<String>,
}

struct DeadlineTcpReader<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}

impl<'a> DeadlineTcpReader<'a> {
    fn new(stream: &'a mut TcpStream, timeout: Duration) -> Self {
        Self {
            stream,
            deadline: Instant::now() + timeout,
        }
    }
}

impl Read for DeadlineTcpReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpRequestError {
    Incomplete,
    Malformed,
    Timeout,
    TooLarge,
}

impl HttpRequestError {
    fn response(self) -> (&'static str, &'static str) {
        match self {
            Self::Incomplete | Self::Malformed => ("400 Bad Request", "Bad request"),
            Self::Timeout => ("408 Request Timeout", "Request timeout"),
            Self::TooLarge => (
                "431 Request Header Fields Too Large",
                "Request headers too large",
            ),
        }
    }
}

#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ByteRange {
    pub(super) start: u64,
    pub(super) end: u64,
}

const MUSIC_HOST_TOKEN_BYTES: usize = 32;
const MUSIC_HOST_MAX_CONNECTIONS: usize = 8;
const MUSIC_HOST_MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MUSIC_HOST_READ_TIMEOUT: Duration = Duration::from_secs(3);
const MUSIC_HOST_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(desktop)]
const MUSIC_HOST_MAX_REGISTRY_ENTRIES: usize = 128;
#[cfg(desktop)]
const MUSIC_HOST_MAX_REGISTRY_BYTES: usize = 64 * 1024 * 1024;
#[cfg(desktop)]
const MUSIC_HOST_MAX_MEDIA_URLS_PER_COMMAND: usize = 128;
const BASIC_RESPONSE_CSP: &str =
    "default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; sandbox";

#[tauri::command]
#[cfg(desktop)]
pub fn music_register_media_file(
    state: State<'_, MusicHostState>,
    path: String,
    generation: u64,
) -> Result<String, String> {
    let path = PathBuf::from(path);
    require_absolute_file(&path)?;
    let id = media_file_id(&path);
    state
        .registry
        .lock()
        .map_err(|_| "media host registry is temporarily unavailable".to_string())?
        .register(id.clone(), HostedMedia::File(path), generation)?;
    Ok(hosted_media_url(&state, &id))
}

#[tauri::command]
#[cfg(desktop)]
pub fn music_register_embedded_artwork(
    state: State<'_, MusicHostState>,
    path: String,
    generation: u64,
) -> Result<Option<String>, String> {
    let path = PathBuf::from(path);
    require_absolute_file(&path)?;
    if !state
        .registry
        .lock()
        .map_err(|_| "media host registry is temporarily unavailable".to_string())?
        .accepts_generation(generation)
    {
        return Err("hosted media registration is stale".to_string());
    }
    let Some(artwork) = extract_embedded_artwork(&path)? else {
        return Ok(None);
    };
    let id = embedded_artwork_id(&path, &artwork);
    state
        .registry
        .lock()
        .map_err(|_| "media host registry is temporarily unavailable".to_string())?
        .register(
            id.clone(),
            HostedMedia::Bytes {
                content_type: artwork.content_type,
                bytes: Arc::new(artwork.bytes),
            },
            generation,
        )?;
    Ok(Some(hosted_media_url(&state, &id)))
}

#[tauri::command]
#[cfg(desktop)]
pub fn music_retain_hosted_media(
    state: State<'_, MusicHostState>,
    media_urls: Vec<String>,
    generation: u64,
) -> Result<(), String> {
    let ids = hosted_media_ids(&state, media_urls)?;
    state
        .registry
        .lock()
        .map_err(|_| "media host registry is temporarily unavailable".to_string())?
        .retain(generation, ids);
    Ok(())
}

#[tauri::command]
#[cfg(desktop)]
pub fn music_unregister_hosted_media(
    state: State<'_, MusicHostState>,
    media_urls: Vec<String>,
    generation: u64,
) -> Result<(), String> {
    let ids = hosted_media_ids(&state, media_urls)?;
    state
        .registry
        .lock()
        .map_err(|_| "media host registry is temporarily unavailable".to_string())?
        .unregister(generation, &ids);
    Ok(())
}

#[tauri::command]
#[cfg(mobile)]
pub fn music_retain_hosted_media(media_urls: Vec<String>, generation: u64) {
    let _ = (media_urls, generation);
}

#[tauri::command]
#[cfg(mobile)]
pub fn music_unregister_hosted_media(media_urls: Vec<String>, generation: u64) {
    let _ = (media_urls, generation);
}

#[tauri::command]
pub fn music_youtube_host_url(state: State<'_, MusicHostState>) -> String {
    state.youtube_url.clone()
}

pub fn setup_youtube_host(app: &tauri::AppHandle) -> Result<(), String> {
    app.manage(spawn_music_host()?);
    Ok(())
}

#[cfg(desktop)]
fn media_file_id(path: &Path) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

#[cfg(desktop)]
fn hosted_media_url(state: &MusicHostState, id: &str) -> String {
    format!("{}/media/{id}?token={}", state.media_base_url, state.token)
}

#[cfg(desktop)]
fn hosted_media_ids(
    state: &MusicHostState,
    media_urls: Vec<String>,
) -> Result<HashSet<String>, String> {
    if media_urls.len() > MUSIC_HOST_MAX_MEDIA_URLS_PER_COMMAND {
        return Err("too many hosted media URLs".to_string());
    }
    let prefix = format!("{}/media/", state.media_base_url);
    media_urls
        .into_iter()
        .map(|url| {
            let suffix = url
                .strip_prefix(&prefix)
                .ok_or_else(|| "invalid hosted media URL".to_string())?;
            let (id, _) = suffix
                .split_once('?')
                .ok_or_else(|| "invalid hosted media URL".to_string())?;
            if id.is_empty() || id.contains('/') {
                return Err("invalid hosted media URL".to_string());
            }
            let token =
                request_token(&url).ok_or_else(|| "invalid hosted media URL".to_string())?;
            if !constant_time_token_eq(&state.token, token) {
                return Err("invalid hosted media URL".to_string());
            }
            Ok(id.to_string())
        })
        .collect()
}

struct MusicHostConnectionLimit {
    active: AtomicUsize,
    max: usize,
}

impl MusicHostConnectionLimit {
    fn new(max: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            max,
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<MusicHostConnectionPermit> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= self.max {
                return None;
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(MusicHostConnectionPermit {
                        limit: Arc::clone(self),
                    });
                }
                Err(next) => active = next,
            }
        }
    }
}

struct MusicHostConnectionPermit {
    limit: Arc<MusicHostConnectionLimit>,
}

impl Drop for MusicHostConnectionPermit {
    fn drop(&mut self) {
        self.limit.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn spawn_music_host() -> Result<MusicHostState, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("failed to bind music player host: {e}"))?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("failed to inspect music player host address: {e}"))?;
    let token = make_music_host_token()?;
    #[cfg(desktop)]
    let registry = Arc::new(Mutex::new(MusicMediaRegistry::new()));
    let shared = Arc::new(MusicHostShared {
        token: token.clone(),
        #[cfg(desktop)]
        registry: Arc::clone(&registry),
    });
    let connection_limit = Arc::new(MusicHostConnectionLimit::new(MUSIC_HOST_MAX_CONNECTIONS));
    let thread_shared = Arc::clone(&shared);
    let thread_connection_limit = Arc::clone(&connection_limit);
    thread::Builder::new()
        .name("music-player-host".to_string())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let Some(permit) = thread_connection_limit.try_acquire() else {
                    reject_saturated_connection(stream);
                    continue;
                };
                let connection_shared = Arc::clone(&thread_shared);
                let spawn_result = thread::Builder::new()
                    .name("music-player-host-request".to_string())
                    .spawn(move || {
                        let _permit = permit;
                        handle_music_host_stream(stream, connection_shared);
                    });
                if spawn_result.is_err() {
                    continue;
                }
            }
        })
        .map_err(|e| format!("failed to start music player host: {e}"))?;

    let base_url = format!("http://{addr}");
    Ok(MusicHostState {
        youtube_url: format!("{base_url}/youtube-player.html?token={token}"),
        #[cfg(desktop)]
        media_base_url: base_url,
        #[cfg(desktop)]
        token,
        #[cfg(desktop)]
        registry,
    })
}

fn make_music_host_token() -> Result<String, String> {
    let provider = rustls::crypto::ring::default_provider();
    let mut bytes = [0_u8; MUSIC_HOST_TOKEN_BYTES];
    provider
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| "failed to generate music host authorization".to_string())?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Ok(token)
}

fn handle_music_host_stream(mut stream: TcpStream, shared: Arc<MusicHostShared>) {
    if stream
        .set_write_timeout(Some(MUSIC_HOST_WRITE_TIMEOUT))
        .is_err()
    {
        return;
    }
    let request = match read_http_request(&mut DeadlineTcpReader::new(
        &mut stream,
        MUSIC_HOST_READ_TIMEOUT,
    )) {
        Ok(request) => request,
        Err(error) => {
            let (status, body) = error.response();
            let _ = write_http_response(
                &mut stream,
                status,
                "text/plain; charset=utf-8",
                body,
                false,
                BASIC_RESPONSE_CSP,
            );
            return;
        }
    };
    handle_music_host_request(&mut stream, &request, &shared);
}

fn reject_saturated_connection(mut stream: TcpStream) {
    let _ = stream.set_write_timeout(Some(MUSIC_HOST_WRITE_TIMEOUT));
    let _ = write_http_response(
        &mut stream,
        "503 Service Unavailable",
        "text/plain; charset=utf-8",
        "Service unavailable",
        false,
        BASIC_RESPONSE_CSP,
    );
}

fn handle_music_host_request<W: Write>(
    stream: &mut W,
    request: &HttpRequest,
    shared: &MusicHostShared,
) {
    let authorized = request_token(&request.path)
        .is_some_and(|token| constant_time_token_eq(&shared.token, token));
    let is_head = request.method == "HEAD";
    if request.method != "GET" && request.method != "HEAD" {
        let _ = write_http_response(
            stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            "Method not allowed",
            false,
            BASIC_RESPONSE_CSP,
        );
        return;
    }
    let route = request.path.split('?').next().unwrap_or_default();
    if route == "/youtube-player.html" && authorized {
        let _ = write_http_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            youtube_host_html(),
            is_head,
            youtube_host_content_security_policy(),
        );
        return;
    }
    #[cfg(desktop)]
    if route.starts_with("/media/") && authorized {
        match media_from_request(request, shared) {
            Ok(media) => {
                if write_hosted_media_response(stream, request, &media).is_err() {
                    let _ = write_http_response(
                        stream,
                        "500 Internal Server Error",
                        "text/plain; charset=utf-8",
                        "Media read failed",
                        is_head,
                        BASIC_RESPONSE_CSP,
                    );
                }
            }
            Err(status) => {
                let _ = write_http_response(
                    stream,
                    status,
                    "text/plain; charset=utf-8",
                    status,
                    is_head,
                    BASIC_RESPONSE_CSP,
                );
            }
        }
        return;
    }
    let _ = write_http_response(
        stream,
        "404 Not Found",
        "text/plain; charset=utf-8",
        "Not found",
        is_head,
        BASIC_RESPONSE_CSP,
    );
}

fn read_http_request<R: Read>(stream: &mut R) -> Result<HttpRequest, HttpRequestError> {
    let mut bytes = Vec::with_capacity(1024);
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if bytes.len() >= MUSIC_HOST_MAX_REQUEST_HEADER_BYTES {
            return Err(HttpRequestError::TooLarge);
        }
        let mut buffer = [0_u8; 1024];
        let read_limit = buffer
            .len()
            .min(MUSIC_HOST_MAX_REQUEST_HEADER_BYTES - bytes.len());
        match stream.read(&mut buffer[..read_limit]) {
            Ok(0) => return Err(HttpRequestError::Incomplete),
            Ok(size) => bytes.extend_from_slice(&buffer[..size]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(HttpRequestError::Timeout);
            }
            Err(_) => return Err(HttpRequestError::Malformed),
        }
    };
    let request =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| HttpRequestError::Malformed)?;
    let mut lines = request.split("\r\n");
    let request_line = lines.next().ok_or(HttpRequestError::Malformed)?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_uppercase()))
        .ok_or(HttpRequestError::Malformed)?
        .to_string();
    let path = request_parts
        .next()
        .filter(|value| value.starts_with('/'))
        .ok_or(HttpRequestError::Malformed)?
        .to_string();
    let version = request_parts.next().ok_or(HttpRequestError::Malformed)?;
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || request_parts.next().is_some() {
        return Err(HttpRequestError::Malformed);
    }
    #[cfg(desktop)]
    let mut range = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(HttpRequestError::Malformed)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() && byte != b'\t')
        {
            return Err(HttpRequestError::Malformed);
        }
        #[cfg(desktop)]
        if name.eq_ignore_ascii_case("range") {
            if range.is_some() {
                return Err(HttpRequestError::Malformed);
            }
            range = Some(value.trim().to_string());
        }
    }
    Ok(HttpRequest {
        method,
        path,
        #[cfg(desktop)]
        range,
    })
}

fn request_token(path: &str) -> Option<&str> {
    let query = path.split_once('?')?.1;
    let mut token = None;
    for part in query.split('&') {
        let (name, value) = part.split_once('=')?;
        if name == "token" {
            if token.is_some() {
                return None;
            }
            token = Some(value);
        }
    }
    token
}

fn constant_time_token_eq(expected: &str, candidate: &str) -> bool {
    let expected = expected.as_bytes();
    let candidate = candidate.as_bytes();
    let mut difference = expected.len() ^ candidate.len();
    for (index, expected_byte) in expected.iter().enumerate() {
        difference |= usize::from(*expected_byte ^ candidate.get(index).copied().unwrap_or(0));
    }
    difference == 0
}

#[cfg(desktop)]
fn media_from_request(
    request: &HttpRequest,
    shared: &MusicHostShared,
) -> Result<HostedMedia, &'static str> {
    let path_without_query = request.path.split('?').next().unwrap_or_default();
    let id = path_without_query
        .strip_prefix("/media/")
        .filter(|value| !value.is_empty())
        .ok_or("404 Not Found")?;
    shared
        .registry
        .lock()
        .map_err(|_| "503 Service Unavailable")?
        .get(id)
        .ok_or("404 Not Found")
}

#[cfg(desktop)]
fn write_media_file_response<W: Write>(
    stream: &mut W,
    request: &HttpRequest,
    path: &Path,
) -> std::io::Result<()> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let content_type = media_content_type(path);
    let Some(range) = request
        .range
        .as_deref()
        .and_then(|value| parse_byte_range(value, len))
    else {
        write_media_headers(stream, "200 OK", content_type, len, None)?;
        if request.method != "HEAD" {
            copy_limited(&mut file, stream, len)?;
        }
        return Ok(());
    };

    write_media_headers(
        stream,
        "206 Partial Content",
        content_type,
        range.end - range.start + 1,
        Some((range, len)),
    )?;
    if request.method != "HEAD" {
        file.seek(SeekFrom::Start(range.start))?;
        copy_limited(&mut file, stream, range.end - range.start + 1)?;
    }
    Ok(())
}

#[cfg(desktop)]
fn write_hosted_media_response<W: Write>(
    stream: &mut W,
    request: &HttpRequest,
    media: &HostedMedia,
) -> std::io::Result<()> {
    match media {
        HostedMedia::File(path) => write_media_file_response(stream, request, path),
        HostedMedia::Bytes {
            content_type,
            bytes,
        } => write_media_bytes_response(stream, request, content_type, bytes.as_slice()),
    }
}

#[cfg(desktop)]
fn write_media_bytes_response<W: Write>(
    stream: &mut W,
    request: &HttpRequest,
    content_type: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let len = bytes.len() as u64;
    let Some(range) = request
        .range
        .as_deref()
        .and_then(|value| parse_byte_range(value, len))
    else {
        write_media_headers(stream, "200 OK", content_type, len, None)?;
        if request.method != "HEAD" {
            stream.write_all(bytes)?;
            stream.flush()?;
        }
        return Ok(());
    };

    write_media_headers(
        stream,
        "206 Partial Content",
        content_type,
        range.end - range.start + 1,
        Some((range, len)),
    )?;
    if request.method != "HEAD" {
        let start = range.start as usize;
        let end = range.end as usize + 1;
        stream.write_all(&bytes[start..end])?;
        stream.flush()?;
    }
    Ok(())
}

#[cfg(desktop)]
fn write_media_headers<W: Write>(
    stream: &mut W,
    status: &str,
    content_type: &str,
    content_length: u64,
    content_range: Option<(ByteRange, u64)>,
) -> std::io::Result<()> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nAccept-Ranges: bytes\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nContent-Security-Policy: {BASIC_RESPONSE_CSP}\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n"
    );
    if let Some((range, len)) = content_range {
        response.push_str(&format!(
            "Content-Range: bytes {}-{}/{}\r\n",
            range.start, range.end, len
        ));
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes())
}

#[cfg(desktop)]
fn copy_limited<W: Write>(
    file: &mut File,
    stream: &mut W,
    mut remaining: u64,
) -> std::io::Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let read_limit =
            usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let bytes_read = file.read(&mut buffer[..read_limit])?;
        if bytes_read == 0 {
            break;
        }
        stream.write_all(&buffer[..bytes_read])?;
        remaining = remaining.saturating_sub(bytes_read as u64);
    }
    stream.flush()
}

#[cfg(desktop)]
pub(super) fn parse_byte_range(header: &str, len: u64) -> Option<ByteRange> {
    if len == 0 {
        return None;
    }
    let range = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start, end) = range.split_once('-')?;
    if start.is_empty() {
        let suffix_len = end.parse::<u64>().ok()?.min(len);
        if suffix_len == 0 {
            return None;
        }
        return Some(ByteRange {
            start: len - suffix_len,
            end: len - 1,
        });
    }
    let start = start.parse::<u64>().ok()?;
    if start >= len {
        return None;
    }
    let end = if end.is_empty() {
        len - 1
    } else {
        end.parse::<u64>().ok()?.min(len - 1)
    };
    if end < start {
        return None;
    }
    Some(ByteRange { start, end })
}

#[cfg(desktop)]
pub(super) fn media_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("aac") => "audio/aac",
        Some("aif") | Some("aiff") => "audio/aiff",
        Some("flac") => "audio/flac",
        Some("m4a") | Some("alac") => "audio/mp4",
        Some("mp3") => "audio/mpeg",
        Some("ogg") | Some("opus") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("wma") => "audio/x-ms-wma",
        Some("avi") => "video/x-msvideo",
        Some("flv") => "video/x-flv",
        Some("m4v") | Some("mp4") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("mpeg") | Some("mpg") => "video/mpeg",
        Some("ogv") => "video/ogg",
        Some("webm") => "video/webm",
        Some("wmv") => "video/x-ms-wmv",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn write_http_response<W: Write>(
    stream: &mut W,
    status: &str,
    content_type: &str,
    body: &str,
    head_only: bool,
    content_security_policy: &str,
) -> io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: {content_security_policy}\r\nReferrer-Policy: strict-origin-when-cross-origin\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    if !head_only {
        stream.write_all(body.as_bytes())?;
    }
    stream.flush()
}

#[cfg(all(test, desktop))]
mod host_security_contract_tests {
    use super::*;
    use std::io::Cursor;

    fn bytes_media(bytes: &[u8]) -> HostedMedia {
        HostedMedia::Bytes {
            content_type: "image/png".to_string(),
            bytes: Arc::new(bytes.to_vec()),
        }
    }

    fn test_shared(token: &str) -> MusicHostShared {
        MusicHostShared {
            token: token.to_string(),
            registry: Arc::new(Mutex::new(MusicMediaRegistry::with_limits(8, 1024))),
        }
    }

    fn test_state(token: &str) -> MusicHostState {
        MusicHostState {
            youtube_url: format!("http://127.0.0.1:1234/youtube-player.html?token={token}"),
            media_base_url: "http://127.0.0.1:1234".to_string(),
            token: token.to_string(),
            registry: Arc::new(Mutex::new(MusicMediaRegistry::with_limits(8, 1024))),
        }
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
    }

    #[test]
    fn music_host_security_contract_youtube_headers_allow_only_required_remote_sources() {
        let shared = test_shared("correct-token");
        let request = HttpRequest {
            method: "HEAD".to_string(),
            path: "/youtube-player.html?token=correct-token".to_string(),
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
            range: Some("bytes=1-3".to_string()),
        };
        let mut response = Vec::new();
        write_media_bytes_response(&mut response, &request, "video/mp4", b"abcdef").unwrap();
        let response = String::from_utf8(response).unwrap();

        assert!(response.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(response.contains("Content-Range: bytes 1-3/6\r\n"));
        assert!(response.contains("Accept-Ranges: bytes\r\n"));
        assert!(response.contains("Access-Control-Allow-Origin: *\r\n"));
        assert!(response.contains("Cache-Control: no-store\r\n"));
        assert!(response.contains("Referrer-Policy: no-referrer\r\n"));
        assert!(response.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(response.ends_with("\r\n\r\nbcd"));

        let head_request = HttpRequest {
            method: "HEAD".to_string(),
            path: "/media/id?token=token".to_string(),
            range: None,
        };
        let mut head_response = Vec::new();
        write_media_bytes_response(&mut head_response, &head_request, "audio/mpeg", b"abcdef")
            .unwrap();
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
}
