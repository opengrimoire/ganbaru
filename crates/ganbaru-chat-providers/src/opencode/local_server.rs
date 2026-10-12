//! Owned loopback OpenCode server lifecycle.

use super::cli::{process_environment, resolve_executable};
use super::config::{OPENCODE_PASSWORD_ENVIRONMENT, OpenCodeSecret};
use crate::models::{ChatError, ChatErrorCode, ChatResult, ProviderInstanceConfig};
use crate::process::{ProviderProcessConfig, ProviderProcessHandle, spawn_provider_process};
use reqwest::Url;
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

const READY_PREFIX: &str = "opencode server listening";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const GRACEFUL_STOP: Duration = Duration::from_millis(750);
const FORCE_STOP: Duration = Duration::from_secs(2);
const STDERR_LIMIT_BYTES: usize = 256 * 1024;
const STDOUT_LIMIT_BYTES: usize = 256 * 1024;
const LINE_LIMIT_BYTES: usize = 16 * 1024;

pub struct OwnedOpenCodeServer {
    pub origin: String,
    process: ProviderProcessHandle,
    stdout_task: JoinHandle<()>,
}

impl OwnedOpenCodeServer {
    pub async fn start(
        configuration: &ProviderInstanceConfig,
        workspace: &Path,
        password: Option<&OpenCodeSecret>,
    ) -> ChatResult<Self> {
        Self::start_with_timeout(configuration, workspace, password, STARTUP_TIMEOUT).await
    }

    pub(super) async fn start_with_timeout(
        configuration: &ProviderInstanceConfig,
        workspace: &Path,
        password: Option<&OpenCodeSecret>,
        startup_timeout: Duration,
    ) -> ChatResult<Self> {
        let mut environment = process_environment(configuration)?;
        if let Some(password) = password {
            environment.insert(
                OPENCODE_PASSWORD_ENVIRONMENT.to_string(),
                password.expose().to_string(),
            );
        }
        let executable = resolve_executable(&configuration.executable, &environment)?;
        let arguments = launch_arguments(&configuration.launch_arguments)?;
        let mut process = spawn_provider_process(ProviderProcessConfig {
            executable: executable.executable,
            arguments,
            working_directory: workspace.to_path_buf(),
            environment,
            stderr_limit_bytes: STDERR_LIMIT_BYTES,
        })?;
        let stdout = process.take_stdout()?;
        let (ready_sender, ready_receiver) = oneshot::channel();
        let stdout_task = tokio::spawn(read_stdout(stdout, ready_sender));
        let ready = tokio::time::timeout(startup_timeout, ready_receiver).await;
        let origin = match ready {
            Ok(Ok(Ok(origin))) => origin,
            Ok(Ok(Err(error))) => {
                let diagnostic = process.diagnostic().ok();
                let _ = process.stop(GRACEFUL_STOP, FORCE_STOP).await;
                stdout_task.abort();
                return Err(with_diagnostic(error, diagnostic.map(|value| value.text)));
            }
            Ok(Err(_)) => {
                let diagnostic = process.diagnostic().ok();
                let _ = process.stop(GRACEFUL_STOP, FORCE_STOP).await;
                stdout_task.abort();
                return Err(with_diagnostic(
                    startup_error("OpenCode server exited before readiness"),
                    diagnostic.map(|value| value.text),
                ));
            }
            Err(_) => {
                let diagnostic = process.diagnostic().ok();
                let _ = process.stop(GRACEFUL_STOP, FORCE_STOP).await;
                stdout_task.abort();
                return Err(with_diagnostic(
                    ChatError::new(
                        ChatErrorCode::Timeout,
                        "OpenCode server startup timed out",
                        true,
                    ),
                    diagnostic.map(|value| value.text),
                ));
            }
        };
        Ok(Self {
            origin,
            process,
            stdout_task,
        })
    }

    pub async fn stop(&mut self) -> ChatResult<()> {
        let result = self.process.stop(GRACEFUL_STOP, FORCE_STOP).await;
        self.stdout_task.abort();
        result
    }

    #[cfg(test)]
    pub fn process_id(&self) -> Option<u32> {
        self.process.process_id()
    }
}

