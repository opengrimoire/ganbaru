//! Bounded newline-delimited JSON-RPC transport for ACP version 1.

use crate::DriverOperationContext;
use crate::live_session::{ConnectionKill, ProcessSlot, WriterShutdown};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::process::ProviderProcessHandle;
use crate::response_map::{ResponseMap, ResponseSlotRefusal};
use serde_json::{Value, json};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_INBOUND_MESSAGES: usize = 256;
const MAX_OUTBOUND_MESSAGES: usize = 256;
const MAX_METHOD_BYTES: usize = 256;
const MAX_RPC_ID_BYTES: usize = 256;
const MAX_ERROR_BYTES: usize = 2_048;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcpRpcFailure {
    Cancelled,
    Closed,
    Malformed(String),
    Remote { code: i64, message: String },
    Timeout,
    Unavailable,
}

impl AcpRpcFailure {
    pub fn to_chat_error(&self, operation: &str) -> ChatError {
        let (code, message, recoverable) = match self {
            Self::Cancelled => (
                ChatErrorCode::Cancelled,
                format!("ACP provider {operation} was cancelled"),
                true,
            ),
            Self::Timeout => (
                ChatErrorCode::Timeout,
                format!("ACP provider {operation} timed out"),
                true,
            ),
            Self::Closed | Self::Unavailable => (
                ChatErrorCode::TransportUnavailable,
                format!("ACP transport became unavailable during {operation}"),
                true,
            ),
            Self::Malformed(reason) => (
                ChatErrorCode::Protocol,
                format!("ACP provider returned an invalid {operation} response: {reason}"),
                false,
            ),
            Self::Remote { message, .. } => (
                ChatErrorCode::Protocol,
                format!("ACP provider rejected {operation}: {message}"),
                true,
            ),
        };
        let mut error = ChatError::new(code, message, recoverable);
        if let Self::Remote { code, .. } = self {
            error.details = Some(Box::new(json!({ "providerCode": code })));
        }
        error
    }
}

impl fmt::Display for AcpRpcFailure {
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
pub enum AcpInboundMessage {
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

type PendingResponses = ResponseMap<AcpRpcFailure>;

#[derive(Clone)]
pub struct AcpRpcClient {
    outbound: mpsc::Sender<Vec<u8>>,
    pending: PendingResponses,
    next_request_id: Arc<std::sync::atomic::AtomicU64>,
}

impl AcpRpcClient {
    pub async fn request(
        &self,
        method: &str,
        params: Value,
        context: &DriverOperationContext,
    ) -> Result<Value, AcpRpcFailure> {
        let (key, receiver) = self.send_request(method, params, context).await?;
        self.await_response(&key, receiver, None, context).await
    }

    /// Sends a request that may also complete through `fallback`, such as a prompt whose
    /// result arrives as a notification before its response.
    pub async fn request_with_fallback(
        &self,
        method: &str,
        params: Value,
        fallback: oneshot::Receiver<Value>,
        context: &DriverOperationContext,
    ) -> Result<Value, AcpRpcFailure> {
        let (key, receiver) = self.send_request(method, params, context).await?;
        self.await_response(&key, receiver, Some(fallback), context)
            .await
    }

