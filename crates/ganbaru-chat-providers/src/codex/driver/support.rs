use super::*;

pub(crate) fn validated_app_server_arguments(arguments: &[String]) -> ChatResult<Vec<String>> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--strict-config" {
            output.push(argument.clone());
            index += 1;
            continue;
        }
        if argument.starts_with("--config=")
            || argument.starts_with("-c=")
            || argument.starts_with("--enable=")
            || argument.starts_with("--disable=")
        {
            output.push(argument.clone());
            index += 1;
            continue;
        }
        if matches!(
            argument.as_str(),
            "--config" | "-c" | "--enable" | "--disable"
        ) {
            let value = arguments.get(index + 1).ok_or_else(|| {
                ChatError::validation("launchArguments", "Codex option requires a value")
            })?;
            if value.is_empty() || value.starts_with('-') {
                return Err(ChatError::validation(
                    "launchArguments",
                    "Codex option value is invalid",
                ));
            }
            output.push(argument.clone());
            output.push(value.clone());
            index += 2;
            continue;
        }
        return Err(ChatError::validation(
            "launchArguments",
            "Codex app-server accepts only configuration and feature arguments",
        ));
    }
    Ok(output)
}

pub(super) async fn interrupt_live_turn(
    live: &mut CodexLiveSession,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let state = live.route.lock().map_err(|_| driver_state_error())?.clone();
    let (Some(provider_thread_id), Some(provider_turn_id)) =
        (state.provider_thread_id, state.active_provider_turn_id)
    else {
        return Ok(());
    };
    live.connection
        .client()
        .request(
            "turn/interrupt",
            json!({ "threadId": provider_thread_id, "turnId": provider_turn_id }),
            context,
        )
        .await
        .map_err(|error| error.to_chat_error("turn interrupt"))?;
    Ok(())
}

pub(super) async fn wait_for_provider_thread(
    mut receiver: watch::Receiver<Option<String>>,
    expected: &str,
    context: &DriverOperationContext,
) -> ChatResult<String> {
    let timeout = context
        .deadline
        .saturating_duration_since(Instant::now())
        .min(PROVIDER_THREAD_NOTIFICATION_TIMEOUT);
    let wait = async {
        loop {
            if let Some(provider_thread) = receiver.borrow().clone() {
                return Ok(provider_thread);
            }
            receiver.changed().await.map_err(|_| {
                ChatError::new(
                    ChatErrorCode::TransportUnavailable,
                    "Codex thread notification stream closed",
                    true,
                )
            })?;
        }
    };
    let provider_thread = tokio::time::timeout(timeout, wait).await.map_err(|_| {
        ChatError::new(
            ChatErrorCode::Protocol,
            "Codex did not confirm the provider thread before the deadline",
            true,
        )
    })??;
    if provider_thread != expected {
        return Err(ChatError::new(
            ChatErrorCode::Protocol,
            "Codex confirmed a different provider thread",
            false,
        ));
    }
    Ok(provider_thread)
}

pub(super) fn provider_history(response: Value, limit: u32) -> ChatResult<ProviderHistoryPage> {
    let limit = usize::try_from(limit)
        .unwrap_or(MAX_HISTORY_ITEMS)
        .min(MAX_HISTORY_ITEMS);
    let thread = response
        .get("thread")
        .and_then(Value::as_object)
        .ok_or_else(|| protocol_identifier_error("history thread"))?;
    let turns = thread
        .get("turns")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol_identifier_error("history turns"))?;
    let mut items = Vec::new();
    for turn in turns {
        let Some(turn) = turn.as_object() else {
            return Err(protocol_identifier_error("history turn"));
        };
        let provider_turn_id = turn
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| ProviderTurnId::new(id.to_string()).ok());
        let Some(turn_items) = turn.get("items").and_then(Value::as_array) else {
            continue;
        };
        for item in turn_items {
            if items.len() >= limit {
                break;
            }
            let object = item
                .as_object()
                .ok_or_else(|| protocol_identifier_error("history item"))?;
            let provider_item_id = object
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| ProviderItemId::new(id.to_string()).ok());
            let kind = object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            items.push(ProviderHistoryItem {
                provider_item_id,
                provider_turn_id: provider_turn_id.clone(),
                kind,
                data: VersionedJson {
                    schema_version: 1,
                    value: item.clone(),
                },
            });
        }
        if items.len() >= limit {
            break;
        }
    }
    Ok(ProviderHistoryPage {
        items,
        next_cursor: None,
    })
}

