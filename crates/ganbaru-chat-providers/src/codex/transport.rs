//! Bounded JSONL JSON-RPC transport for Codex app-server.

use crate::DriverOperationContext;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::process::ProviderProcessHandle;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const MAX_JSONL_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PENDING_REQUESTS: usize = 128;
const MAX_INBOUND_MESSAGES: usize = 256;
const MAX_OUTBOUND_MESSAGES: usize = 256;
const MAX_METHOD_BYTES: usize = 256;
const MAX_RPC_ID_BYTES: usize = 256;
const MAX_ERROR_MESSAGE_BYTES: usize = 2_048;
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexRpcFailure {
    Cancelled,
    Closed,
    Malformed(String),
    Remote { code: i64, message: String },
    Timeout,
    Unavailable,
}

impl CodexRpcFailure {
    pub fn to_chat_error(&self, operation: &str) -> ChatError {
        let (code, message, recoverable) = match self {
            Self::Cancelled => (
                ChatErrorCode::Cancelled,
                format!("Codex {operation} was cancelled"),
                true,
            ),
            Self::Timeout => (
                ChatErrorCode::Timeout,
                format!("Codex {operation} timed out"),
                true,
            ),
            Self::Closed | Self::Unavailable => (
                ChatErrorCode::TransportUnavailable,
                format!("Codex app-server became unavailable during {operation}"),
                true,
            ),
            Self::Malformed(reason) => (
                ChatErrorCode::Protocol,
                format!("Codex app-server returned an invalid {operation} response: {reason}"),
                false,
            ),
            Self::Remote { message, .. } => (
                ChatErrorCode::Protocol,
                format!("Codex rejected {operation}: {message}"),
                true,
            ),
        };
        let mut error = ChatError::new(code, message, recoverable);
        if let Self::Remote {
            code: remote_code, ..
        } = self
        {
            error.details = Some(Box::new(json!({ "providerCode": remote_code })));
        }
        error
    }
}

impl fmt::Display for CodexRpcFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => formatter.write_str("request cancelled"),
            Self::Closed => formatter.write_str("transport closed"),
            Self::Malformed(reason) => write!(formatter, "malformed protocol: {reason}"),
            Self::Remote { code, message } => write!(formatter, "remote error {code}: {message}"),
            Self::Timeout => formatter.write_str("request timed out"),
            Self::Unavailable => formatter.write_str("transport unavailable"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CodexInboundMessage {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Malformed {
        reason: String,
        byte_length: usize,
    },
    Closed,
}

type PendingResponse = oneshot::Sender<Result<Value, CodexRpcFailure>>;
type PendingResponses = Arc<Mutex<HashMap<String, PendingResponse>>>;

#[derive(Clone)]
pub struct CodexRpcClient {
    outbound: mpsc::Sender<Vec<u8>>,
    pending: PendingResponses,
    next_request_id: Arc<std::sync::atomic::AtomicU64>,
}

impl CodexRpcClient {
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        context: &DriverOperationContext,
    ) -> Result<Value, CodexRpcFailure> {
        validate_method(method)?;
        if context.is_cancelled() {
            return Err(CodexRpcFailure::Cancelled);
        }
        let id = self
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = id.to_string();
        let (sender, mut receiver) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| CodexRpcFailure::Unavailable)?;
            if pending.len() >= MAX_PENDING_REQUESTS {
                return Err(CodexRpcFailure::Unavailable);
            }
            pending.insert(key.clone(), sender);
        }
        if let Err(error) = self
            .send_value(json!({ "id": id, "method": method, "params": params }))
            .await
        {
            self.remove_pending(&key);
            return Err(error);
        }

        loop {
            if context.is_cancelled() {
                self.remove_pending(&key);
                return Err(if Instant::now() >= context.deadline {
                    CodexRpcFailure::Timeout
                } else {
                    CodexRpcFailure::Cancelled
                });
            }
            let remaining = context.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.remove_pending(&key);
                return Err(CodexRpcFailure::Timeout);
            }
            let poll = remaining.min(CANCELLATION_POLL_INTERVAL);
            match tokio::time::timeout(poll, &mut receiver).await {
                Ok(Ok(result)) => return result,
                Ok(Err(_)) => return Err(CodexRpcFailure::Closed),
                Err(_) => {}
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), CodexRpcFailure> {
        validate_method(method)?;
        self.send_value(json!({ "method": method, "params": params }))
            .await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), CodexRpcFailure> {
        validate_rpc_id(&id)?;
        self.send_value(json!({ "id": id, "result": result })).await
    }

    pub async fn respond_error(
        &self,
        id: Value,
        code: i64,
        message: &str,
    ) -> Result<(), CodexRpcFailure> {
        validate_rpc_id(&id)?;
        self.send_value(json!({
            "id": id,
            "error": {
                "code": code,
                "message": truncate_text(message, MAX_ERROR_MESSAGE_BYTES),
            }
        }))
        .await
    }

    async fn send_value(&self, value: Value) -> Result<(), CodexRpcFailure> {
        let mut bytes = serde_json::to_vec(&value)
            .map_err(|_| CodexRpcFailure::Malformed("request serialization failed".to_string()))?;
        if bytes.len() > MAX_JSONL_MESSAGE_BYTES {
            return Err(CodexRpcFailure::Malformed(
                "outbound message exceeds the byte limit".to_string(),
            ));
        }
        bytes.push(b'\n');
        self.outbound
            .send(bytes)
            .await
            .map_err(|_| CodexRpcFailure::Closed)
    }

    fn remove_pending(&self, key: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(key);
        }
    }
}