    /// Registers a pending response slot and writes the request.
    async fn send_request(
        &self,
        method: &str,
        params: Value,
        context: &DriverOperationContext,
    ) -> Result<(String, oneshot::Receiver<Result<Value, AcpRpcFailure>>), AcpRpcFailure> {
        validate_method(method)?;
        if context.is_cancelled() {
            return Err(AcpRpcFailure::Cancelled);
        }
        let id = self
            .next_request_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let key = id.to_string();
        let receiver = self
            .pending
            .register(key.clone())
            .map_err(|refusal| match refusal {
                ResponseSlotRefusal::Full => AcpRpcFailure::Unavailable,
                ResponseSlotRefusal::Closed(failure) => failure,
            })?;
        if let Err(error) = self
            .send_value(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .await
        {
            self.remove_pending(&key);
            return Err(error);
        }
        Ok((key, receiver))
    }

    /// Waits for the response, the fallback completion, cancellation, or the deadline,
    /// whichever comes first, and wakes at once on each.
    ///
    /// Cancellation and the deadline win over a response that is ready at the same time.
    async fn await_response(
        &self,
        key: &str,
        receiver: oneshot::Receiver<Result<Value, AcpRpcFailure>>,
        fallback: Option<oneshot::Receiver<Value>>,
        context: &DriverOperationContext,
    ) -> Result<Value, AcpRpcFailure> {
        let fallback = async move {
            match fallback {
                Some(fallback) => fallback.await.map_err(|_| AcpRpcFailure::Closed),
                None => std::future::pending().await,
            }
        };
        let deadline = tokio::time::Instant::from_std(context.deadline);
        let outcome = tokio::select! {
            biased;
            () = context.cancellation.cancelled() => Err(if Instant::now() >= context.deadline {
                AcpRpcFailure::Timeout
            } else {
                AcpRpcFailure::Cancelled
            }),
            () = tokio::time::sleep_until(deadline) => Err(AcpRpcFailure::Timeout),
            completion = fallback => completion,
            response = receiver => return response.unwrap_or(Err(AcpRpcFailure::Closed)),
        };
        self.remove_pending(key);
        outcome
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), AcpRpcFailure> {
        validate_method(method)?;
        self.send_value(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }

    pub async fn respond(&self, id: Value, result: Value) -> Result<(), AcpRpcFailure> {
        validate_rpc_id(&id)?;
        self.send_value(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await
    }

    pub async fn respond_error(
        &self,
        id: Value,
        code: i64,
        message: &str,
    ) -> Result<(), AcpRpcFailure> {
        validate_rpc_id(&id)?;
        self.send_value(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": bounded_text(message, MAX_ERROR_BYTES) },
        }))
        .await
    }

    async fn send_value(&self, value: Value) -> Result<(), AcpRpcFailure> {
        let mut bytes = serde_json::to_vec(&value)
            .map_err(|_| AcpRpcFailure::Malformed("serialization failed".to_string()))?;
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(AcpRpcFailure::Malformed(
                "outbound message exceeds the byte limit".to_string(),
            ));
        }
        bytes.push(b'\n');
        self.outbound
            .send(bytes)
            .await
            .map_err(|_| AcpRpcFailure::Closed)
    }

    fn remove_pending(&self, key: &str) {
        self.pending.take(key);
    }
}

pub struct AcpRpcConnection {
    client: AcpRpcClient,
    inbound: Option<mpsc::Receiver<AcpInboundMessage>>,
    reader_task: JoinHandle<()>,
    writer_task: JoinHandle<()>,
    writer_shutdown: WriterShutdown,
    process: ProcessSlot,
}

impl AcpRpcConnection {
    /// Process ID of the provider process, while the connection still owns it.
    pub fn process_id(&self) -> Option<u32> {
        self.process
            .with(ProviderProcessHandle::process_id)
            .flatten()
    }

    /// Watch that tells whether the resource scope's memory ceiling stopped the provider; `None`
    /// when the provider is not scoped.
    pub fn memory_limit_watch(&self) -> Option<crate::process::systemd_scope::MemoryLimitWatch> {
        self.process
            .with(ProviderProcessHandle::memory_limit_watch)
            .flatten()
    }

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
        let pending = PendingResponses::new();
        let reader_task = tokio::spawn(read_messages(reader, inbound_sender, pending.clone()));
        let (writer_shutdown, shutdown_receiver) = oneshot::channel();
        let writer_task = tokio::spawn(write_messages(
            writer,
            outbound_receiver,
            shutdown_receiver,
            pending.clone(),
        ));
        Self {
            client: AcpRpcClient {
                outbound,
                pending,
                next_request_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
            },
            inbound: Some(inbound),
            reader_task,
            writer_task,
            writer_shutdown: WriterShutdown::new(writer_shutdown),
            process: ProcessSlot::new(process),
        }
    }

    pub fn client(&self) -> AcpRpcClient {
        self.client.clone()
    }

    /// A handle that stops the provider process from a task that does not own the connection.
    pub fn kill_handle(&self) -> ConnectionKill {
        ConnectionKill::new(self.writer_shutdown.clone(), self.process.clone())
    }

    pub fn take_inbound(&mut self) -> ChatResult<mpsc::Receiver<AcpInboundMessage>> {
        self.inbound.take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "ACP inbound stream was already claimed",
                false,
            )
        })
    }

    pub async fn stop(
        &mut self,
        graceful_deadline: Duration,
        force_deadline: Duration,
    ) -> ChatResult<()> {
        self.writer_shutdown.signal();
        let _ = tokio::time::timeout(graceful_deadline, &mut self.writer_task).await;
        let process_result = self.process.stop(graceful_deadline, force_deadline).await;
        let _ = tokio::time::timeout(force_deadline, &mut self.reader_task).await;
        if !self.writer_task.is_finished() {
            self.writer_task.abort();
        }
        if !self.reader_task.is_finished() {
            self.reader_task.abort();
        }
        self.client.pending.close(AcpRpcFailure::Closed);
        process_result
    }
}