impl Drop for OwnedOpenCodeServer {
    fn drop(&mut self) {
        self.stdout_task.abort();
    }
}

pub fn launch_arguments(configured: &[String]) -> ChatResult<Vec<String>> {
    if configured.len() > 128
        || configured.iter().any(|argument| {
            let normalized = argument.trim().to_ascii_lowercase();
            argument.is_empty()
                || argument.len() > 8 * 1024
                || argument.contains(['\0', '\n', '\r'])
                || matches!(
                    normalized.as_str(),
                    "serve" | "--hostname" | "--port" | "--password" | "-p"
                )
                || normalized.starts_with("--hostname=")
                || normalized.starts_with("--port=")
                || normalized.starts_with("--password=")
        })
    {
        return Err(ChatError::validation(
            "launchArguments",
            "OpenCode launch arguments cannot override server ownership or credentials",
        ));
    }
    let mut arguments = configured.to_vec();
    arguments.extend([
        "serve".to_string(),
        "--hostname=127.0.0.1".to_string(),
        "--port=0".to_string(),
    ]);
    Ok(arguments)
}

async fn read_stdout<R>(reader: R, ready: oneshot::Sender<ChatResult<String>>)
where
    R: AsyncRead + Unpin,
{
    let mut reader = reader;
    let mut ready = Some(ready);
    let mut stdout_bytes = 0_usize;
    let mut line = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(read) => read,
            Err(_) => {
                send_ready(
                    &mut ready,
                    Err(startup_error("OpenCode server output could not be read")),
                );
                return;
            }
        };
        if read == 0 {
            send_ready(
                &mut ready,
                Err(startup_error("OpenCode server exited before readiness")),
            );
            return;
        }
        if ready.is_none() {
            continue;
        }
        stdout_bytes = stdout_bytes.saturating_add(read);
        if stdout_bytes > STDOUT_LIMIT_BYTES {
            send_ready(
                &mut ready,
                Err(startup_error("OpenCode server output exceeded its bound")),
            );
            return;
        }
        for byte in &chunk[..read] {
            if *byte == b'\n' {
                let text = String::from_utf8_lossy(&line);
                if let Some(origin) = parse_readiness_line(&text) {
                    send_ready(&mut ready, origin);
                    line.clear();
                    break;
                }
                line.clear();
                continue;
            }
            if line.len() == LINE_LIMIT_BYTES {
                send_ready(
                    &mut ready,
                    Err(startup_error("OpenCode server output exceeded its bound")),
                );
                return;
            }
            line.push(*byte);
        }
    }
}

fn send_ready(sender: &mut Option<oneshot::Sender<ChatResult<String>>>, value: ChatResult<String>) {
    if let Some(sender) = sender.take() {
        let _ = sender.send(value);
    }
}

pub fn parse_readiness_line(line: &str) -> Option<ChatResult<String>> {
    let line = line.trim();
    if !line.starts_with(READY_PREFIX) {
        return None;
    }
    let (_, candidate) = line.rsplit_once(" on ")?;
    Some(validate_owned_origin(candidate))
}

fn validate_owned_origin(value: &str) -> ChatResult<String> {
    let url = Url::parse(value).map_err(|_| startup_error("OpenCode readiness URL is invalid"))?;
    if url.scheme() != "http"
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
        || url.port().is_none()
        || !url.host_str().is_some_and(|host| {
            host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
        })
    {
        return Err(startup_error(
            "OpenCode readiness URL is not the owned loopback server",
        ));
    }
    let host = match url.host_str() {
        Some("::1") => "[::1]",
        Some(host) if host.eq_ignore_ascii_case("localhost") => "localhost",
        Some(host) => host,
        None => return Err(startup_error("OpenCode readiness URL is invalid")),
    };
    Ok(format!("http://{host}:{}", url.port().unwrap_or_default()))
}

fn with_diagnostic(mut error: ChatError, diagnostic: Option<String>) -> ChatError {
    error.details = Some(Box::new(serde_json::json!({
        "diagnosticAvailable": diagnostic.is_some_and(|value| !value.trim().is_empty())
    })));
    error
}

fn startup_error(message: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::TransportUnavailable,
        message.to_string(),
        true,
    )
}
