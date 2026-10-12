use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

mod config;
mod events;
mod linked_usage;
mod rules;
mod snapshot;

use config::{is_protected_app_name, normalize_app_name, normalize_host_rule};
use events::log_block_event;
use linked_usage::{UsageSample, record_usage_sample};
use rules::{decide_url_with_limits, feed_fingerprint, host_from_url, rules_fingerprint};
use snapshot::{
    StateSnapshot, config_dir_candidates, is_valid_local_date, load_snapshot, runtime_status,
    should_enforce,
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("native messaging async runtime should initialize")
        .block_on(future)
}

// Chromium native messaging host names allow underscores but not hyphens.
const HOST_NAME: &str = "org.opengrimoire.ganbaru_ai.distractions";
const DEV_HOST_NAME: &str = "org.opengrimoire.ganbaru_ai.distractions_dev";
const EXTENSION_CONNECTION_FILE: &str = "distractions-extension-status.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeRequest {
    #[serde(rename = "type")]
    message_type: String,
    url: Option<String>,
    host: Option<String>,
    log_event: Option<bool>,
    source_type: Option<String>,
    source_key: Option<String>,
    display_name: Option<String>,
    elapsed_seconds: Option<i64>,
    started_at_ms: Option<i64>,
    local_date: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeResponse {
    #[serde(rename = "type")]
    message_type: &'static str,
    host_name: String,
    connected: bool,
    active: bool,
    paused: bool,
    pause_reason: Option<String>,
    phase: String,
    remaining_seconds: Option<i64>,
    rules_fingerprint: String,
    blocked: bool,
    host: Option<String>,
    matched_rule_name: Option<String>,
    reason: Option<String>,
    environment_name: &'static str,
}

fn main() {
    let response = match run() {
        Ok(response) => response,
        Err(reason) => NativeResponse {
            message_type: "error",
            host_name: native_host_name(),
            connected: false,
            active: false,
            paused: false,
            pause_reason: None,
            phase: "inactive".to_string(),
            remaining_seconds: None,
            rules_fingerprint: "unavailable".to_string(),
            blocked: false,
            host: None,
            matched_rule_name: None,
            reason: Some(reason),
            environment_name: "Ganbaru AI",
        },
    };

    if let Err(err) = write_native_message(&response) {
        eprintln!("failed to write native messaging response: {err}");
    }
}

fn native_host_name() -> String {
    match std::env::var("GANBARU_AI_NATIVE_HOST_NAME") {
        Ok(value) if value == DEV_HOST_NAME => value,
        _ => HOST_NAME.to_string(),
    }
}

fn run() -> Result<NativeResponse, String> {
    let request = read_native_message()?;
    let snapshot = load_snapshot();
    if let Err(err) =
        record_extension_connection(snapshot.config_dir.as_deref(), &request.message_type)
    {
        eprintln!("failed to record extension connection: {err}");
    }
    let mut response = response_from_snapshot(&snapshot);

    if request.message_type == "decide_url" {
        if let Some(host) = normalized_request_host(&request) {
            response.host = Some(host.clone());
            let regular_rules_active = should_enforce(&snapshot, &response);
            let decision = decide_url_with_limits(
                &host,
                request.url.as_deref(),
                &snapshot.config,
                snapshot.limit_state.as_ref(),
                regular_rules_active,
            );
            response.blocked = decision.is_blocked();
            response.matched_rule_name = decision.matched_rule_name();
            if response.blocked && request.log_event.unwrap_or(true) {
                log_block_event(&snapshot, &host, &decision);
            }
        } else {
            response.reason = Some("unsupported or invalid URL".to_string());
        }
    } else if request.message_type == "record_usage" {
        match normalize_usage_sample(&request) {
            Ok(sample) => {
                match record_usage_sample(
                    snapshot.config_dir.as_deref(),
                    snapshot.vault_path.as_deref(),
                    sample,
                ) {
                    Ok(()) => {}
                    Err(reason) => response.reason = Some(reason),
                }
            }
            Err(reason) => response.reason = Some(reason),
        }
    } else if request.message_type != "get_state" {
        response.reason = Some(format!(
            "unsupported message type '{}'",
            request.message_type
        ));
    }

    Ok(response)
}

fn write_text_file_atomically(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "connection status path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "connection status path has no file name".to_string())?
        .to_string_lossy();
    let temporary_path = parent.join(format!("{file_name}.tmp"));
    {
        let mut file = std::fs::File::create(&temporary_path).map_err(|e| e.to_string())?;
        file.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temporary_path, path).map_err(|e| e.to_string())
}

fn extension_connection_dir(config_dir: Option<&Path>) -> Option<PathBuf> {
    config_dir
        .map(Path::to_path_buf)
        .or_else(|| config_dir_candidates().into_iter().next())
}

fn record_extension_connection(
    config_dir: Option<&Path>,
    message_type: &str,
) -> Result<(), String> {
    let status_dir = extension_connection_dir(config_dir)
        .ok_or_else(|| "app config directory is unavailable".to_string())?;
    let payload = serde_json::json!({
        "lastSeenAt": now_utc().to_rfc3339_opts(SecondsFormat::Millis, true),
        "lastMessageType": message_type,
    });
    let json = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
    write_text_file_atomically(&status_dir.join(EXTENSION_CONNECTION_FILE), &json)
}

