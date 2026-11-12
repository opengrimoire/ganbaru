//! Replay over loopback HTTP for providers without a stdio seam: a fake server answers the
//! driver's requests and streams server-sent events from the transcript.
//!
//! An outbound frame is one request, `{"method", "path", "query"?, "body"?}`, with the query
//! decoded and the origin left out so the port never reaches a transcript. An inbound frame is
//! one of:
//!
//! - `{"status", "headers"?, "body"?}`: answers the oldest matched request that has no answer
//!   yet, with the extra response headers `headers` names.
//! - `{"status", "stream": true}`: answers it with an event stream that later frames write to.
//! - `{"event"}`: writes one server-sent event on the open stream.
//! - `{"close_stream": true}`: closes the open stream, as a dropped subscription does.
//! - `{"await_events": {"kind", "count"}}`: waits until the driver emitted `count` events of
//!   `kind`, so an event the server sends next cannot overtake the driver's own events.

use super::*;
use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

/// The largest request head the fake server reads.
const MAX_REQUEST_HEAD_BYTES: usize = 64 * 1024;
/// The largest request body the fake server reads.
const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
/// The response header of an event stream, as OpenCode sends it.
const EVENT_STREAM_HEADER: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";

/// One request the fake server read, with the connection its answer goes to.
struct ReceivedRequest {
    frame: Value,
    connection: TcpStream,
}

/// The loopback listener of one replay.
struct ReplayServer {
    origin: String,
    requests: mpsc::UnboundedReceiver<ReceivedRequest>,
    /// Set once the transcript's provider exited; later connections are closed unanswered.
    exited: Arc<AtomicBool>,
    accept_task: tokio::task::JoinHandle<()>,
}

impl ReplayServer {
    async fn bind() -> Result<Self, ReplayFailure> {
        let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|error| {
            ReplayFailure::new(
                None,
                None,
                format!("cannot bind the replay server: {error}"),
            )
        })?;
        let address = listener.local_addr().map_err(|error| {
            ReplayFailure::new(
                None,
                None,
                format!("cannot read the replay address: {error}"),
            )
        })?;
        let (sender, requests) = mpsc::unbounded_channel();
        let exited = Arc::new(AtomicBool::new(false));
        let accept_exited = Arc::clone(&exited);
        let accept_task = tokio::spawn(async move {
            while let Ok((mut connection, _)) = listener.accept().await {
                if accept_exited.load(Ordering::Acquire) {
                    continue;
                }
                let sender = sender.clone();
                tokio::spawn(async move {
                    let frame = match read_request(&mut connection).await {
                        Ok(frame) => frame,
                        Err(reason) => json!({ "unreadable": reason }),
                    };
                    let _ = sender.send(ReceivedRequest { frame, connection });
                });
            }
        });
        Ok(Self {
            origin: format!("http://{address}"),
            requests,
            exited,
            accept_task,
        })
    }
}

impl Drop for ReplayServer {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

/// Reads one HTTP request and returns its frame.
async fn read_request(connection: &mut TcpStream) -> Result<Value, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let head_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > MAX_REQUEST_HEAD_BYTES {
            return Err("the request head exceeds the replay bound".to_string());
        }
        let read = connection
            .read(&mut buffer)
            .await
            .map_err(|error| format!("cannot read the request: {error}"))?;
        if read == 0 {
            return Err("the connection closed before the request head ended".to_string());
        }
        bytes.extend_from_slice(&buffer[..read]);
    };
    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
    let content_length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    if content_length > MAX_REQUEST_BODY_BYTES {
        return Err("the request body exceeds the replay bound".to_string());
    }
    while bytes.len() < head_end + content_length {
        let read = connection
            .read(&mut buffer)
            .await
            .map_err(|error| format!("cannot read the request body: {error}"))?;
        if read == 0 {
            return Err("the connection closed before the request body ended".to_string());
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    let mut request_line = head.lines().next().unwrap_or_default().split_whitespace();
    let method = request_line.next().unwrap_or_default();
    let target = request_line.next().unwrap_or_default();
    request_frame(method, target, &bytes[head_end..head_end + content_length])
}

/// The frame of a request: method, path, decoded query, and the body as JSON when it is JSON.
fn request_frame(method: &str, target: &str, body: &[u8]) -> Result<Value, String> {
    let url = reqwest::Url::parse(&format!("http://replay{target}"))
        .map_err(|error| format!("the request target {target} is invalid: {error}"))?;
    let mut frame = Map::new();
    frame.insert("method".to_string(), json!(method));
    frame.insert("path".to_string(), json!(url.path()));
    let query = url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), Value::String(value.into_owned())))
        .collect::<Map<_, _>>();
    if !query.is_empty() {
        frame.insert("query".to_string(), Value::Object(query));
    }
    if !body.is_empty() {
        let body = serde_json::from_slice(body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(body).into_owned()));
        frame.insert("body".to_string(), body);
    }
    Ok(Value::Object(frame))
}

