use super::*;

pub(crate) async fn upsert_adaptive_context_state_tx(
    tx: &mut Transaction<'_, Sqlite>,
    snapshot: &AdaptiveRunSnapshotForClose,
    ended_at: &str,
    summary: &AdaptiveRunOutcomeSummary,
    closure: &PomodoroRunClosure,
) -> Result<(), String> {
    let base =
        load_existing_adaptive_context_state_tx(tx, &snapshot.policy_id, &snapshot.context_key)
            .await?
            .unwrap_or_else(|| snapshot.state_scores.clone());
    let observed = observed_state_from_run_summary(summary, closure);
    let updated = blend_adaptive_state_scores(&base, &observed);

    sqlx::query(
        "INSERT INTO pomodoro_adaptive_context_states
            (policy_id, context_key, readiness, strain, recovery_debt,
             avoidance_pressure, momentum, confidence, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(policy_id, context_key) DO UPDATE SET
            readiness = excluded.readiness,
            strain = excluded.strain,
            recovery_debt = excluded.recovery_debt,
            avoidance_pressure = excluded.avoidance_pressure,
            momentum = excluded.momentum,
            confidence = excluded.confidence,
            updated_at = excluded.updated_at",
    )
    .bind(&snapshot.policy_id)
    .bind(&snapshot.context_key)
    .bind(updated.readiness)
    .bind(updated.strain)
    .bind(updated.recovery_debt)
    .bind(updated.avoidance_pressure)
    .bind(updated.momentum)
    .bind(updated.confidence)
    .bind(ended_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("upsert adaptive context state: {e}"))?;

    sqlx::query(
        "INSERT INTO pomodoro_adaptive_context_state_history
            (id, policy_id, context_key, observed_at, readiness, strain,
             recovery_debt, avoidance_pressure, momentum, confidence)
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snapshot.policy_id)
    .bind(&snapshot.context_key)
    .bind(ended_at)
    .bind(updated.readiness)
    .bind(updated.strain)
    .bind(updated.recovery_debt)
    .bind(updated.avoidance_pressure)
    .bind(updated.momentum)
    .bind(updated.confidence)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert adaptive context state history: {e}"))?;

    Ok(())
}

async fn load_existing_adaptive_context_state_tx(
    tx: &mut Transaction<'_, Sqlite>,
    policy_id: &str,
    context_key: &str,
) -> Result<Option<PomodoroAdaptiveStateScoresWrite>, String> {
    let row = sqlx::query(
        "SELECT readiness, strain, recovery_debt, avoidance_pressure, momentum, confidence
         FROM pomodoro_adaptive_context_states
         WHERE policy_id = ? AND context_key = ?",
    )
    .bind(policy_id)
    .bind(context_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load existing adaptive context state: {e}"))?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(PomodoroAdaptiveStateScoresWrite {
        readiness: row
            .try_get("readiness")
            .map_err(|e| format!("read existing adaptive readiness: {e}"))?,
        strain: row
            .try_get("strain")
            .map_err(|e| format!("read existing adaptive strain: {e}"))?,
        recovery_debt: row
            .try_get("recovery_debt")
            .map_err(|e| format!("read existing adaptive recovery_debt: {e}"))?,
        avoidance_pressure: row
            .try_get("avoidance_pressure")
            .map_err(|e| format!("read existing adaptive avoidance_pressure: {e}"))?,
        momentum: row
            .try_get("momentum")
            .map_err(|e| format!("read existing adaptive momentum: {e}"))?,
        confidence: row
            .try_get("confidence")
            .map_err(|e| format!("read existing adaptive confidence: {e}"))?,
    }))
}

fn observed_state_from_run_summary(
    summary: &AdaptiveRunOutcomeSummary,
    closure: &PomodoroRunClosure,
) -> PomodoroAdaptiveStateScoresWrite {
    let focus_attempts =
        (summary.completed_focus_segments + summary.interrupted_focus_segments).max(1) as f64;
    let break_attempts = (summary.break_started_count + summary.break_skipped_count).max(1) as f64;
    let clean_completion_rate = summary.completed_focus_segments as f64 / focus_attempts;
    let failure_rate = summary.focus_failure_count as f64 / focus_attempts;
    let idle_pressure = normalized_score(
        summary.idle_pause_count as f64 + summary.idle_pause_seconds as f64 / 600.0,
        4.0,
    );
    let manual_pause_pressure = normalized_score(
        summary.manual_pause_count as f64 + summary.manual_pause_seconds as f64 / 900.0,
        4.0,
    );
    let blocked_pressure = normalized_score(summary.blocked_attempt_count as f64, 8.0);
    let skipped_break_pressure = normalized_score(summary.break_skipped_count as f64, 3.0);
    let break_overtime_pressure = normalized_score(
        summary.short_break_overtime_seconds as f64 / 180.0
            + summary.long_break_overtime_seconds as f64 / 300.0,
        4.0,
    );
    let stop_pressure = if closure.end_reason == "stopped" {
        1.0
    } else {
        0.0
    };
    let recovery_debt = clamp_score(
        skipped_break_pressure * 0.35
            + break_overtime_pressure * 0.32
            + failure_rate * 0.20
            + stop_pressure * 0.13,
    );
    let strain = clamp_score(
        failure_rate * 0.32
            + idle_pressure * 0.25
            + manual_pause_pressure * 0.12
            + blocked_pressure * 0.18
            + stop_pressure * 0.13,
    );
    let avoidance_pressure = clamp_score(blocked_pressure * 0.80 + stop_pressure * 0.20);
    let momentum = clamp_score(
        clean_completion_rate * 0.55
            + normalized_score(summary.clean_focus_seconds as f64 / 60.0, 120.0) * 0.25
            + (1.0 - blocked_pressure) * 0.10
            - recovery_debt * 0.25,
    );
    let readiness = clamp_score(
        momentum * 0.55 + clean_completion_rate * 0.25 - strain * 0.25 - recovery_debt * 0.20,
    );
    let confidence = clamp_score(
        normalized_score(focus_attempts + break_attempts, 8.0) * 0.55
            + normalized_score(summary.clean_focus_seconds as f64 / 60.0, 180.0) * 0.25
            + 0.20,
    );

    PomodoroAdaptiveStateScoresWrite {
        readiness,
        strain,
        recovery_debt,
        avoidance_pressure,
        momentum,
        confidence,
    }
}

fn blend_adaptive_state_scores(
    previous: &PomodoroAdaptiveStateScoresWrite,
    observed: &PomodoroAdaptiveStateScoresWrite,
) -> PomodoroAdaptiveStateScoresWrite {
    PomodoroAdaptiveStateScoresWrite {
        readiness: blend_score(previous.readiness, observed.readiness),
        strain: blend_score(previous.strain, observed.strain),
        recovery_debt: blend_score(previous.recovery_debt, observed.recovery_debt),
        avoidance_pressure: blend_score(previous.avoidance_pressure, observed.avoidance_pressure),
        momentum: blend_score(previous.momentum, observed.momentum),
        confidence: blend_score(previous.confidence, observed.confidence).max(observed.confidence),
    }
}

fn blend_score(previous: f64, observed: f64) -> f64 {
    clamp_score(previous * 0.65 + observed * 0.35)
}

fn normalized_score(value: f64, scale: f64) -> f64 {
    if scale <= 0.0 {
        0.0
    } else {
        clamp_score(value / scale)
    }
}

fn clamp_score(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}
