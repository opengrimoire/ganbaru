//! Browser block events the host spools for the app to record in the vault.

use chrono::SecondsFormat;

use crate::config::normalize_host_rule;
use crate::now_utc;
use crate::rules::HostDecision;
use crate::snapshot::{RuntimeState, StateSnapshot};
use crate::spool::{BlockEvent, record_block_event};

/// Spools a block for the app. Browsing history is kept nowhere else, and only the redacted host
/// is recorded, never the full URL.
pub(super) fn log_block_event(snapshot: &StateSnapshot, host: &str, decision: &HostDecision) {
    let occurred_at = now_utc().to_rfc3339_opts(SecondsFormat::Millis, true);
    if let Err(err) = spool_block_event(snapshot, &occurred_at, host, decision) {
        eprintln!("failed to record distraction block event: {err}");
    }
}

pub(super) fn spool_block_event(
    snapshot: &StateSnapshot,
    occurred_at: &str,
    host: &str,
    decision: &HostDecision,
) -> Result<(), String> {
    let source_key = normalize_host_rule(host)
        .filter(|host| !host.contains('/'))
        .ok_or_else(|| "block event host is invalid".to_string())?;
    let phase = block_event_phase(snapshot.runtime.as_ref());
    let run_id = phase
        .as_deref()
        .and(snapshot.runtime.as_ref())
        .and_then(|runtime| runtime.active_run_id.clone());
    let event_nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    record_block_event(
        snapshot.config_dir.as_deref(),
        snapshot.vault_path.as_deref(),
        BlockEvent {
            id: format!("block-{event_nonce}-{}", std::process::id()),
            run_id,
            occurred_at: occurred_at.to_string(),
            source_key,
            phase,
            decision: decision.event_decision(),
            rule_kind: decision.rule_kind(),
            rule_label: decision.matched_rule_name(),
            blocker_mode: decision.blocker_mode(&snapshot.config.mode),
        },
    )
}

pub(super) fn block_event_phase(runtime: Option<&RuntimeState>) -> Option<String> {
    let runtime = runtime?;
    if runtime.paused {
        return match runtime.pause_reason.as_deref() {
            Some("idle") => Some("idle_pause".to_string()),
            Some("suspend") => Some("suspend_pause".to_string()),
            Some("manual") | None => Some("manual_pause".to_string()),
            Some(_) => None,
        };
    }
    match runtime.phase.as_str() {
        "focus" | "short_break" | "long_break" => Some(runtime.phase.clone()),
        _ => None,
    }
}
