use super::*;

pub(super) async fn handle_read_text_file(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: ReadTextFileRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => return reject_callback(resources, rpc_id, "Invalid file read request").await,
    };
    let result = (|| {
        let (workspace, session_id) = route_workspace(resources)?;
        require_acp_session(&request.session_id.to_string(), &session_id)?;
        let path = verified_existing_file(&workspace, &request.path)?;
        let metadata = fs::metadata(&path).map_err(|_| callback_permission_error())?;
        if metadata.len() > MAX_ACP_FILE_BYTES {
            return Err(ChatError::new(
                ChatErrorCode::Protocol,
                "ACP file read exceeds the supported limit",
                true,
            ));
        }
        let text = fs::read_to_string(path).map_err(|_| callback_permission_error())?;
        if text.contains('\0') {
            return Err(ChatError::validation("path", "ACP file is not text"));
        }
        let start = request.line.unwrap_or(1).max(1).saturating_sub(1) as usize;
        let limit = request
            .limit
            .unwrap_or(MAX_ACP_FILE_LINES)
            .min(MAX_ACP_FILE_LINES) as usize;
        let content = text
            .lines()
            .skip(start)
            .take(limit)
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::to_value(ReadTextFileResponse::new(content))
            .map_err(|_| protocol_error("ACP file read response"))
    })();
    match result {
        Ok(response) => resources
            .client
            .respond(rpc_id, response)
            .await
            .map_err(|error| error.to_chat_error("file read response")),
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn handle_write_text_file(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: WriteTextFileRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => return reject_callback(resources, rpc_id, "Invalid file write request").await,
    };
    let result = (|| {
        let (workspace, session_id) = route_workspace(resources)?;
        require_acp_session(&request.session_id.to_string(), &session_id)?;
        if request.content.len() as u64 > MAX_ACP_FILE_BYTES || request.content.contains('\0') {
            return Err(ChatError::validation(
                "content",
                "ACP file write exceeds the supported text limit",
            ));
        }
        let path = verified_write_path(&workspace, &request.path)?;
        atomic_write_text(&path, request.content.as_bytes())?;
        serde_json::to_value(WriteTextFileResponse::new())
            .map_err(|_| protocol_error("ACP file write response"))
    })();
    match result {
        Ok(response) => resources
            .client
            .respond(rpc_id, response)
            .await
            .map_err(|error| error.to_chat_error("file write response")),
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn handle_terminal_create(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: CreateTerminalRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => {
            return reject_callback(resources, rpc_id, "Invalid terminal create request").await;
        }
    };
    let result = create_acp_terminal(&resources.terminal_callbacks, request).await;
    match result {
        Ok(terminal_id) => {
            let response = serde_json::to_value(CreateTerminalResponse::new(terminal_id))
                .map_err(|_| protocol_error("ACP terminal create response"))?;
            resources
                .client
                .respond(rpc_id, response)
                .await
                .map_err(|error| error.to_chat_error("terminal create response"))
        }
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn handle_terminal_output(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: TerminalOutputRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => {
            return reject_callback(resources, rpc_id, "Invalid terminal output request").await;
        }
    };
    let result = (|| {
        resources
            .terminal_callbacks
            .verify_session(&request.session_id.to_string())?;
        let terminal = resources
            .terminal_callbacks
            .terminal(&request.terminal_id.to_string())?;
        terminal_output_response(&terminal)
    })();
    match result {
        Ok(response) => resources
            .client
            .respond(rpc_id, response)
            .await
            .map_err(|error| error.to_chat_error("terminal output response")),
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn handle_terminal_wait(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: WaitForTerminalExitRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => return reject_callback(resources, rpc_id, "Invalid terminal wait request").await,
    };
    resources
        .terminal_callbacks
        .verify_session(&request.session_id.to_string())?;
    let terminal = resources
        .terminal_callbacks
        .terminal(&request.terminal_id.to_string())?;
    let client = resources.client.clone();
    tokio::spawn(async move {
        let exit_status = loop {
            let notified = terminal.completed.notified();
            let current = {
                let state = match terminal.state.lock() {
                    Ok(state) => state,
                    Err(_) => return,
                };
                state.exit_status.clone()
            };
            if let Some(status) = current {
                break status;
            }
            notified.await;
        };
        let Ok(response) = serde_json::to_value(WaitForTerminalExitResponse::new(exit_status))
        else {
            return;
        };
        let _ = client.respond(rpc_id, response).await;
    });
    Ok(())
}

pub(super) async fn handle_terminal_kill(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: KillTerminalRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => return reject_callback(resources, rpc_id, "Invalid terminal kill request").await,
    };
    let result = (|| {
        resources
            .terminal_callbacks
            .verify_session(&request.session_id.to_string())?;
        let terminal = resources
            .terminal_callbacks
            .terminal(&request.terminal_id.to_string())?;
        terminal
            .kill
            .try_send(())
            .map_err(|_| callback_permission_error())?;
        serde_json::to_value(KillTerminalResponse::new())
            .map_err(|_| protocol_error("ACP terminal kill response"))
    })();
    match result {
        Ok(response) => resources
            .client
            .respond(rpc_id, response)
            .await
            .map_err(|error| error.to_chat_error("terminal kill response")),
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn handle_terminal_release(
    resources: &CursorRouterResources,
    rpc_id: Value,
    params: Value,
) -> ChatResult<()> {
    let request: ReleaseTerminalRequest = match serde_json::from_value(params) {
        Ok(request) => request,
        Err(_) => {
            return reject_callback(resources, rpc_id, "Invalid terminal release request").await;
        }
    };
    let result = (|| {
        resources
            .terminal_callbacks
            .verify_session(&request.session_id.to_string())?;
        let terminal = resources
            .terminal_callbacks
            .terminals
            .lock()
            .map_err(|_| driver_state_error())?
            .remove(&request.terminal_id.to_string())
            .ok_or_else(callback_permission_error)?;
        let _ = terminal.kill.try_send(());
        serde_json::to_value(ReleaseTerminalResponse::new())
            .map_err(|_| protocol_error("ACP terminal release response"))
    })();
    match result {
        Ok(response) => resources
            .client
            .respond(rpc_id, response)
            .await
            .map_err(|error| error.to_chat_error("terminal release response")),
        Err(error) => reject_callback(resources, rpc_id, &error.message).await,
    }
}

pub(super) async fn create_acp_terminal(
    callbacks: &AcpTerminalCallbacks,
    request: CreateTerminalRequest,
) -> ChatResult<String> {
    callbacks.verify_session(&request.session_id.to_string())?;
    validate_terminal_command(&request)?;
    let cwd = match request.cwd.as_deref() {
        Some(path) => verified_directory(&callbacks.workspace, path)?,
        None => callbacks.workspace.clone(),
    };
    let output_limit = request
        .output_byte_limit
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(DEFAULT_ACP_TERMINAL_OUTPUT_BYTES)
        .clamp(1, MAX_ACP_TERMINAL_OUTPUT_BYTES);
    let mut command = Command::new(&request.command);
    command
        .args(&request.args)
        .current_dir(cwd)
        .envs(request.env.iter().map(|value| (&value.name, &value.value)))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| {
        ChatError::new(
            ChatErrorCode::DriverUnavailable,
            "ACP terminal command could not be started",
            true,
        )
    })?;
    let stdout = child.stdout.take().ok_or_else(callback_permission_error)?;
    let stderr = child.stderr.take().ok_or_else(callback_permission_error)?;
    let terminal_id = format!(
        "ganbaru-acp-terminal-{}",
        NEXT_ACP_TERMINAL.fetch_add(1, Ordering::Relaxed)
    );
    let state = Arc::new(Mutex::new(AcpTerminalState {
        output: Vec::new(),
        output_limit,
        truncated: false,
        exit_status: None,
    }));
    let completed = Arc::new(Notify::new());
    let (kill, mut kill_receiver) = mpsc::channel(1);
    let terminal = AcpTerminal {
        state: Arc::clone(&state),
        kill,
        completed: Arc::clone(&completed),
    };
    callbacks
        .terminals
        .lock()
        .map_err(|_| driver_state_error())?
        .insert(terminal_id.clone(), terminal);
    let stdout_task = tokio::spawn(capture_terminal_output(stdout, Arc::clone(&state)));
    let stderr_task = tokio::spawn(capture_terminal_output(stderr, Arc::clone(&state)));
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(40));
        let status = loop {
            tokio::select! {
                _ = kill_receiver.recv() => {
                    let _ = child.kill().await;
                    break child.wait().await.ok();
                }
                _ = interval.tick() => match child.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => {}
                    Err(_) => break None,
                }
            }
        };
        let _ = stdout_task.await;
        let _ = stderr_task.await;
        if let Ok(mut state) = state.lock() {
            state.exit_status = Some(match status.and_then(|status| status.code()) {
                Some(code) if code >= 0 => TerminalExitStatus::new().exit_code(code as u32),
                _ => TerminalExitStatus::new().signal("terminated".to_string()),
            });
        }
        completed.notify_waiters();
    });
    Ok(terminal_id)
}