impl Drop for AcpRpcConnection {
    fn drop(&mut self) {
        self.writer_shutdown.signal();
        self.writer_task.abort();
        self.reader_task.abort();
        self.client.pending.close(AcpRpcFailure::Closed);
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
    loop {
        let message = tokio::select! {
            biased;
            _ = &mut shutdown => break,
            message = outbound.recv() => match message {
                Some(message) => message,
                None => break,
            },
        };
        if writer.write_all(&message).await.is_err() || writer.flush().await.is_err() {
            pending.close(AcpRpcFailure::Closed);
            break;
        }
    }
    let _ = writer.shutdown().await;
}

async fn read_messages<R>(
    reader: R,
    inbound: mpsc::Sender<AcpInboundMessage>,
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
                            pending.close(AcpRpcFailure::Closed);
                            return;
                        }
                    }
                    Err(reason) => {
                        let _ = inbound
                            .send(AcpInboundMessage::Malformed {
                                reason,
                                byte_length: line.len(),
                            })
                            .await;
                    }
                },
                Err(_) => {
                    let _ = inbound
                        .send(AcpInboundMessage::Malformed {
                            reason: "message is not valid JSON".to_string(),
                            byte_length: line.len(),
                        })
                        .await;
                }
            },
            Ok(None) => break,
            Err(byte_length) => {
                let _ = inbound
                    .send(AcpInboundMessage::Malformed {
                        reason: "message exceeds the byte limit".to_string(),
                        byte_length,
                    })
                    .await;
            }
        }
    }
    pending.close(AcpRpcFailure::Closed);
    let _ = inbound.send(AcpInboundMessage::Closed).await;
}

enum InboundEnvelope {
    Response {
        id: Value,
        result: Result<Value, AcpRpcFailure>,
    },
    Message(AcpInboundMessage),
}

fn classify_inbound(value: Value) -> Result<InboundEnvelope, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "message must be a JSON object".to_string())?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err("jsonrpc must equal 2.0".to_string());
    }
    if let Some(method) = object.get("method") {
        let method = method
            .as_str()
            .ok_or_else(|| "method must be a string".to_string())?;
        validate_method(method).map_err(|error| error.to_string())?;
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        if let Some(id) = object.get("id") {
            validate_rpc_id(id).map_err(|error| error.to_string())?;
            return Ok(InboundEnvelope::Message(AcpInboundMessage::Request {
                id: id.clone(),
                method: method.to_string(),
                params,
            }));
        }
        return Ok(InboundEnvelope::Message(AcpInboundMessage::Notification {
            method: method.to_string(),
            params,
        }));
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

fn parse_remote_error(value: &Value) -> Result<AcpRpcFailure, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "response error must be an object".to_string())?;
    let code = object
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(|| "response error code must be an integer".to_string())?;
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| "response error message must be a string".to_string())?;
    Ok(AcpRpcFailure::Remote {
        code,
        message: bounded_text(message, MAX_ERROR_BYTES),
    })
}

fn deliver_response(pending: &PendingResponses, id: &Value, result: Result<Value, AcpRpcFailure>) {
    let Ok(key) = rpc_id_key(id) else {
        return;
    };
    pending.deliver(&key, result);
}

fn validate_method(method: &str) -> Result<(), AcpRpcFailure> {
    if method.is_empty() || method.len() > MAX_METHOD_BYTES || method.chars().any(char::is_control)
    {
        Err(AcpRpcFailure::Malformed("method is invalid".to_string()))
    } else {
        Ok(())
    }
}

fn validate_rpc_id(id: &Value) -> Result<(), AcpRpcFailure> {
    match id {
        Value::Number(_) => Ok(()),
        Value::String(value)
            if !value.is_empty()
                && value.len() <= MAX_RPC_ID_BYTES
                && !value.chars().any(char::is_control) =>
        {
            Ok(())
        }
        _ => Err(AcpRpcFailure::Malformed(
            "request id is invalid".to_string(),
        )),
    }
}

fn rpc_id_key(id: &Value) -> Result<String, AcpRpcFailure> {
    validate_rpc_id(id)?;
    serde_json::to_string(id)
        .map_err(|_| AcpRpcFailure::Malformed("request id is invalid".to_string()))
}

pub(super) async fn read_bounded_line<R>(
    reader: &mut BufReader<R>,
) -> Result<Option<Vec<u8>>, usize>
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
            if line.len().saturating_add(consumed) > MAX_MESSAGE_BYTES + 1 {
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

fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}
