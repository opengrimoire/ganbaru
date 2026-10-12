//! Typed and bounded HTTP boundary for the OpenCode server API.

use super::config::OpenCodeSecret;
use super::permissions::OpenCodePermissionRule;
use super::protocol::{
    MAX_HTTP_BODY_BYTES, OpenCodeCommand, parse_commands, protocol_error, validate_identifier,
};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{
    Method, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_HISTORY_LIMIT: u32 = 1_000;

#[derive(Clone)]
pub struct OpenCodeHttpClient {
    http: reqwest::Client,
    events_http: reqwest::Client,
    origin: Url,
    workspace: String,
    authorization: Option<HeaderValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeHistoryPage {
    pub messages: Vec<Value>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenCodeSessionLookup {
    Found(Value),
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodePromptModel {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OpenCodePromptPart {
    Text {
        text: String,
    },
    File {
        url: String,
        mime: String,
        filename: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodePrompt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenCodePromptModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub parts: Vec<OpenCodePromptPart>,
}

impl OpenCodeHttpClient {
    pub fn new(
        origin: &str,
        workspace: &Path,
        password: Option<&OpenCodeSecret>,
    ) -> ChatResult<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let origin = Url::parse(origin).map_err(|_| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "OpenCode server origin is invalid",
                true,
            )
        })?;
        if !workspace.is_absolute() {
            return Err(ChatError::validation(
                "workspace",
                "OpenCode workspace must be an absolute path",
            ));
        }
        let workspace = workspace.to_str().ok_or_else(|| {
            ChatError::validation("workspace", "OpenCode workspace path is not valid Unicode")
        })?;
        let authorization = password.map(basic_authorization).transpose()?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::TransportUnavailable,
                    "OpenCode HTTP client could not be created",
                    true,
                )
            })?;
        let events_http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::TransportUnavailable,
                    "OpenCode event client could not be created",
                    true,
                )
            })?;
        Ok(Self {
            http,
            events_http,
            origin,
            workspace: workspace.to_string(),
            authorization,
        })
    }

    pub async fn provider_inventory(&self) -> ChatResult<Value> {
        self.json(Method::GET, &["provider"], &[], Option::<&Value>::None)
            .await
    }

    pub async fn health(&self) -> ChatResult<Value> {
        self.json(
            Method::GET,
            &["global", "health"],
            &[],
            Option::<&Value>::None,
        )
        .await
    }

    pub async fn agents(&self) -> ChatResult<Value> {
        self.json(Method::GET, &["agent"], &[], Option::<&Value>::None)
            .await
    }

    pub async fn commands(&self) -> ChatResult<Vec<OpenCodeCommand>> {
        parse_commands(
            self.json(Method::GET, &["command"], &[], Option::<&Value>::None)
                .await?,
        )
    }

    pub async fn lsp_status(&self) -> ChatResult<Value> {
        self.json(Method::GET, &["lsp"], &[], Option::<&Value>::None)
            .await
    }

    pub async fn formatter_status(&self) -> ChatResult<Value> {
        self.json(Method::GET, &["formatter"], &[], Option::<&Value>::None)
            .await
    }

    pub async fn subscribe_events(&self) -> ChatResult<reqwest::Response> {
        let mut url = endpoint_url(&self.origin, &["event"])?;
        url.query_pairs_mut()
            .append_pair("directory", &self.workspace);
        let mut request = self
            .events_http
            .get(url)
            .header(ACCEPT, "text/event-stream");
        if let Some(authorization) = self.authorization.as_ref() {
            request = request.header(AUTHORIZATION, authorization.clone());
        }
        let mut response = request.send().await.map_err(transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = read_capped_body(&mut response, MAX_ERROR_BODY_BYTES).await?;
            return Err(status_error(status, &body));
        }
        let valid_content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
        if !valid_content_type {
            return Err(protocol_error("event stream content type"));
        }
        Ok(response)
    }

    pub async fn create_session(
        &self,
        permission: Option<&[OpenCodePermissionRule]>,
    ) -> ChatResult<Value> {
        let body = permission
            .map(|rules| json!({ "permission": rules }))
            .unwrap_or_else(|| json!({}));
        self.json(Method::POST, &["session"], &[], Some(&body))
            .await
    }

    pub async fn add_mcp_server(
        &self,
        name: &str,
        url: &str,
        bearer_token: &str,
    ) -> ChatResult<Value> {
        if name.is_empty()
            || name.len() > 240
            || name.chars().any(char::is_control)
            || !url.starts_with("http://127.0.0.1:")
            || bearer_token.len() != 64
        {
            return Err(ChatError::validation(
                "internalMcp",
                "OpenCode MCP server configuration is invalid",
            ));
        }
        self.json(
            Method::POST,
            &["mcp"],
            &[],
            Some(&json!({
                "name": name,
                "config": {
                    "type": "remote",
                    "url": url,
                    "headers": {
                        "Authorization": format!("Bearer {bearer_token}")
                    },
                    "oauth": false
                }
            })),
        )
        .await
    }

    pub async fn session(&self, session_id: &str) -> ChatResult<OpenCodeSessionLookup> {
        validate_identifier(session_id, "session ID")?;
        let response = self
            .send(
                Method::GET,
                &["session", session_id],
                &[],
                Option::<&Value>::None,
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(OpenCodeSessionLookup::NotFound);
        }
        Ok(OpenCodeSessionLookup::Found(
            decode_json_response(response, "session").await?,
        ))
    }

    pub async fn update_permission(
        &self,
        session_id: &str,
        permission: &[OpenCodePermissionRule],
    ) -> ChatResult<Value> {
        validate_identifier(session_id, "session ID")?;
        self.json(
            Method::PATCH,
            &["session", session_id],
            &[],
            Some(&json!({ "permission": permission })),
        )
        .await
    }

    pub async fn fork_session(
        &self,
        session_id: &str,
        message_id: Option<&str>,
    ) -> ChatResult<Value> {
        validate_identifier(session_id, "session ID")?;
        if let Some(message_id) = message_id {
            validate_identifier(message_id, "message ID")?;
        }
        self.json(
            Method::POST,
            &["session", session_id, "fork"],
            &[],
            Some(&json!({ "messageID": message_id })),
        )
        .await
    }

    pub async fn prompt_async(&self, session_id: &str, prompt: &OpenCodePrompt) -> ChatResult<()> {
        validate_identifier(session_id, "session ID")?;
        let response = self
            .send(
                Method::POST,
                &["session", session_id, "prompt_async"],
                &[],
                Some(prompt),
            )
            .await?;
        expect_empty_success(response, StatusCode::NO_CONTENT, "asynchronous prompt").await
    }

    pub async fn execute_command(
        &self,
        session_id: &str,
        command: &str,
        arguments: &str,
    ) -> ChatResult<Value> {
        validate_identifier(session_id, "session ID")?;
        validate_identifier(command, "command")?;
        if arguments.len() > MAX_HTTP_BODY_BYTES || arguments.contains('\0') {
            return Err(ChatError::validation(
                "arguments",
                "OpenCode command arguments exceed the supported size",
            ));
        }
        let body = json!({ "command": command, "arguments": arguments });
        let mut url = endpoint_url(&self.origin, &["session", session_id, "command"])?;
        url.query_pairs_mut()
            .append_pair("directory", &self.workspace);
        let encoded = serde_json::to_vec(&body).map_err(|_| protocol_error("command request"))?;
        let mut request = self
            .events_http
            .post(url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .body(encoded);
        if let Some(authorization) = self.authorization.as_ref() {
            request = request.header(AUTHORIZATION, authorization.clone());
        }
        decode_json_response(request.send().await.map_err(transport_error)?, "command").await
    }

    pub async fn abort(&self, session_id: &str) -> ChatResult<()> {
        validate_identifier(session_id, "session ID")?;
        let value = self
            .json(
                Method::POST,
                &["session", session_id, "abort"],
                &[],
                Option::<&Value>::None,
            )
            .await?;
        expect_true(value, "abort")
    }

    pub async fn messages(
        &self,
        session_id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> ChatResult<OpenCodeHistoryPage> {
        validate_identifier(session_id, "session ID")?;
        if limit == 0 || limit > MAX_HISTORY_LIMIT {
            return Err(ChatError::validation(
                "limit",
                "OpenCode history limit is outside the supported range",
            ));
        }
        if let Some(cursor) = before {
            validate_identifier(cursor, "history cursor")?;
        }
        let limit_text = limit.to_string();
        let mut query = vec![("limit", limit_text.as_str())];
        if let Some(cursor) = before {
            query.push(("before", cursor));
        }
        let response = self
            .send(
                Method::GET,
                &["session", session_id, "message"],
                &query,
                Option::<&Value>::None,
            )
            .await?;
        let next_cursor = response
            .headers()
            .get("X-Next-Cursor")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if let Some(cursor) = next_cursor.as_deref() {
            validate_identifier(cursor, "history cursor")?;
        }
        let value = decode_json_response(response, "message history").await?;
        let messages = value
            .as_array()
            .cloned()
            .ok_or_else(|| protocol_error("message history"))?;
        Ok(OpenCodeHistoryPage {
            messages,
            next_cursor,
        })
    }

    pub async fn reply_permission(&self, request_id: &str, reply: &str) -> ChatResult<()> {
        validate_identifier(request_id, "permission request ID")?;
        let value = self
            .json(
                Method::POST,
                &["permission", request_id, "reply"],
                &[],
                Some(&json!({ "reply": reply })),
            )
            .await?;
        expect_true(value, "permission reply")
    }

    pub async fn reply_question(
        &self,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> ChatResult<()> {
        validate_identifier(request_id, "question request ID")?;
        let value = self
            .json(
                Method::POST,
                &["question", request_id, "reply"],
                &[],
                Some(&json!({ "answers": answers })),
            )
            .await?;
        expect_true(value, "question reply")
    }

    pub async fn reject_question(&self, request_id: &str) -> ChatResult<()> {
        validate_identifier(request_id, "question request ID")?;
        let value = self
            .json(
                Method::POST,
                &["question", request_id, "reject"],
                &[],
                Option::<&Value>::None,
            )
            .await?;
        expect_true(value, "question rejection")
    }

    pub async fn revert(
        &self,
        session_id: &str,
        message_id: &str,
        part_id: Option<&str>,
    ) -> ChatResult<Value> {
        validate_identifier(session_id, "session ID")?;
        validate_identifier(message_id, "message ID")?;
        if let Some(part_id) = part_id {
            validate_identifier(part_id, "part ID")?;
        }
        self.json(
            Method::POST,
            &["session", session_id, "revert"],
            &[],
            Some(&json!({ "messageID": message_id, "partID": part_id })),
        )
        .await
    }

    async fn json<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, &str)],
        body: Option<&T>,
    ) -> ChatResult<Value> {
        decode_json_response(self.send(method, path, query, body).await?, "response").await
    }

    async fn send<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &[&str],
        query: &[(&str, &str)],
        body: Option<&T>,
    ) -> ChatResult<reqwest::Response> {
        let mut url = endpoint_url(&self.origin, path)?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("directory", &self.workspace);
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        let mut request = self
            .http
            .request(method, url)
            .header(ACCEPT, "application/json");
        if let Some(authorization) = self.authorization.as_ref() {
            request = request.header(AUTHORIZATION, authorization.clone());
        }
        if let Some(body) = body {
            let encoded = serde_json::to_vec(body).map_err(|_| protocol_error("request body"))?;
            if encoded.len() > MAX_HTTP_BODY_BYTES {
                return Err(ChatError::validation(
                    "request",
                    "OpenCode request exceeds the supported size",
                ));
            }
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(encoded);
        }
        request.send().await.map_err(transport_error)
    }
}

