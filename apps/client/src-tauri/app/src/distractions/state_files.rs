use super::*;
use std::io::{Read, Write};

static STATE_WRITE_GENERATION: AtomicU64 = AtomicU64::new(0);
const MAX_DEVICE_STATE_BYTES: u64 = 1024 * 1024;

fn read_bounded_state(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_DEVICE_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DEVICE_STATE_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub(super) fn state_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| format!("create app config dir: {e}"))?;
    path.push(STATE_FILE);
    Ok(path)
}

pub(super) fn extension_connection_path<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| format!("create app config dir: {e}"))?;
    path.push(EXTENSION_CONNECTION_FILE);
    Ok(path)
}

pub(super) fn limit_state_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    let mut path = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&path).map_err(|e| format!("create app config dir: {e}"))?;
    path.push(LIMIT_STATE_FILE);
    Ok(path)
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

pub(super) fn clear_enforcement_state_files(
    state_path: &Path,
    limit_state_path: &Path,
) -> Result<(), String> {
    remove_file_if_exists(state_path)?;
    remove_file_if_exists(limit_state_path)
}

pub fn clear_distractions_enforcement_state<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<(), String> {
    runtime::stop_and_clear(app)
}

pub(super) fn validate_state(state: &DistractionsRuntimeState) -> Result<(), String> {
    match state.phase.as_str() {
        "inactive" | "focus" | "short_break" | "long_break" => {}
        other => return Err(format!("unsupported distraction phase '{other}'")),
    }
    if state
        .remaining_seconds
        .is_some_and(|remaining_seconds| remaining_seconds < 0)
    {
        return Err("remaining_seconds must be non-negative".to_string());
    }
    if state.updated_at.trim().is_empty() {
        return Err("updated_at is required".to_string());
    }
    if state.valid_until_ms.is_some_and(|deadline| deadline < 0) {
        return Err("valid_until_ms must be non-negative".into());
    }
    match state.pause_reason.as_deref() {
        None | Some("manual" | "idle" | "suspend") => {}
        Some(other) => return Err(format!("unsupported distraction pause reason '{other}'")),
    }
    Ok(())
}

pub(super) fn read_fresh_runtime_state(
    path: &Path,
    checked_at: DateTime<Utc>,
) -> Option<DistractionsRuntimeState> {
    let contents = read_bounded_state(path)?;
    let state = serde_json::from_str::<DistractionsRuntimeState>(&contents).ok()?;
    if validate_state(&state).is_err() {
        return None;
    }
    let updated_at = DateTime::parse_from_rfc3339(&state.updated_at)
        .ok()?
        .with_timezone(&Utc);
    let age = checked_at - updated_at;
    if age < chrono::Duration::zero() || age > chrono::Duration::seconds(ACTIVE_STATE_STALE_SECONDS)
    {
        return None;
    }
    if state
        .valid_until_ms
        .is_some_and(|deadline| checked_at.timestamp_millis() >= deadline)
        || (state.active
            && !state.paused
            && state.remaining_seconds.is_some_and(|remaining| {
                checked_at.timestamp_millis()
                    >= updated_at
                        .timestamp_millis()
                        .saturating_add(remaining.saturating_mul(1000))
            }))
    {
        return None;
    }
    Some(state)
}