pub struct CodexRpcConnection {
    client: CodexRpcClient,
    inbound: Option<mpsc::Receiver<CodexInboundMessage>>,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
    writer_shutdown: Option<oneshot::Sender<()>>,
    process: Option<ProviderProcessHandle>,
}

impl CodexRpcConnection {
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
        let reader_pending = Arc::clone(&pending);
        let reader_task = tokio::spawn(read_messages(reader, inbound_sender, reader_pending));
        let (writer_shutdown, shutdown_receiver) = oneshot::channel();
        let writer_pending = Arc::clone(&pending);
        let writer_task = tokio::spawn(write_messages(
            writer,
            outbound_receiver,
            shutdown_receiver,
            writer_pending,
        ));
        Self {
            client: CodexRpcClient {
                outbound,
                pending,
                next_request_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            },
            inbound: Some(inbound),
            reader_task,
            writer_task,
            writer_shutdown: Some(writer_shutdown),
            process,
        }
    }

    pub fn client(&self) -> CodexRpcClient {
        self.client.clone()
    }

    pub fn take_inbound(&mut self) -> ChatResult<mpsc::Receiver<CodexInboundMessage>> {
        self.inbound.take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "Codex inbound stream was already claimed",
                false,
            )
        })
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
        fail_pending(&self.client.pending, CodexRpcFailure::Closed);
        process_result
    }
}

impl Drop for CodexRpcConnection {
    fn drop(&mut self) {
        if let Some(shutdown) = self.writer_shutdown.take() {
            let _ = shutdown.send(());
        }
        self.writer_task.abort();
        self.reader_task.abort();
        fail_pending(&self.client.pending, CodexRpcFailure::Closed);
    }
}

async fn write_messages<W>(
    mut writer: W,
    mut outbound: mpsc::Receiver<Vec<u8>>,
    mut shutdown: oneshot::Receiver<()>,
    pending: PendingResponses,
) where
    W: AsyncWrite + Send + Unpin + 'static,
{
    while let Err(tokio::sync::oneshot::error::TryRecvError::Empty) = shutdown.try_recv() {
        let message = match tokio::time::timeout(CANCELLATION_POLL_INTERVAL, outbound.recv()).await
        {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(_) => continue,
        };
        if writer.write_all(&message).await.is_err() || writer.flush().await.is_err() {
            fail_pending(&pending, CodexRpcFailure::Closed);
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn read_messages<R>(
    reader: R,
    inbound: mpsc::Sender<CodexInboundMessage>,
    pending: PendingResponses,
) where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut reader = BufReader::new(reader);
    loop {
        match read_bounded_line(&mut reader).await {
            Ok(Some(line)) => match serde_json::from_slice::<Value>(&line) {
                Ok(value) => match classify_inbound(value) {
                    Ok(InboundEnvelope::Response { id, result }) => {
                        deliver_response(&pending, &id, result);
                    }
                    Ok(InboundEnvelope::Message(message)) => {
                        if inbound.send(message).await.is_err() {
                            fail_pending(&pending, CodexRpcFailure::Closed);
                            return;
                        }
                    }
                    Err(reason) => {
                        let _ = inbound
                            .send(CodexInboundMessage::Malformed {
                                reason,
                                byte_length: line.len(),
                            })
                            .await;
                    }
                },
                Err(_) => {
                    let _ = inbound
                        .send(CodexInboundMessage::Malformed {
                            reason: "message is not valid JSON".to_string(),
                            byte_length: line.len(),
                        })
                        .await;
                }
            },
            Ok(None) => break,
            Err(byte_length) => {
                let _ = inbound
                    .send(CodexInboundMessage::Malformed {
                        reason: "message exceeds the byte limit".to_string(),
                        byte_length,
                    })
                    .await;
            }
        }
    }
    fail_pending(&pending, CodexRpcFailure::Closed);
    let _ = inbound.send(CodexInboundMessage::Closed).await;
}

enum InboundEnvelope {
    Response {
        id: Value,
        result: Result<Value, CodexRpcFailure>,
    },
    Message(CodexInboundMessage),
}

fn classify_inbound(value: Value) -> Result<InboundEnvelope, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "message must be a JSON object".to_string())?;
    if let Some(method) = object.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| "method must be a string".to_string())?;
        validate_method(method).map_err(|error| error.to_string())?;
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        if let Some(id) = object.get("id") {
            validate_rpc_id(id).map_err(|error| error.to_string())?;
            return Ok(InboundEnvelope::Message(CodexInboundMessage::Request {
                id: id.clone(),
                method: method.to_string(),
                params,
            }));
        }
        return Ok(InboundEnvelope::Message(
            CodexInboundMessage::Notification {
                method: method.to_string(),
                params,
            },
        ));
    }

    let id = object
        .get("id")
        .ok_or_else(|| "response is missing an id".to_string())?;
    validate_rpc_id(id).map_err(|error| error.to_string())?;
    let result = match (object.get("result"), object.get("error")) {
        (Some(result), None) => Ok(result.clone()),
        (None, Some(error)) => Err(parse_remote_error(error)?),
        _ => return Err("response must contain exactly one result or error".to_string()),
    };
    Ok(InboundEnvelope::Response {
        id: id.clone(),
        result,
    })
}

