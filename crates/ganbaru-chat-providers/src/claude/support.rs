//! Claude lifecycle inputs, probes, capabilities, and identifiers.

use super::driver::{
    CLAUDE_STDERR_LIMIT_BYTES, NEXT_SESSION_ID, NEXT_UUID, SESSION_FORCE_STOP,
    SESSION_GRACEFUL_STOP, VERSION_OUTPUT_LIMIT_BYTES,
};
use super::home::*;
use super::protocol::*;
use super::transport::ClaudeJsonlConnection;
use crate::DriverOperationContext;
use crate::models::*;
use crate::process::{ProviderProcessConfig, spawn_provider_process};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tokio::io::AsyncReadExt;

pub(super) enum SessionOpenInput {
    Fresh(StartSessionRequest),
    Resume(ResumeSessionRequest),
}

impl SessionOpenInput {
    pub(super) fn provider_instance_id(&self) -> &ProviderInstanceId {
        match self {
            Self::Fresh(request) => &request.provider_instance_id,
            Self::Resume(request) => &request.provider_instance_id,
        }
    }

    pub(super) fn thread_id(&self) -> &ChatThreadId {
        match self {
            Self::Fresh(request) => &request.thread_id,
            Self::Resume(request) => &request.thread_id,
        }
    }

    pub(super) fn workspace(&self) -> &VerifiedWorkspaceContext {
        match self {
            Self::Fresh(request) => &request.workspace,
            Self::Resume(request) => &request.workspace,
        }
    }

    pub(super) fn modes(&self) -> TurnModeSnapshot {
        match self {
            Self::Fresh(request) => request.modes,
            Self::Resume(request) => request.modes,
        }
    }

    pub(super) fn model_id(&self) -> Option<&ModelId> {
        match self {
            Self::Fresh(request) => request.model_id.as_ref(),
            Self::Resume(_) => None,
        }
    }

    pub(super) fn model_options(&self) -> &[ModelOptionSelection] {
        match self {
            Self::Fresh(request) => &request.model_options,
            Self::Resume(_) => &[],
        }
    }

    pub(super) fn is_resume(&self) -> bool {
        matches!(self, Self::Resume(_))
    }

    pub(super) fn resume_cursor(
        &self,
        instance_id: &ProviderInstanceId,
    ) -> ChatResult<ClaudeResumeCursor> {
        match self {
            Self::Fresh(_) => Ok(ClaudeResumeCursor {
                session_uuid: new_uuid(instance_id),
                last_assistant_uuid: None,
                turn_count: 0,
            }),
            Self::Resume(request) => {
                let cursor = parse_resume_cursor(&request.resume_cursor)?;
                if cursor.session_uuid != request.provider_thread_id.as_str() {
                    return Err(ChatError::validation(
                        "resumeCursor",
                        "Claude resume cursor does not match the provider session",
                    ));
                }
                Ok(cursor)
            }
        }
    }

    pub(super) fn verify_continuation(&self, actual: &ContinuationGroupId) -> ChatResult<()> {
        let Self::Resume(request) = self else {
            return Ok(());
        };
        if &request.continuation_group_id != actual {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Claude config directory is incompatible with this thread. Fork the thread to continue.",
                true,
            ));
        }
        Ok(())
    }
}

pub(super) async fn initialize(
    connection: &ClaudeJsonlConnection,
    context: &DriverOperationContext,
) -> ChatResult<ClaudeInitializeResponse> {
    let response = connection
        .client()
        .request("initialize", json!({ "supportedDialogKinds": [] }), context)
        .await
        .map_err(|error| error.to_chat_error("initialize"))?;
    decode_initialize(response)
}

pub(super) async fn probe_version(
    executable: &ResolvedClaudeExecutable,
    working_directory: &Path,
    environment: std::collections::BTreeMap<String, String>,
) -> ChatResult<ClaudeVersion> {
    let mut arguments = executable.prefix_arguments.clone();
    arguments.push("--version".to_string());
    let mut process = spawn_provider_process(ProviderProcessConfig {
        executable: executable.executable.clone(),
        arguments,
        working_directory: working_directory.to_path_buf(),
        environment,
        stderr_limit_bytes: CLAUDE_STDERR_LIMIT_BYTES,
    })?;
    let mut stdout = process.take_stdout()?.take(VERSION_OUTPUT_LIMIT_BYTES);
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).await.map_err(|_| {
        ChatError::new(
            ChatErrorCode::TransportUnavailable,
            "Claude version output could not be read",
            true,
        )
    })?;
    process
        .stop(SESSION_GRACEFUL_STOP, SESSION_FORCE_STOP)
        .await?;
    parse_version(&String::from_utf8_lossy(&output))
}