pub(super) async fn capture_terminal_output<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    state: Arc<Mutex<AcpTerminalState>>,
) {
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let Ok(mut state) = state.lock() else { break };
        append_terminal_bytes(&mut state, &buffer[..read]);
    }
}

pub(super) fn append_terminal_bytes(state: &mut AcpTerminalState, bytes: &[u8]) {
    state.output.extend_from_slice(bytes);
    let excess = state.output.len().saturating_sub(state.output_limit);
    if excess > 0 {
        state.output.drain(..excess);
        state.truncated = true;
    }
}

pub(super) fn terminal_output_response(terminal: &AcpTerminal) -> ChatResult<Value> {
    let state = terminal.state.lock().map_err(|_| driver_state_error())?;
    let output = String::from_utf8_lossy(&state.output).into_owned();
    let response =
        TerminalOutputResponse::new(output, state.truncated).exit_status(state.exit_status.clone());
    serde_json::to_value(response).map_err(|_| protocol_error("ACP terminal output response"))
}

pub(super) fn validate_terminal_command(request: &CreateTerminalRequest) -> ChatResult<()> {
    if request.command.is_empty()
        || request.command.len() > 4_096
        || request.command.chars().any(|character| character == '\0')
        || request.args.len() > 10_000
        || request
            .args
            .iter()
            .any(|argument| argument.len() > 65_536 || argument.contains('\0'))
        || request.env.len() > 1_024
        || request.env.iter().any(|value| {
            value.name.is_empty()
                || value.name.len() > 1_024
                || value
                    .name
                    .chars()
                    .any(|character| matches!(character, '=' | '\0'))
                || value.value.len() > 65_536
                || value.value.contains('\0')
        })
    {
        return Err(ChatError::validation(
            "command",
            "ACP terminal command is invalid",
        ));
    }
    Ok(())
}