fn parse_remote_error(value: &Value) -> Result<CodexRpcFailure, String> {
    let error = value
        .as_object()
        .ok_or_else(|| "response error must be an object".to_string())?;
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(|| "response error code must be an integer".to_string())?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| "response error message must be a string".to_string())?;
    Ok(CodexRpcFailure::Remote {
        code,
        message: truncate_text(message, MAX_ERROR_MESSAGE_BYTES),
    })
}

fn deliver_response(
    pending: &PendingResponses,
    id: &Value,
    result: Result<Value, CodexRpcFailure>,
) {
    let Ok(key) = rpc_id_key(id) else {
        return;
    };
    let sender = pending
        .lock()
        .ok()
        .and_then(|mut responses| responses.remove(&key));
    if let Some(sender) = sender {
        let _ = sender.send(result);
    }
}

fn fail_pending(pending: &PendingResponses, failure: CodexRpcFailure) {
    let responses = pending
        .lock()
        .map(|mut pending| {
            pending
                .drain()
                .map(|(_, sender)| sender)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for response in responses {
        let _ = response.send(Err(failure.clone()));
    }
}

fn validate_method(method: &str) -> Result<(), CodexRpcFailure> {
    if method.is_empty() || method.len() > MAX_METHOD_BYTES || method.chars().any(char::is_control)
    {
        return Err(CodexRpcFailure::Malformed("method is invalid".to_string()));
    }
    Ok(())
}

fn validate_rpc_id(id: &Value) -> Result<(), CodexRpcFailure> {
    match id {
        Value::Number(_) => Ok(()),
        Value::String(value)
            if !value.is_empty()
                && value.len() <= MAX_RPC_ID_BYTES
                && !value.chars().any(char::is_control) =>
        {
            Ok(())
        }
        _ => Err(CodexRpcFailure::Malformed(
            "request id is invalid".to_string(),
        )),
    }
}

fn rpc_id_key(id: &Value) -> Result<String, CodexRpcFailure> {
    validate_rpc_id(id)?;
    serde_json::to_string(id)
        .map_err(|_| CodexRpcFailure::Malformed("request id is invalid".to_string()))
}

async fn read_bounded_line<R>(reader: &mut BufReader<R>) -> Result<Option<Vec<u8>>, usize>
where
    R: AsyncRead + Unpin,
{
    let mut line = Vec::new();
    let mut byte_length = 0_usize;
    let mut oversized = false;
    loop {
        let available = match reader.fill_buf().await {
            Ok(available) => available,
            Err(_) => return Ok(None),
        };
        if available.is_empty() {
            if byte_length == 0 {
                return Ok(None);
            }
            return if oversized {
                Err(byte_length)
            } else {
                Ok(Some(trim_line_ending(line)))
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        byte_length = byte_length.saturating_add(consumed);
        if !oversized {
            if line.len().saturating_add(consumed) > MAX_JSONL_MESSAGE_BYTES + 1 {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..consumed]);
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            return if oversized {
                Err(byte_length)
            } else {
                Ok(Some(trim_line_ending(line)))
            };
        }
    }
}

fn trim_line_ending(mut line: Vec<u8>) -> Vec<u8> {
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    line
}

fn truncate_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut boundary = max_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_string()
}
