//! Host tools through which a running teammate asks a person for a secret value and learns
//! whether it arrived. The tools never carry the value: the teammate reads it from the
//! private file the request names, once the person provided it.

use super::delegation::required_text;
use super::{HostToolContext, generic_denial_error};
use crate::chat::ingestion::notify_rows_changed;
use crate::chat::internal_mcp::InternalMcpRunScope;
use crate::chat::secrets::{
    MAX_SECRET_LABEL_CHARS, MAX_SECRET_PURPOSE_CHARS, open_request, request_state,
    secret_file_path, secret_root,
};
use chrono::{DateTime, Utc};
use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatProjectionKey, ChatResult};
use serde_json::{Map, Value, json};
use sqlx::SqlitePool;
use std::path::Path;

/// Longest request id the status tool accepts.
const MAX_REQUEST_ID_CHARS: usize = 64;

/// A secret tool call the teammate may correct. The code is the error message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SecretToolRefusal {
    /// The run already has [`crate::chat::secrets::MAX_OPEN_SECRET_REQUESTS_PER_RUN`] open requests.
    OpenRequestLimit,
}

impl SecretToolRefusal {
    const ALL: [Self; 1] = [Self::OpenRequestLimit];

    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::OpenRequestLimit => "secret_request_limit",
        }
    }

    pub(super) fn into_error(self) -> ChatError {
        ChatError::new(ChatErrorCode::Permission, self.code(), false)
    }

    pub(super) fn from_error(error: &ChatError) -> Option<Self> {
        if error.code != ChatErrorCode::Permission {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|refusal| refusal.code() == error.message)
    }
}

/// The input properties of the request tool.
pub(super) fn request_properties() -> Value {
    json!({
        "label": { "type": "string", "minLength": 1, "maxLength": MAX_SECRET_LABEL_CHARS },
        "purpose": { "type": "string", "minLength": 1, "maxLength": MAX_SECRET_PURPOSE_CHARS }
    })
}

/// The input properties of the status tool.
pub(super) fn status_properties() -> Value {
    json!({
        "requestId": { "type": "string", "minLength": 1, "maxLength": MAX_REQUEST_ID_CHARS }
    })
}

/// Opens a secret request for the calling run and tells its reply thread to show it.
pub(super) async fn request_secret(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    let root = secret_root(context.app)?;
    let opened = request_secret_in(context.pool, &root, context.scope, arguments, Utc::now()).await;
    if opened.is_ok() {
        notify_rows_changed(
            context.app,
            context.pool,
            context.thread_id,
            &[ChatProjectionKey::Organizational],
        )
        .await;
    }
    opened
}

/// Reports the state of one of the calling run's secret requests.
pub(super) async fn secret_status(
    context: &HostToolContext<'_>,
    arguments: &Map<String, Value>,
) -> ChatResult<Value> {
    secret_status_in(context.pool, context.scope, arguments, Utc::now()).await
}

pub(super) async fn request_secret_in(
    pool: &SqlitePool,
    root: &Path,
    scope: &InternalMcpRunScope,
    arguments: &Map<String, Value>,
    now: DateTime<Utc>,
) -> ChatResult<Value> {
    if arguments.len() != 2 {
        return Err(generic_denial_error());
    }
    let label = required_text(arguments, "label", MAX_SECRET_LABEL_CHARS)?;
    if label.contains('\n') || label.contains('\t') {
        return Err(generic_denial_error());
    }
    let purpose = required_text(arguments, "purpose", MAX_SECRET_PURPOSE_CHARS)?;
    let run_id = scope.run_id.as_str();
    let request_id = open_request(
        pool,
        run_id,
        scope.assignment_id.as_str(),
        &label,
        &purpose,
        now,
    )
    .await?
    .ok_or_else(|| SecretToolRefusal::OpenRequestLimit.into_error())?;
    let path = secret_file_path(root, run_id, &request_id);
    Ok(json!({
        "requestId": request_id,
        "state": "pending",
        "path": path.to_string_lossy(),
    }))
}

pub(super) async fn secret_status_in(
    pool: &SqlitePool,
    scope: &InternalMcpRunScope,
    arguments: &Map<String, Value>,
    now: DateTime<Utc>,
) -> ChatResult<Value> {
    if arguments.len() != 1 {
        return Err(generic_denial_error());
    }
    let request_id = required_text(arguments, "requestId", MAX_REQUEST_ID_CHARS)?;
    let state = request_state(pool, scope.run_id.as_str(), &request_id, now)
        .await?
        .ok_or_else(generic_denial_error)?;
    Ok(json!({ "requestId": request_id, "state": state }))
}
