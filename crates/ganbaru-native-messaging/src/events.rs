//! Browser block events the host spools for the app to record in the vault.

use chrono::{DateTime, SecondsFormat, Utc};

use crate::config::normalize_host_rule;
use crate::now_utc;
use crate::rules::HostDecision;
use crate::snapshot::{StateSnapshot, runtime_status_at};
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
    let (phase, run_id) = block_event_context(snapshot, now_utc());
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

/// The phase and run a block event records at `checked_at`. Only runtime authority that is current
/// for enforcement contributes, so a stale or expired file never stamps an old run on the event.
pub(super) fn block_event_context(
    snapshot: &StateSnapshot,
    checked_at: DateTime<Utc>,
) -> (Option<String>, Option<String>) {
    let status = runtime_status_at(snapshot, checked_at);
    let phase = status.authority.block_event_phase().map(str::to_string);
    let run_id = status.active_phase().map(|active| active.run_id.clone());
    (phase, run_id)
}