pub(super) fn claude_capability_kinds() -> Vec<ProviderCapability> {
    vec![
        ProviderCapability::NativeResume,
        ProviderCapability::NativePlan,
        ProviderCapability::Steering,
        ProviderCapability::DynamicModelChange,
        ProviderCapability::Images,
        ProviderCapability::FileReferences,
        ProviderCapability::Skills,
        ProviderCapability::SlashCommands,
        ProviderCapability::Approvals,
        ProviderCapability::StructuredQuestions,
        ProviderCapability::ReasoningSummaries,
        ProviderCapability::StructuredPlans,
        ProviderCapability::ContextUsage,
        ProviderCapability::CostReporting,
        ProviderCapability::McpStatus,
        ProviderCapability::AccountStatus,
        ProviderCapability::TaskActivity,
    ]
}

pub(super) fn claude_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        entries: claude_capability_kinds()
            .into_iter()
            .map(|capability| ProviderCapabilitySupport {
                capability,
                supported: true,
                explanation: None,
            })
            .collect(),
    }
}

pub(super) fn canonical_verified_workspace(
    workspace: &VerifiedWorkspaceContext,
) -> ChatResult<PathBuf> {
    let path = PathBuf::from(&workspace.canonical_path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(ChatError::validation(
            "workspace",
            "Claude workspace binding is invalid",
        ));
    }
    std::fs::canonicalize(path).map_err(|_| {
        ChatError::validation("workspace", "Claude workspace could not be canonicalized")
    })
}

pub(super) fn canonical_current_directory() -> ChatResult<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|path| std::fs::canonicalize(path).ok())
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                "Claude probe working directory is unavailable",
                true,
            )
        })
}

pub(super) fn new_uuid(instance_id: &ProviderInstanceId) -> String {
    let sequence = NEXT_UUID.fetch_add(1, Ordering::Relaxed);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-claude-session-v1\0");
    digest.update(instance_id.as_str().as_bytes());
    digest.update(now.to_le_bytes());
    digest.update(sequence.to_le_bytes());
    let mut bytes = digest.finalize()[..16].to_vec();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

pub(super) fn new_session_id(instance_id: &ProviderInstanceId) -> ChatResult<ProviderSessionId> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    ProviderSessionId::new(format!(
        "claude-{}-{now:x}-{sequence:x}",
        instance_id.as_str()
    ))
    .map_err(|_| protocol_identifier_error("session"))
}

pub(super) fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Claude timestamp could not be created",
            false,
        )
    })
}

pub(super) fn confirmed_resume_not_found(error: &ChatError) -> bool {
    error.code == ChatErrorCode::Protocol && diagnostic_confirms_resume_not_found(&error.message)
}

pub(super) fn diagnostic_confirms_resume_not_found(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("session")
        && [
            "not found",
            "does not exist",
            "missing session",
            "unknown session",
        ]
        .iter()
        .any(|fragment| value.contains(fragment))
}

pub(super) fn resume_not_found_error(diagnostic: &str) -> ChatError {
    let mut error = ChatError::new(
        ChatErrorCode::ResumeNotFound,
        "Claude continuation was not found; fork the thread to start a fresh native session",
        true,
    );
    error.details = Some(Box::new(json!({
        "diagnosticAvailable": !diagnostic.trim().is_empty(),
    })));
    error
}

pub(super) fn probe_state_for_error(code: ChatErrorCode) -> ProbeState {
    match code {
        ChatErrorCode::ExecutableMissing => ProbeState::ExecutableMissing,
        ChatErrorCode::UnsupportedVersion | ChatErrorCode::Protocol => {
            ProbeState::UnsupportedVersion
        }
        ChatErrorCode::AuthenticationRequired => ProbeState::AuthenticationRequired,
        ChatErrorCode::ConfigurationInvalid | ChatErrorCode::Validation => {
            ProbeState::ConfigurationInvalid
        }
        _ => ProbeState::TransportUnavailable,
    }
}

pub(super) fn probe_detail(code: ChatErrorCode) -> &'static str {
    match probe_state_for_error(code) {
        ProbeState::ExecutableMissing => "Claude executable is unavailable",
        ProbeState::UnsupportedVersion => "Claude SDK protocol version is unsupported",
        ProbeState::AuthenticationRequired => "Claude authentication is required",
        ProbeState::ConfigurationInvalid => "Claude configuration is invalid",
        ProbeState::TransportUnavailable => "Claude SDK transport is unavailable",
        ProbeState::Healthy => "Claude is ready",
    }
}

pub(super) fn operation_receipt(operation_id: &str, detail: &str) -> DriverOperationReceipt {
    DriverOperationReceipt {
        accepted: true,
        operation_id: operation_id.to_string(),
        detail: Some(detail.to_string()),
    }
}

pub(super) fn protocol_identifier_error(label: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Claude returned an invalid {label} identifier"),
        false,
    )
}

pub(super) fn driver_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Claude driver state is unavailable",
        false,
    )
}
