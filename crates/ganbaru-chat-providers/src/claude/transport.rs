//! Bounded JSONL transport for Claude Code's bidirectional SDK protocol.

use crate::DriverOperationContext;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::process::ProviderProcessHandle;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const MAX_JSONL_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PENDING_REQUESTS: usize = 128;
const MAX_INBOUND_MESSAGES: usize = 256;
const MAX_OUTBOUND_MESSAGES: usize = 256;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaudeTransportFailure {
    Cancelled,
    Closed,
    Malformed(String),
    Remote(String),
    Timeout,
}

impl ClaudeTransportFailure {
    pub fn to_chat_error(&self, operation: &str) -> ChatError {
        let (code, message, recoverable) = match self {
            Self::Cancelled => (
                ChatErrorCode::Cancelled,
                format!("Claude {operation} was cancelled"),
                true,
            ),
            Self::Timeout => (
                ChatErrorCode::Timeout,
                format!("Claude {operation} timed out"),
                true,
            ),
            Self::Closed => (
                ChatErrorCode::TransportUnavailable,
                format!("Claude became unavailable during {operation}"),
                true,
            ),
            Self::Malformed(reason) => (
                ChatErrorCode::Protocol,
                format!("Claude returned an invalid {operation} response: {reason}"),
                false,
            ),
            Self::Remote(message) => (
                ChatErrorCode::Protocol,
                format!("Claude rejected {operation}: {message}"),
                true,
            ),
        };
        ChatError::new(code, message, recoverable)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeInboundMessage {
    Message(Value),
    Malformed { reason: String, byte_length: usize },
    Closed,
}

type PendingResponse = oneshot::Sender<Result<Value, ClaudeTransportFailure>>;
type PendingResponses = Arc<Mutex<HashMap<String, PendingResponse>>>;

#[derive(Clone)]
pub struct ClaudeJsonlClient {
    outbound: mpsc::Sender<Vec<u8>>,
    pending: PendingResponses,
    next_request_id: Arc<AtomicU64>,
}

impl ClaudeJsonlClient {
    pub async fn request(
        &self,
        subtype: &str,
        fields: Value,
        context: &DriverOperationContext,
    ) -> Result<Value, ClaudeTransportFailure> {
        if subtype.is_empty() || subtype.len() > 128 || !fields.is_object() {
            return Err(ClaudeTransportFailure::Malformed(
                "control request shape is invalid".to_string(),
            ));
        }
        let request_id = format!(
            "ganbaru-{}",
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let receiver = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| ClaudeTransportFailure::Closed)?;
            if pending.len() >= MAX_PENDING_REQUESTS {
                return Err(ClaudeTransportFailure::Malformed(
                    "too many pending control requests".to_string(),
                ));
            }
            let (sender, receiver) = oneshot::channel();
            pending.insert(request_id.clone(), sender);
            receiver
        };
        let mut request = fields.as_object().cloned().unwrap_or_default();
        request.insert("subtype".to_string(), Value::String(subtype.to_string()));
        if let Err(error) = self
            .send(json!({
                "type": "control_request",
                "request_id": request_id,
                "request": request,
            }))
            .await
        {
            self.remove_pending(&request_id);
            return Err(error);
        }
        await_response(receiver, &request_id, &self.pending, context).await
    }

    pub async fn send(&self, value: Value) -> Result<(), ClaudeTransportFailure> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| {
            ClaudeTransportFailure::Malformed("message serialization failed".to_string())
        })?;
        if bytes.len() > MAX_JSONL_MESSAGE_BYTES {
            return Err(ClaudeTransportFailure::Malformed(
                "outbound message exceeds the byte limit".to_string(),
            ));
        }
        bytes.push(b'\n');
        self.outbound
            .send(bytes)
            .await
            .map_err(|_| ClaudeTransportFailure::Closed)
    }

    fn remove_pending(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }
}

pub struct ClaudeJsonlConnection {
    client: ClaudeJsonlClient,
    inbound: Option<mpsc::Receiver<ClaudeInboundMessage>>,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
    writer_shutdown: Option<oneshot::Sender<()>>,
    process: Option<ProviderProcessHandle>,
}