pub(super) fn canonical_verified_workspace(
    workspace: &VerifiedWorkspaceContext,
) -> ChatResult<PathBuf> {
    let path = PathBuf::from(&workspace.canonical_path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(ChatError::validation(
            "workspace",
            "Codex workspace binding is invalid",
        ));
    }
    std::fs::canonicalize(path).map_err(|_| {
        ChatError::validation("workspace", "Codex workspace could not be canonicalized")
    })
}

pub(super) fn canonical_current_directory() -> ChatResult<PathBuf> {
    let current = std::env::current_dir().map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex probe working directory is unavailable",
            true,
        )
    })?;
    std::fs::canonicalize(current).map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex probe working directory is unavailable",
            true,
        )
    })
}

pub(super) fn verify_reported_home(layout: &CodexHomeLayout, reported: &Path) -> ChatResult<()> {
    let reported = std::fs::canonicalize(reported).map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex reported an unavailable home directory",
            true,
        )
    })?;
    if reported != layout.effective_home {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Codex app-server used a different home directory",
            true,
        ));
    }
    Ok(())
}

pub(super) fn verify_effective_safety(
    mode: SafetyMode,
    response: &ThreadOpenResponse,
) -> ChatResult<()> {
    let Some(expected) = safety_settings(mode) else {
        return verify_custom_safety(response);
    };
    if response.approval_policy.as_str() != Some(expected.approval_policy) {
        return Err(permission_error());
    }
    if response.approvals_reviewer != expected.approvals_reviewer {
        return Err(permission_error());
    }
    let sandbox = response
        .sandbox
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| response.sandbox.as_str());
    if sandbox != Some(expected.turn_sandbox_type) && sandbox != Some(expected.sandbox) {
        return Err(permission_error());
    }
    Ok(())
}

pub(super) fn verify_custom_safety(response: &ThreadOpenResponse) -> ChatResult<()> {
    let approval_valid = response.approval_policy.as_str().is_some()
        || response
            .approval_policy
            .get("granular")
            .is_some_and(Value::is_object);
    let sandbox_valid = response
        .sandbox
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| response.sandbox.as_str())
        .is_some();
    if approval_valid && sandbox_valid && !response.approvals_reviewer.trim().is_empty() {
        Ok(())
    } else {
        Err(permission_error())
    }
}

pub(crate) fn confirmed_resume_not_found(error: &CodexRpcFailure) -> bool {
    let CodexRpcFailure::Remote { code, message } = error else {
        return false;
    };
    if !matches!(*code, -32602 | -32000 | -32001) {
        return false;
    }
    let message = message.to_ascii_lowercase();
    message.contains("thread")
        && [
            "not found",
            "missing thread",
            "no such thread",
            "unknown thread",
            "does not exist",
            "no rollout found",
        ]
        .iter()
        .any(|fragment| message.contains(fragment))
}

pub(super) fn new_session_id(instance_id: &ProviderInstanceId) -> ChatResult<ProviderSessionId> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    ProviderSessionId::new(format!(
        "codex-{}-{now:x}-{sequence:x}",
        instance_id.as_str()
    ))
    .map_err(|_| protocol_identifier_error("session"))
}

pub(super) fn now_utc() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<chrono::Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Internal,
            "Codex timestamp could not be created",
            false,
        )
    })
}

pub(super) fn parse_user_agent_version(user_agent: &str) -> Option<String> {
    let (_, suffix) = user_agent.split_once('/')?;
    suffix.split_whitespace().next().map(str::to_string)
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
        ProbeState::ExecutableMissing => "Codex executable is unavailable",
        ProbeState::UnsupportedVersion => "Codex app-server protocol is unsupported",
        ProbeState::AuthenticationRequired => "Codex authentication is required",
        ProbeState::ConfigurationInvalid => "Codex configuration is invalid",
        ProbeState::TransportUnavailable => "Codex app-server transport is unavailable",
        ProbeState::Healthy => "Codex is ready",
    }
}

pub(super) fn operation_receipt(
    context: &DriverOperationContext,
    detail: &str,
) -> DriverOperationReceipt {
    DriverOperationReceipt {
        accepted: true,
        operation_id: context.operation_id.clone(),
        detail: Some(detail.to_string()),
    }
}

pub(super) fn permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Codex did not confirm the requested approval and sandbox policy",
        false,
    )
}

pub(super) fn protocol_identifier_error(label: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Protocol,
        format!("Codex returned an invalid {label} identifier"),
        false,
    )
}

pub(super) fn driver_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Codex driver state is unavailable",
        false,
    )
}