/// Build phase-dependent rules only from accepted native execution. Waiting,
/// expired, and failed phases cannot authorize a successor phase's rules.
pub(crate) fn committed_focus_state(
    effect: &ganbaru_pomodoro::CommittedFocusEffect,
    now_ms: i64,
) -> Result<DistractionsRuntimeState, String> {
    use ganbaru_pomodoro::{FocusMode, FocusPhase};
    let paused = matches!(
        effect.mode,
        FocusMode::ManualPause | FocusMode::IdlePause | FocusMode::Suspended
    );
    let valid_until = effect.effective_valid_until_ms();
    let active = matches!(
        effect.mode,
        FocusMode::Running | FocusMode::ManualPause | FocusMode::IdlePause | FocusMode::Suspended
    ) && effect.run_id.is_some()
        && effect.segment_id.is_some()
        && effect.phase.is_some()
        && valid_until > now_ms;
    let phase = if active {
        match effect.phase {
            Some(FocusPhase::Focus) => "focus",
            Some(FocusPhase::ShortBreak) => "short_break",
            Some(FocusPhase::LongBreak) => "long_break",
            None => "inactive",
        }
    } else {
        "inactive"
    };
    let updated_at = DateTime::<Utc>::from_timestamp_millis(now_ms)
        .ok_or("Native Focus clock is outside the supported range")?
        .to_rfc3339_opts(SecondsFormat::Millis, true);
    Ok(DistractionsRuntimeState {
        active,
        paused: active && paused,
        pause_reason: if active {
            match effect.mode {
                FocusMode::ManualPause => Some("manual".into()),
                FocusMode::IdlePause => Some("idle".into()),
                FocusMode::Suspended => Some("suspend".into()),
                _ => None,
            }
        } else {
            None
        },
        phase: phase.into(),
        active_run_id: effect.run_id.clone(),
        active_occurrence_id: effect.occurrence_id.clone(),
        remaining_seconds: active.then(|| effect.remaining_ms.max(0).saturating_add(999) / 1000),
        updated_at,
        valid_until_ms: Some(valid_until.max(0)),
    })
}

/// Publish a committed phase under the same native revocation fence as budgets.
/// The caller runs this bounded filesystem work on a blocking worker.
pub(crate) fn publish_committed_focus(
    app: &tauri::AppHandle,
    effect: &ganbaru_pomodoro::CommittedFocusEffect,
    publication_generation: Option<u64>,
    now_ms: i64,
) -> Result<(), String> {
    let state = committed_focus_state(effect, now_ms)?;
    let path = state_path(app)?;
    let json = serde_json::to_string(&state)
        .map_err(|error| format!("Encode native Focus rules: {error}"))?;
    runtime::publish(app, publication_generation, || {
        if !crate::pomodoro::native_runtime::effect_is_current(app, effect, now_ms)? {
            return Err("Native Focus rule publication was superseded or expired".into());
        }
        write_text_file_atomically(&path, &json)
    })
}

pub(super) fn validate_limit_state(state: &DistractionsLimitState) -> Result<(), String> {
    if !is_valid_local_date(&state.local_date) {
        return Err("local_date must use yyyy-mm-dd".to_string());
    }
    if !is_valid_local_date(&state.week_start_local_date) {
        return Err("week_start_local_date must use yyyy-mm-dd".to_string());
    }
    if state.week_start_local_date != limits::week_start(&state.local_date)?
        || state.limits.len() > limits::MAX_LIMITS * 2
    {
        return Err("limit week start or item count is invalid".into());
    }
    DateTime::parse_from_rfc3339(&state.updated_at)
        .map_err(|e| format!("parse updated_at: {e}"))?;
    if let Some(database_path) = &state.database_path {
        let path = Path::new(database_path);
        if !path.is_absolute() {
            return Err("database_path must be absolute".to_string());
        }
        if path.file_name().and_then(|name| name.to_str()) != Some(vault::APP_SQLITE_FILE) {
            return Err("database_path must point to ganbaru-ai.sqlite".to_string());
        }
    }
    let mut identities = HashSet::new();
    for limit in &state.limits {
        if limit.id.trim().is_empty() {
            return Err("limit id is required".to_string());
        }
        if !matches!(limit.period.as_str(), "day" | "week") {
            return Err("limit period must be day or week".to_string());
        }
        let expected_start = if limit.period == "day" {
            &state.local_date
        } else {
            &state.week_start_local_date
        };
        if &limit.window_start_local_date != expected_start
            || limit.window_end_local_date != state.local_date
            || !identities.insert((&limit.id, &limit.period))
        {
            return Err("limit window or identity is inconsistent".into());
        }
        if !is_valid_local_date(&limit.window_start_local_date)
            || !is_valid_local_date(&limit.window_end_local_date)
        {
            return Err("limit window dates must use yyyy-mm-dd".to_string());
        }
        if limit.window_start_local_date.as_str() > limit.window_end_local_date.as_str() {
            return Err("limit window start must not be after window end".to_string());
        }
        if limit.used_seconds < 0 || limit.limit_seconds <= 0 || limit.remaining_seconds < 0 {
            return Err("limit seconds must be non-negative".to_string());
        }
        if limit.remaining_seconds != (limit.limit_seconds - limit.used_seconds).max(0)
            || limit.exhausted != (limit.used_seconds >= limit.limit_seconds)
        {
            return Err("limit exhaustion is inconsistent with its totals".into());
        }
    }
    Ok(())
}