impl ClaudeJsonlConnection {
    pub fn from_process(mut process: ProviderProcessHandle) -> ChatResult<Self> {
        let stdout = process.take_stdout()?;
        let stdin = process.take_stdin()?;
        Ok(Self::from_io(stdout, stdin, Some(process)))
    }

    #[cfg(test)]
    pub fn from_test_io<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        Self::from_io(reader, writer, None)
    }

    fn from_io<R, W>(reader: R, writer: W, process: Option<ProviderProcessHandle>) -> Self
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (outbound, outbound_receiver) = mpsc::channel(MAX_OUTBOUND_MESSAGES);
        let (inbound_sender, inbound) = mpsc::channel(MAX_INBOUND_MESSAGES);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let reader_task = tokio::spawn(read_messages(reader, inbound_sender, Arc::clone(&pending)));
        let (writer_shutdown, shutdown_receiver) = oneshot::channel();
        let writer_task = tokio::spawn(write_messages(
            writer,
            outbound_receiver,
            shutdown_receiver,
            Arc::clone(&pending),
        ));
        Self {
            client: ClaudeJsonlClient {
                outbound,
                pending,
                next_request_id: Arc::new(AtomicU64::new(1)),
            },
            inbound: Some(inbound),
            reader_task,
            writer_task,
            writer_shutdown: Some(writer_shutdown),
            process,
        }
    }

    pub fn client(&self) -> ClaudeJsonlClient {
        self.client.clone()
    }

    pub fn take_inbound(&mut self) -> ChatResult<mpsc::Receiver<ClaudeInboundMessage>> {
        self.inbound.take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "Claude inbound stream was already claimed",
                false,
            )
        })
    }

    pub fn diagnostic(&self) -> ChatResult<Option<String>> {
        self.process
            .as_ref()
            .map(|process| process.diagnostic().map(|value| value.text))
            .transpose()
    }

    pub async fn stop(
        &mut self,
        graceful_deadline: Duration,
        force_deadline: Duration,
    ) -> ChatResult<()> {
        if let Some(shutdown) = self.writer_shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = tokio::time::timeout(graceful_deadline, &mut self.writer_task).await;
        let process_result = match self.process.as_mut() {
            Some(process) => process.stop(graceful_deadline, force_deadline).await,
            None => Ok(()),
        };
        let _ = tokio::time::timeout(force_deadline, &mut self.reader_task).await;
        if !self.writer_task.is_finished() {
            self.writer_task.abort();
        }
        if !self.reader_task.is_finished() {
            self.reader_task.abort();
        }
        fail_pending(&self.client.pending, ClaudeTransportFailure::Closed);
        process_result
    }
}

impl Drop for ClaudeJsonlConnection {
    fn drop(&mut self) {
        if let Some(shutdown) = self.writer_shutdown.take() {
            let _ = shutdown.send(());
        }
        self.reader_task.abort();
        self.writer_task.abort();
        fail_pending(&self.client.pending, ClaudeTransportFailure::Closed);
    }
}

async fn await_response(
    mut receiver: oneshot::Receiver<Result<Value, ClaudeTransportFailure>>,
    request_id: &str,
    pending: &PendingResponses,
    context: &DriverOperationContext,
) -> Result<Value, ClaudeTransportFailure> {
    loop {
        if context.is_cancelled() {
            remove_pending(pending, request_id);
            return Err(if Instant::now() >= context.deadline {
                ClaudeTransportFailure::Timeout
            } else {
                ClaudeTransportFailure::Cancelled
            });
        }
        let remaining = context.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            remove_pending(pending, request_id);
            return Err(ClaudeTransportFailure::Timeout);
        }
        match tokio::time::timeout(remaining.min(CANCELLATION_POLL_INTERVAL), &mut receiver).await {
            Ok(Ok(result)) => return result,
            Ok(Err(_)) => return Err(ClaudeTransportFailure::Closed),
            Err(_) => {}
        }
    }
}

