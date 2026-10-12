//! Host-only block event logging and existing-vault database writes.

use chrono::SecondsFormat;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::io::Write;
use std::str::FromStr;

use crate::config::normalize_host_rule;
use crate::linked_usage::native_vault_is_writable;
use crate::rules::HostDecision;
use crate::snapshot::{RuntimeState, StateSnapshot, usage_db_path};
use crate::{block_on, now_utc};

const EVENTS_FILE: &str = "distractions-events.jsonl";

pub(super) fn log_block_event(snapshot: &StateSnapshot, host: &str, decision: &HostDecision) {
    let matched_rule_name = decision.matched_rule_name();
    let occurred_at = now_utc().to_rfc3339_opts(SecondsFormat::Millis, true);
    let Some(config_dir) = &snapshot.config_dir else {
        return;
    };
    let event = serde_json::json!({
        "occurredAt": occurred_at,
        "urlHost": host,
        "phase": snapshot.runtime.as_ref().map(|state| state.phase.as_str()).unwrap_or("inactive"),
        "ruleNameSnapshot": matched_rule_name,
        "decision": "blocked"
    });
    if let Ok(line) = serde_json::to_string(&event) {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(config_dir.join(EVENTS_FILE))
        {
            let _ = writeln!(file, "{line}");
        }
    }
    if native_vault_is_writable(config_dir, snapshot.vault_path.as_deref()) {
        if let Err(err) = record_block_event_in_database(snapshot, &occurred_at, host, decision) {
            eprintln!("failed to record distraction block event: {err}");
        }
    }
}

pub(super) fn record_block_event_in_database(
    snapshot: &StateSnapshot,
    occurred_at: &str,
    host: &str,
    decision: &HostDecision,
) -> Result<(), String> {
    let vault_path = snapshot
        .vault_path
        .as_deref()
        .ok_or_else(|| "active Ganbaru AI folder is unavailable".to_string())?;
    let db_path = usage_db_path(vault_path, snapshot.limit_state.as_ref());
    if !db_path.exists() {
        return Err("usage database is unavailable".to_string());
    }
    let source_key =
        normalize_host_rule(host).ok_or_else(|| "block event host is invalid".to_string())?;
    let phase = block_event_phase(snapshot.runtime.as_ref());
    let run_id = phase
        .as_deref()
        .and(snapshot.runtime.as_ref())
        .and_then(|runtime| runtime.active_run_id.clone());
    let matched_rule_name = decision.matched_rule_name();
    let rule_kind = decision.rule_kind();
    let blocker_mode = decision.blocker_mode(&snapshot.config.mode);
    let event_decision = decision.event_decision();
    let event_nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let event_id = format!("block-{event_nonce}-{}", std::process::id());

    block_on(async move {
        let db_url = format!(
            "sqlite:{}",
            db_path.to_str().ok_or_else(|| {
                "usage database path contains non-utf8 characters".to_string()
            })?
        );
        let options = SqliteConnectOptions::from_str(&db_url)
            .map_err(|e| format!("parse usage database url: {e}"))?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|e| format!("connect usage database: {e}"))?;
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&pool)
            .await
            .map_err(|e| format!("usage database busy timeout: {e}"))?;
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_block_events
                (id, run_id, segment_id, occurred_at, source_type, source_key,
                 display_name, phase, decision, rule_id, category_id)
             VALUES (?, ?, NULL, ?, 'browser', ?, ?, ?, ?, NULL, NULL)",
        )
        .bind(&event_id)
        .bind(&run_id)
        .bind(occurred_at)
        .bind(&source_key)
        .bind(&source_key)
        .bind(&phase)
        .bind(event_decision)
        .execute(&pool)
        .await
        .map_err(|e| format!("record block event: {e}"))?;
        sqlx::query(
            "INSERT OR REPLACE INTO distractions_block_event_rule_snapshots
                    (block_event_id, rule_id, rule_kind, rule_label, environment_id, blocker_mode)
                 VALUES (?, NULL, ?, ?, NULL, ?)",
        )
        .bind(&event_id)
        .bind(rule_kind)
        .bind(matched_rule_name)
        .bind(blocker_mode)
        .execute(&pool)
        .await
        .map_err(|e| format!("record block event rule snapshot: {e}"))?;
        pool.close().await;
        Ok(())
    })
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