fn endpoint_url(origin: &Url, path: &[&str]) -> ChatResult<Url> {
    let mut url = origin.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| protocol_error("server origin"))?;
        segments.clear();
        for segment in path {
            validate_identifier(segment, "path segment")?;
            if segment == &"." || segment == &".." || segment.contains('/') {
                return Err(protocol_error("path segment"));
            }
            segments.push(segment);
        }
    }
    Ok(url)
}

fn basic_authorization(password: &OpenCodeSecret) -> ChatResult<HeaderValue> {
    let encoded = STANDARD.encode(format!("opencode:{}", password.expose()));
    let mut value = HeaderValue::from_str(&format!("Basic {encoded}")).map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "OpenCode server password is invalid",
            true,
        )
    })?;
    value.set_sensitive(true);
    Ok(value)
}

async fn decode_json_response(mut response: reqwest::Response, label: &str) -> ChatResult<Value> {
    let status = response.status();
    let max_body_bytes = if status.is_success() {
        MAX_HTTP_BODY_BYTES
    } else {
        MAX_ERROR_BODY_BYTES
    };
    let body = read_capped_body(&mut response, max_body_bytes).await?;
    if !status.is_success() {
        return Err(status_error(status, &body));
    }
    serde_json::from_slice(&body).map_err(|_| protocol_error(label))
}