async fn read_messages<R>(
    reader: R,
    inbound: mpsc::Sender<ClaudeInboundMessage>,
    pending: PendingResponses,
) where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let line = match read_bounded_line(&mut reader, &mut buffer, MAX_JSONL_MESSAGE_BYTES).await
        {
            Ok(line) => line,
            Err(_) => break,
        };
        let Some(line) = line else {
            break;
        };
        if line.oversized {
            let _ = inbound
                .send(ClaudeInboundMessage::Malformed {
                    reason: "message exceeds the byte limit".to_string(),
                    byte_length: line.byte_length,
                })
                .await;
            continue;
        }
        let value: Value = match serde_json::from_slice(&buffer) {
            Ok(value) => value,
            Err(_) => {
                let _ = inbound
                    .send(ClaudeInboundMessage::Malformed {
                        reason: "message is not valid JSON".to_string(),
                        byte_length: buffer.len(),
                    })
                    .await;
                continue;
            }
        };
        if route_control_response(&value, &pending) {
            continue;
        }
        if inbound
            .send(ClaudeInboundMessage::Message(value))
            .await
            .is_err()
        {
            break;
        }
    }
    fail_pending(&pending, ClaudeTransportFailure::Closed);
    let _ = inbound.send(ClaudeInboundMessage::Closed).await;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct BoundedLine {
    pub byte_length: usize,
    pub oversized: bool,
}

pub(super) async fn read_bounded_line<R>(
    reader: &mut R,
    output: &mut Vec<u8>,
    max_payload_bytes: usize,
) -> std::io::Result<Option<BoundedLine>>
where
    R: AsyncBufRead + Unpin,
{
    let mut byte_length = 0_usize;
    loop {
        let (consumed, found_newline) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                if byte_length == 0 {
                    return Ok(None);
                }
                return Ok(Some(BoundedLine {
                    byte_length,
                    oversized: byte_length > max_payload_bytes,
                }));
            }
            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |index| index + 1);
            let remaining = max_payload_bytes
                .saturating_add(1)
                .saturating_sub(output.len());
            output.extend_from_slice(&available[..consumed.min(remaining)]);
            (consumed, newline.is_some())
        };
        byte_length = byte_length.saturating_add(consumed);
        reader.consume(consumed);
        if found_newline {
            return Ok(Some(BoundedLine {
                byte_length,
                oversized: byte_length > max_payload_bytes.saturating_add(1),
            }));
        }
    }
}

fn route_control_response(value: &Value, pending: &PendingResponses) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) != Some("control_response") {
        return false;
    }
    let Some(response) = object.get("response").and_then(Value::as_object) else {
        return false;
    };
    let Some(request_id) = response.get("request_id").and_then(Value::as_str) else {
        return false;
    };
    let sender = pending
        .lock()
        .ok()
        .and_then(|mut map| map.remove(request_id));
    if let Some(sender) = sender {
        let result = match response.get("subtype").and_then(Value::as_str) {
            Some("success") => Ok(response
                .get("response")
                .cloned()
                .unwrap_or_else(|| json!({}))),
            Some("error") => Err(ClaudeTransportFailure::Remote(
                response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Claude rejected the request")
                    .chars()
                    .take(2_048)
                    .collect(),
            )),
            _ => Err(ClaudeTransportFailure::Malformed(
                "control response subtype is invalid".to_string(),
            )),
        };
        let _ = sender.send(result);
    }
    true
}

async fn write_messages<W>(
    mut writer: W,
    mut outbound: mpsc::Receiver<Vec<u8>>,
    mut shutdown: oneshot::Receiver<()>,
    pending: PendingResponses,
) where
    W: AsyncWrite + Send + Unpin + 'static,
{
    loop {
        match shutdown.try_recv() {
            Ok(()) | Err(oneshot::error::TryRecvError::Closed) => break,
            Err(oneshot::error::TryRecvError::Empty) => {}
        }
        let message = match tokio::time::timeout(CANCELLATION_POLL_INTERVAL, outbound.recv()).await
        {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => continue,
        };
        if writer.write_all(&message).await.is_err() || writer.flush().await.is_err() {
            break;
        }
    }
    let _ = writer.shutdown().await;
    fail_pending(&pending, ClaudeTransportFailure::Closed);
}

fn remove_pending(pending: &PendingResponses, request_id: &str) {
    if let Ok(mut map) = pending.lock() {
        map.remove(request_id);
    }
}

fn fail_pending(pending: &PendingResponses, error: ClaudeTransportFailure) {
    let entries = pending
        .lock()
        .map(|mut map| map.drain().map(|(_, sender)| sender).collect::<Vec<_>>())
        .unwrap_or_default();
    for sender in entries {
        let _ = sender.send(Err(error.clone()));
    }
}