pub(super) fn read_fresh_limit_state(
    path: &Path,
    checked_at: DateTime<Utc>,
    expected_database_path: &Path,
) -> Option<DistractionsLimitState> {
    let contents = read_bounded_state(path)?;
    let state = serde_json::from_str::<DistractionsLimitState>(&contents).ok()?;
    if validate_limit_state(&state).is_err()
        || state.database_path.as_deref().map(Path::new) != Some(expected_database_path)
    {
        return None;
    }
    let updated_at = DateTime::parse_from_rfc3339(&state.updated_at)
        .ok()?
        .with_timezone(&Utc);
    let age = checked_at - updated_at;
    (age >= chrono::Duration::zero() && age <= chrono::Duration::seconds(LIMIT_STATE_STALE_SECONDS))
        .then_some(state)
}

pub(super) fn disconnected_extension_status(
    checked_at: DateTime<Utc>,
    reason: impl Into<String>,
) -> DistractionsExtensionStatus {
    DistractionsExtensionStatus {
        connected: false,
        last_seen_at: None,
        last_message_type: None,
        checked_at: checked_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        stale_seconds: EXTENSION_CONNECTION_STALE_SECONDS,
        reason: Some(reason.into()),
    }
}

pub(super) fn extension_status_from_file_contents(
    contents: &str,
    checked_at: DateTime<Utc>,
    fresh_after: Option<DateTime<Utc>>,
) -> DistractionsExtensionStatus {
    let Ok(record) = serde_json::from_str::<DistractionsExtensionConnectionFile>(contents) else {
        return disconnected_extension_status(checked_at, "connection status file is invalid");
    };
    let Ok(last_seen_at) = DateTime::parse_from_rfc3339(&record.last_seen_at) else {
        return disconnected_extension_status(checked_at, "connection timestamp is invalid");
    };

    let age_seconds = (checked_at - last_seen_at.with_timezone(&Utc))
        .num_seconds()
        .max(0);
    let before_current_app_session = fresh_after
        .is_some_and(|minimum_seen_at| last_seen_at.with_timezone(&Utc) < minimum_seen_at);
    DistractionsExtensionStatus {
        connected: age_seconds <= EXTENSION_CONNECTION_STALE_SECONDS && !before_current_app_session,
        last_seen_at: Some(record.last_seen_at),
        last_message_type: record.last_message_type,
        checked_at: checked_at.to_rfc3339_opts(SecondsFormat::Millis, true),
        stale_seconds: EXTENSION_CONNECTION_STALE_SECONDS,
        reason: if before_current_app_session {
            Some("connection is from an older app session".to_string())
        } else if age_seconds > EXTENSION_CONNECTION_STALE_SECONDS {
            Some("connection status is stale".to_string())
        } else {
            None
        },
    }
}

pub(super) fn write_text_file_atomically(
    path: &std::path::Path,
    contents: &str,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "state path has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "state path has no file name".to_string())?
        .to_string_lossy();
    let generation = STATE_WRITE_GENERATION.fetch_add(1, Ordering::Relaxed);
    let temporary_path = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        generation
    ));
    let write_result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(|e| e.to_string())?;
        file.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary_path, path).map_err(|e| e.to_string())
    })();
    if write_result.is_err() {
        let _ = remove_file_if_exists(&temporary_path);
    }
    write_result
}

#[tauri::command]
pub fn distractions_extension_status<R: Runtime>(
    app: tauri::AppHandle<R>,
    fresh_after: Option<String>,
) -> Result<DistractionsExtensionStatus, String> {
    let checked_at = now_utc();
    let fresh_after = fresh_after
        .as_deref()
        .map(DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|e| format!("parse fresh_after: {e}"))?
        .map(|date| date.with_timezone(&Utc));
    let path = extension_connection_path(&app)?;
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(extension_status_from_file_contents(
            &contents,
            checked_at,
            fresh_after,
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(
            disconnected_extension_status(checked_at, "no extension connection has been recorded"),
        ),
        Err(err) => Err(format!("read extension connection status: {err}")),
    }
}