fn read_native_message() -> Result<NativeRequest, String> {
    let mut stdin = std::io::stdin().lock();
    let mut length_bytes = [0_u8; 4];
    stdin
        .read_exact(&mut length_bytes)
        .map_err(|e| format!("read message length: {e}"))?;
    let length = u32::from_ne_bytes(length_bytes) as usize;
    if length > 1024 * 1024 {
        return Err("native message exceeds 1 MiB".to_string());
    }
    let mut buffer = vec![0_u8; length];
    stdin
        .read_exact(&mut buffer)
        .map_err(|e| format!("read message body: {e}"))?;
    serde_json::from_slice(&buffer).map_err(|e| format!("parse native message: {e}"))
}

fn write_native_message(response: &NativeResponse) -> Result<(), String> {
    let bytes = serde_json::to_vec(response).map_err(|e| e.to_string())?;
    let length = u32::try_from(bytes.len())
        .map_err(|_| "native response exceeds u32 length".to_string())?
        .to_ne_bytes();
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&length).map_err(|e| e.to_string())?;
    stdout.write_all(&bytes).map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())
}

fn response_from_snapshot(snapshot: &StateSnapshot) -> NativeResponse {
    let (active, phase, remaining_seconds, reason) = runtime_status(snapshot);
    let paused = snapshot
        .runtime
        .as_ref()
        .is_some_and(|runtime| active && runtime.paused);
    let pause_reason = snapshot.runtime.as_ref().and_then(|runtime| {
        (active && runtime.paused)
            .then(|| runtime.pause_reason.clone())
            .flatten()
    });
    NativeResponse {
        message_type: "decision",
        host_name: native_host_name(),
        connected: snapshot.config_dir.is_some(),
        active,
        paused,
        pause_reason,
        phase,
        remaining_seconds,
        rules_fingerprint: rules_fingerprint(&snapshot.config, snapshot.limit_state.as_ref()),
        blocked: false,
        host: None,
        matched_rule_name: None,
        reason,
        environment_name: "Ganbaru AI",
    }
}

fn normalized_request_host(request: &NativeRequest) -> Option<String> {
    request
        .host
        .as_deref()
        .and_then(normalize_host_rule)
        .or_else(|| request.url.as_deref().and_then(host_from_url))
}

fn normalize_usage_sample(request: &NativeRequest) -> Result<UsageSample, String> {
    let source_type = request
        .source_type
        .as_deref()
        .ok_or_else(|| "usage sourceType is required".to_string())?;
    if !matches!(source_type, "website" | "desktop-app" | "mobile-app") {
        return Err(format!("unsupported usage source type '{source_type}'"));
    }
    let raw_source_key = request
        .source_key
        .as_deref()
        .or(request.host.as_deref())
        .ok_or_else(|| "usage sourceKey is required".to_string())?;
    let source_key = match source_type {
        "website" => normalize_host_rule(raw_source_key)
            .ok_or_else(|| "usage website host is invalid".to_string())?,
        "desktop-app" | "mobile-app" => normalize_app_name(raw_source_key)
            .ok_or_else(|| "usage app name is invalid".to_string())?
            .to_lowercase(),
        _ => unreachable!(),
    };
    if source_type == "desktop-app" && is_protected_app_name(&source_key) {
        return Err("protected desktop apps cannot be tracked".to_string());
    }
    let elapsed_seconds = request
        .elapsed_seconds
        .ok_or_else(|| "usage elapsedSeconds is required".to_string())?;
    if elapsed_seconds <= 0 || elapsed_seconds > 86_400 {
        return Err("usage elapsedSeconds must be between 1 and 86400".to_string());
    }
    let started_at_ms = request
        .started_at_ms
        .ok_or_else(|| "usage startedAt is required".to_string())?;
    if started_at_ms < 0 {
        return Err("usage startedAt must be non-negative".to_string());
    }
    let local_date = request
        .local_date
        .clone()
        .ok_or_else(|| "usage localDate is required".to_string())?;
    if !is_valid_local_date(&local_date) {
        return Err("usage localDate must use yyyy-mm-dd".to_string());
    }
    let display_name = request.display_name.as_ref().and_then(|value| {
        let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
        (!normalized.is_empty()).then(|| normalized.chars().take(120).collect::<String>())
    });
    let mut identity_hash = 14_695_981_039_346_656_037_u64;
    for value in [
        source_type,
        source_key.as_str(),
        &started_at_ms.to_string(),
        &elapsed_seconds.to_string(),
        local_date.as_str(),
    ] {
        feed_fingerprint(&mut identity_hash, value);
    }
    Ok(UsageSample {
        id: format!("ext-{identity_hash:016x}"),
        source_type: source_type.to_string(),
        source_key,
        display_name,
        started_at_ms,
        elapsed_seconds,
        local_date,
        created_at_ms: now_epoch_ms(),
    })
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

fn now_utc() -> DateTime<Utc> {
    std::time::SystemTime::now().into()
}

#[cfg(test)]
mod tests;