/// Replays `transcript` against a driver that reaches the fake server over HTTP.
pub(super) async fn replay_over_http(
    provider: ReplayProvider,
    transcript: Transcript,
    script: &ReplayScript,
    directories: &ReplayDirectories,
) -> Result<ReplayReport, ReplayFailure> {
    let workspace = directories.workspace.to_string_lossy().into_owned();
    let cli_version = transcript.header.cli_version.clone();
    let setup = ReplaySetup {
        workspace: &directories.workspace,
        home: &directories.home,
        organizational: script.organizational,
        cli_version: &cli_version,
    };
    let mut server = ReplayServer::bind().await?;
    let driver = provider
        .http_driver(&setup, &server.origin, script.busy_stream_timeout)
        .map_err(|error| ReplayFailure::new(None, None, format!("driver setup: {error:?}")))?;
    let mask = provider.mask(&transcript)?;
    let cursor = ReplayCursor::new(
        transcript,
        mask,
        ReplayBindings::new(workspace.clone(), None)
            .with_home(directories.home.to_string_lossy().into_owned()),
    );
    let (finished_sender, finished) = watch::channel(false);
    let unexpected = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::new(ReplaySink::default());
    let requests = std::mem::replace(&mut server.requests, mpsc::unbounded_channel().1);
    let process = tokio::spawn(serve(
        cursor,
        HttpPlayback {
            requests,
            exited: Arc::clone(&server.exited),
            sink: Arc::clone(&sink),
            finished: finished_sender,
            unexpected: Arc::clone(&unexpected),
        },
    ));
    let calls = run_script(driver, script, &setup, Arc::clone(&sink), finished).await;
    let outcome = match tokio::time::timeout(SETTLE_TIME, process).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => Err(ReplayFailure::new(
            None,
            None,
            format!("replay task failed: {error}"),
        )),
        Err(_) => Ok(()),
    };
    outcome?;
    report(calls, &unexpected, &sink, &workspace)
}

/// What the server side of an HTTP replay works with.
struct HttpPlayback {
    requests: mpsc::UnboundedReceiver<ReceivedRequest>,
    exited: Arc<AtomicBool>,
    sink: Arc<ReplaySink>,
    finished: watch::Sender<bool>,
    unexpected: Arc<Mutex<Vec<Value>>>,
}

/// The server side: matches requests with expectations, answers them, and writes events.
async fn serve(mut cursor: ReplayCursor, mut playback: HttpPlayback) -> Result<(), ReplayFailure> {
    let mut parked = Vec::<ReceivedRequest>::new();
    let mut unanswered = VecDeque::<TcpStream>::new();
    let mut stream: Option<TcpStream> = None;
    let result = loop {
        let action = match cursor.advance() {
            Ok(action) => action,
            Err((line, reason)) => break Err(ReplayFailure::new(Some(line), None, reason)),
        };
        match action {
            ReplayAction::Expect { line, label, frame } => {
                let expected = cursor.display_expected(&frame);
                let request =
                    match next_request(&mut playback.requests, &mut parked, &expected).await {
                        Ok(request) => request,
                        Err(reason) => {
                            let mut failure = ReplayFailure::new(Some(line), label, reason);
                            failure.expected = Some(Box::new(expected));
                            if let Some(first) = parked.first() {
                                failure.actual = Some(Box::new(first.frame.clone()));
                            }
                            break Err(failure);
                        }
                    };
                if let Err(reason) = cursor.accept(&frame, label.as_deref(), &request.frame) {
                    let mut failure = ReplayFailure::new(
                        Some(line),
                        label,
                        format!("request differs at {reason}"),
                    );
                    failure.expected = Some(Box::new(expected));
                    failure.actual = Some(Box::new(request.frame));
                    break Err(failure);
                }
                unanswered.push_back(request.connection);
            }
            ReplayAction::Emit {
                line,
                label,
                frame,
                after_ms,
            } => {
                if after_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(after_ms)).await;
                }
                if let Err(reason) =
                    emit(&frame, &mut unanswered, &mut stream, &playback.sink).await
                {
                    break Err(ReplayFailure::new(Some(line), label, reason));
                }
            }
            ReplayAction::Exit { .. } => {
                playback.exited.store(true, Ordering::Release);
                drop(stream);
                drop(unanswered);
                drop(parked);
                let _ = playback.finished.send(true);
                while let Some(request) = playback.requests.recv().await {
                    drop(request.connection);
                }
                return Ok(());
            }
            ReplayAction::Done => {
                let _ = playback.finished.send(true);
                // The open stream stays connected until the driver is dropped, as a server
                // that keeps running does.
                let _stream = stream;
                for request in parked.drain(..) {
                    playback.unexpected.lock().unwrap().push(request.frame);
                }
                while let Some(mut request) = playback.requests.recv().await {
                    playback.unexpected.lock().unwrap().push(request.frame);
                    let _ = write_response(&mut request.connection, 404, None, None).await;
                }
                return Ok(());
            }
        }
    };
    let _ = playback.finished.send(true);
    result
}