async fn expect_empty_success(
    mut response: reqwest::Response,
    expected: StatusCode,
    label: &str,
) -> ChatResult<()> {
    let status = response.status();
    let body = read_capped_body(&mut response, MAX_ERROR_BODY_BYTES).await?;
    if status != expected {
        return Err(status_error(status, &body));
    }
    if !body.is_empty() {
        return Err(protocol_error(label));
    }
    Ok(())
}

async fn read_capped_body(
    response: &mut reqwest::Response,
    max_bytes: usize,
) -> ChatResult<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(protocol_error("HTTP response size"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(protocol_error("HTTP response size"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn status_error(status: StatusCode, body: &[u8]) -> ChatError {
    let code = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ChatErrorCode::AuthenticationRequired,
        StatusCode::NOT_FOUND => ChatErrorCode::NotFound,
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => ChatErrorCode::Timeout,
        status if status.is_server_error() => ChatErrorCode::TransportUnavailable,
        _ => ChatErrorCode::Protocol,
    };
    let provider_error_name = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| sanitized_error_name(&value));
    let mut error = ChatError::new(
        code,
        match code {
            ChatErrorCode::AuthenticationRequired => {
                "OpenCode server authentication failed".to_string()
            }
            ChatErrorCode::NotFound => "OpenCode resource was not found".to_string(),
            ChatErrorCode::Timeout => "OpenCode server request timed out".to_string(),
            ChatErrorCode::TransportUnavailable => {
                "OpenCode server is temporarily unavailable".to_string()
            }
            _ => format!(
                "OpenCode server rejected the request with status {}",
                status.as_u16()
            ),
        },
        code != ChatErrorCode::Protocol,
    );
    error.details = provider_error_name.map(|name| Box::new(json!({ "providerError": name })));
    error
}

fn sanitized_error_name(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("name"))
        .and_then(Value::as_str)
        .filter(|name| name.len() <= 128 && name.chars().all(|character| !character.is_control()))
        .map(str::to_string)
}

fn transport_error(error: reqwest::Error) -> ChatError {
    let code = if error.is_timeout() {
        ChatErrorCode::Timeout
    } else {
        ChatErrorCode::TransportUnavailable
    };
    ChatError::new(
        code,
        if error.is_timeout() {
            "OpenCode server request timed out"
        } else {
            "OpenCode server connection failed"
        },
        true,
    )
}

fn expect_true(value: Value, label: &str) -> ChatResult<()> {
    if value == Value::Bool(true) {
        Ok(())
    } else {
        Err(protocol_error(label))
    }
}