pub(super) fn verified_directory(workspace: &Path, requested: &Path) -> ChatResult<PathBuf> {
    let relative = verified_relative_path(workspace, requested)?;
    reject_symlink_components(workspace, relative, false)?;
    let canonical = fs::canonicalize(requested).map_err(|_| callback_permission_error())?;
    if !canonical.starts_with(workspace) || !canonical.is_dir() {
        return Err(callback_permission_error());
    }
    Ok(canonical)
}

pub(super) fn route_workspace(resources: &CursorRouterResources) -> ChatResult<(PathBuf, String)> {
    let route = resources.route.lock().map_err(|_| driver_state_error())?;
    Ok((route.workspace.clone(), route.provider_thread_id.clone()))
}

pub(super) fn require_acp_session(actual: &str, expected: &str) -> ChatResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(callback_permission_error())
    }
}

pub(super) fn verified_existing_file(workspace: &Path, requested: &Path) -> ChatResult<PathBuf> {
    let relative = verified_relative_path(workspace, requested)?;
    reject_symlink_components(workspace, relative, false)?;
    let canonical = fs::canonicalize(requested).map_err(|_| callback_permission_error())?;
    if !canonical.starts_with(workspace) || !canonical.is_file() {
        return Err(callback_permission_error());
    }
    Ok(canonical)
}

pub(super) fn verified_write_path(workspace: &Path, requested: &Path) -> ChatResult<PathBuf> {
    let relative = verified_relative_path(workspace, requested)?;
    reject_symlink_components(workspace, relative, true)?;
    let parent = requested.parent().ok_or_else(callback_permission_error)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| callback_permission_error())?;
    if !canonical_parent.starts_with(workspace) {
        return Err(callback_permission_error());
    }
    if requested.exists() {
        let metadata = fs::symlink_metadata(requested).map_err(|_| callback_permission_error())?;
        if !metadata.file_type().is_file() {
            return Err(callback_permission_error());
        }
    }
    Ok(requested.to_path_buf())
}

pub(super) fn verified_relative_path<'a>(
    workspace: &Path,
    requested: &'a Path,
) -> ChatResult<&'a Path> {
    if !requested.is_absolute() {
        return Err(callback_permission_error());
    }
    let relative = requested
        .strip_prefix(workspace)
        .map_err(|_| callback_permission_error())?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(callback_permission_error());
    }
    Ok(relative)
}

pub(super) fn reject_symlink_components(
    workspace: &Path,
    relative: &Path,
    allow_missing_file: bool,
) -> ChatResult<()> {
    let mut current = workspace.to_path_buf();
    let component_count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(callback_permission_error());
            }
            Ok(_) => {}
            Err(_) if allow_missing_file && index + 1 == component_count => {}
            Err(_) => return Err(callback_permission_error()),
        }
    }
    Ok(())
}

pub(super) fn atomic_write_text(path: &Path, content: &[u8]) -> ChatResult<()> {
    let parent = path.parent().ok_or_else(callback_permission_error)?;
    let permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    let sequence = NEXT_ACP_FILE_WRITE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".ganbaru-acp-write-{}-{sequence}",
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| callback_permission_error())?;
        file.write_all(content)
            .map_err(|_| callback_permission_error())?;
        file.sync_all().map_err(|_| callback_permission_error())?;
        if let Some(permissions) = permissions {
            fs::set_permissions(&temporary, permissions)
                .map_err(|_| callback_permission_error())?;
        }
        fs::rename(&temporary, path).map_err(|_| callback_permission_error())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) async fn reject_callback(
    resources: &CursorRouterResources,
    rpc_id: Value,
    message: &str,
) -> ChatResult<()> {
    resources
        .client
        .respond_error(rpc_id, -32602, message)
        .await
        .map_err(|error| error.to_chat_error("ACP callback rejection"))
}

pub(super) fn callback_permission_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "ACP callback cannot access this workspace path",
        true,
    )
}