/// The next request with the expected method and path, parking the requests that arrive first
/// with another, since the driver's event pump sends requests alongside its calls.
async fn next_request(
    requests: &mut mpsc::UnboundedReceiver<ReceivedRequest>,
    parked: &mut Vec<ReceivedRequest>,
    expected: &Value,
) -> Result<ReceivedRequest, String> {
    let route = |frame: &Value| (frame.get("method").cloned(), frame.get("path").cloned());
    let wanted = route(expected);
    if let Some(index) = parked
        .iter()
        .position(|request| route(&request.frame) == wanted)
    {
        return Ok(parked.remove(index));
    }
    let deadline = tokio::time::Instant::now() + EXPECT_TIMEOUT;
    loop {
        match tokio::time::timeout_at(deadline, requests.recv()).await {
            Err(_) => return Err("the driver never sent the expected request".to_string()),
            Ok(None) => return Err("the replay server stopped accepting requests".to_string()),
            Ok(Some(request)) if route(&request.frame) == wanted => return Ok(request),
            Ok(Some(request)) => parked.push(request),
        }
    }
}

/// Plays one inbound frame.
async fn emit(
    frame: &Value,
    unanswered: &mut VecDeque<TcpStream>,
    stream: &mut Option<TcpStream>,
    sink: &ReplaySink,
) -> Result<(), String> {
    if let Some(status) = frame.get("status") {
        let status = status
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or("the response status is not a number")?;
        let mut connection = unanswered
            .pop_front()
            .ok_or("a response has no matched request to answer")?;
        if frame.get("stream").and_then(Value::as_bool) == Some(true) {
            connection
                .write_all(EVENT_STREAM_HEADER)
                .await
                .map_err(|error| format!("cannot open the event stream: {error}"))?;
            *stream = Some(connection);
            return Ok(());
        }
        return write_response(
            &mut connection,
            status,
            frame.get("headers").and_then(Value::as_object),
            frame.get("body"),
        )
        .await;
    }
    if let Some(event) = frame.get("event") {
        let connection = stream.as_mut().ok_or("an event has no open stream")?;
        let line = format!("data: {event}\n\n");
        return connection
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("cannot write the event: {error}"));
    }
    if frame.get("close_stream").and_then(Value::as_bool) == Some(true) {
        *stream = None;
        return Ok(());
    }
    if let Some(wait) = frame.get("await_events") {
        let kind = wait
            .get("kind")
            .and_then(Value::as_str)
            .ok_or("await_events names no kind")?;
        let count = wait
            .get("count")
            .and_then(Value::as_u64)
            .and_then(|count| usize::try_from(count).ok())
            .ok_or("await_events names no count")?;
        if !sink.wait_for(kind, count).await {
            return Err(format!(
                "the driver emitted fewer than {count} {kind} events; events: {}",
                Value::Array(sink.events().iter().map(event_kind).collect())
            ));
        }
        return Ok(());
    }
    Err(format!(
        "the inbound frame {frame} is not a response, event, or directive"
    ))
}

/// Writes a complete response and closes the connection.
async fn write_response(
    connection: &mut TcpStream,
    status: u16,
    headers: Option<&Map<String, Value>>,
    body: Option<&Value>,
) -> Result<(), String> {
    let body = match body {
        Some(body) => serde_json::to_vec(body).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        _ => "Replay",
    };
    let extra = headers
        .into_iter()
        .flatten()
        .filter_map(|(name, value)| Some(format!("{name}: {}\r\n", value.as_str()?)))
        .collect::<String>();
    let mut response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend(body);
    connection
        .write_all(&response)
        .await
        .map_err(|error| format!("cannot write the response: {error}"))?;
    let _ = connection.shutdown().await;
    Ok(())
}
