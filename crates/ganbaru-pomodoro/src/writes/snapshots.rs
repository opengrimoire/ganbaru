use super::*;

pub(crate) fn run_rhythm_kind(rhythm: &PomodoroRunRhythm) -> &'static str {
    match rhythm {
        PomodoroRunRhythm::Count { .. } => "count",
        PomodoroRunRhythm::Sequence { .. } => "sequence",
    }
}

pub(crate) async fn insert_run_rhythm_snapshot_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run: &PomodoroRunWrite,
) -> Result<(), String> {
    match &run.rhythm {
        PomodoroRunRhythm::Count {
            focus_duration_minutes,
            short_break_minutes,
            long_break_minutes,
            long_break_after_focus_count,
        } => {
            sqlx::query(
                "INSERT INTO pomodoro_run_count_rhythms
                    (run_id, focus_duration_minutes, short_break_minutes, long_break_minutes,
                     long_break_after_focus_count)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&run.id)
            .bind(focus_duration_minutes)
            .bind(short_break_minutes)
            .bind(long_break_minutes)
            .bind(long_break_after_focus_count)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("insert pomodoro run count rhythm: {e}"))?;
        }
        PomodoroRunRhythm::Sequence { steps } => {
            for (step_index, step) in steps.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO pomodoro_run_sequence_steps
                        (run_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes)
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind(&run.id)
                .bind(step_index as i64)
                .bind(step.focus_duration_minutes)
                .bind(&step.break_phase)
                .bind(step.break_duration_minutes)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("insert pomodoro run sequence step: {e}"))?;
            }
        }
    }
    Ok(())
}

pub(crate) async fn insert_run_adaptive_snapshot_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run: &PomodoroRunWrite,
    snapshot: &PomodoroRunAdaptiveSnapshotWrite,
) -> Result<(), String> {
    upsert_adaptive_policy_tx(tx, snapshot).await?;
    insert_adaptive_context_snapshot_tx(tx, &snapshot.context_snapshot).await?;
    insert_adaptive_decision_tx(tx, &snapshot.decision).await?;
    for experiment in &snapshot.experiment_updates {
        upsert_adaptive_experiment_tx(tx, experiment).await?;
    }
    for assignment in &snapshot.experiment_assignments {
        insert_adaptive_experiment_assignment_tx(tx, assignment).await?;
    }
    sqlx::query(
        "INSERT INTO pomodoro_run_adaptive_snapshots
            (run_id, policy_id, policy_version, model_version, context_snapshot_id, decision_id)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&run.id)
    .bind(&snapshot.policy_id)
    .bind(snapshot.policy_version)
    .bind(snapshot.model_version)
    .bind(&snapshot.context_snapshot.id)
    .bind(&snapshot.decision.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro run adaptive snapshot: {e}"))?;
    Ok(())
}

pub(crate) async fn insert_adaptive_decision_envelope_tx(
    tx: &mut Transaction<'_, Sqlite>,
    envelope: &PomodoroAdaptiveDecisionEnvelopeWrite,
) -> Result<(), String> {
    upsert_adaptive_policy_from_parts_tx(
        tx,
        &envelope.policy_id,
        envelope.policy_version,
        envelope.model_version,
        &envelope.decision.occurred_at,
    )
    .await?;
    insert_adaptive_context_snapshot_tx(tx, &envelope.context_snapshot).await?;
    insert_adaptive_decision_tx(tx, &envelope.decision).await?;
    for experiment in &envelope.experiment_updates {
        upsert_adaptive_experiment_tx(tx, experiment).await?;
    }
    for assignment in &envelope.experiment_assignments {
        insert_adaptive_experiment_assignment_tx(tx, assignment).await?;
    }
    Ok(())
}

async fn upsert_adaptive_experiment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    experiment: &PomodoroAdaptiveExperimentWrite,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_experiments
            (id, policy_id, parameter_key, assignment_unit, status, started_at, ended_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            policy_id = excluded.policy_id,
            parameter_key = excluded.parameter_key,
            assignment_unit = excluded.assignment_unit,
            status = CASE
                WHEN pomodoro_adaptive_experiments.status IN ('completed', 'abandoned')
                     AND excluded.status = 'active'
                THEN pomodoro_adaptive_experiments.status
                ELSE excluded.status
            END,
            started_at = COALESCE(pomodoro_adaptive_experiments.started_at, excluded.started_at),
            ended_at = COALESCE(excluded.ended_at, pomodoro_adaptive_experiments.ended_at)",
    )
    .bind(&experiment.id)
    .bind(&experiment.policy_id)
    .bind(&experiment.parameter_key)
    .bind(&experiment.assignment_unit)
    .bind(&experiment.status)
    .bind(&experiment.started_at)
    .bind(&experiment.ended_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("upsert adaptive experiment: {e}"))?;

    for variant in &experiment.variants {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_experiment_variants
                (experiment_id, variant_key, numeric_value, is_control)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(experiment_id, variant_key) DO UPDATE SET
                numeric_value = excluded.numeric_value,
                is_control = excluded.is_control",
        )
        .bind(&experiment.id)
        .bind(&variant.variant_key)
        .bind(variant.numeric_value)
        .bind(if variant.is_control { 1_i64 } else { 0_i64 })
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("upsert adaptive experiment variant: {e}"))?;
    }

    Ok(())
}

async fn insert_adaptive_experiment_assignment_tx(
    tx: &mut Transaction<'_, Sqlite>,
    envelope: &PomodoroAdaptiveExperimentAssignmentWrite,
) -> Result<(), String> {
    upsert_adaptive_experiment_tx(tx, &envelope.experiment).await?;
    let assignment = &envelope.assignment;
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_assignments
            (id, experiment_id, variant_key, run_id, segment_id, context_snapshot_id,
             assignment_seed, assigned_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&assignment.id)
    .bind(&assignment.experiment_id)
    .bind(&assignment.variant_key)
    .bind(&assignment.run_id)
    .bind(&assignment.segment_id)
    .bind(&assignment.context_snapshot_id)
    .bind(&assignment.assignment_seed)
    .bind(&assignment.assigned_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert adaptive experiment assignment: {e}"))?;
    Ok(())
}

async fn upsert_adaptive_policy_tx(
    tx: &mut Transaction<'_, Sqlite>,
    snapshot: &PomodoroRunAdaptiveSnapshotWrite,
) -> Result<(), String> {
    upsert_adaptive_policy_from_parts_tx(
        tx,
        &snapshot.policy_id,
        snapshot.policy_version,
        snapshot.model_version,
        &snapshot.decision.occurred_at,
    )
    .await
}

async fn upsert_adaptive_policy_from_parts_tx(
    tx: &mut Transaction<'_, Sqlite>,
    policy_id: &str,
    policy_version: i64,
    model_version: i64,
    occurred_at: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_policies
            (id, status, policy_version, model_version, exploration_budget_per_week, created_at, updated_at)
         VALUES (?, 'active', ?, ?, 2, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            policy_version = excluded.policy_version,
            model_version = excluded.model_version,
            updated_at = excluded.updated_at",
    )
    .bind(policy_id)
    .bind(policy_version)
    .bind(model_version)
    .bind(occurred_at)
    .bind(occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("upsert pomodoro adaptive policy: {e}"))?;

    for (parameter_key, min_value, max_value) in [
        ("focus_duration_minutes", 15.0, 60.0),
        ("short_break_minutes", 3.0, 12.0),
        ("long_break_minutes", 10.0, 30.0),
        ("long_break_after_focus_count", 2.0, 5.0),
    ] {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_policy_bounds
                (policy_id, parameter_key, min_value, max_value)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(policy_id, parameter_key) DO UPDATE SET
                min_value = excluded.min_value,
                max_value = excluded.max_value",
        )
        .bind(policy_id)
        .bind(parameter_key)
        .bind(min_value)
        .bind(max_value)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("upsert pomodoro adaptive policy bounds: {e}"))?;
    }

    Ok(())
}

async fn insert_adaptive_context_snapshot_tx(
    tx: &mut Transaction<'_, Sqlite>,
    snapshot: &PomodoroAdaptiveContextSnapshotWrite,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_context_snapshots
            (id, run_id, segment_id, local_started_at, time_of_day, session_position,
             event_length, workload, energy, environment_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snapshot.id)
    .bind(&snapshot.run_id)
    .bind(&snapshot.segment_id)
    .bind(&snapshot.local_started_at)
    .bind(&snapshot.time_of_day)
    .bind(&snapshot.session_position)
    .bind(&snapshot.event_length)
    .bind(&snapshot.workload)
    .bind(&snapshot.energy)
    .bind(&snapshot.environment_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro adaptive context snapshot: {e}"))?;

    for feature in &snapshot.features {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_context_snapshot_features
                (snapshot_id, feature_key, numeric_value, categorical_value,
                 boolean_value, missing, source_kind)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&snapshot.id)
        .bind(&feature.feature_key)
        .bind(feature.numeric_value)
        .bind(&feature.categorical_value)
        .bind(
            feature
                .boolean_value
                .map(|value| if value { 1_i64 } else { 0_i64 }),
        )
        .bind(i64::from(feature.missing))
        .bind(&feature.source_kind)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert pomodoro adaptive context feature: {e}"))?;
    }

    for flag in &snapshot.data_quality_flags {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_data_quality_flags (snapshot_id, flag)
             VALUES (?, ?)",
        )
        .bind(&snapshot.id)
        .bind(flag)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert pomodoro adaptive data quality flag: {e}"))?;
    }

    Ok(())
}

async fn insert_adaptive_decision_tx(
    tx: &mut Transaction<'_, Sqlite>,
    decision: &PomodoroAdaptiveDecisionWrite,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pomodoro_adaptive_decisions
            (id, policy_id, run_id, segment_id, context_snapshot_id, opportunity_kind,
             candidate_id, decision_mode, policy_version, model_version, occurred_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&decision.id)
    .bind(&decision.policy_id)
    .bind(&decision.run_id)
    .bind(&decision.segment_id)
    .bind(&decision.context_snapshot_id)
    .bind(&decision.opportunity_kind)
    .bind(&decision.candidate_id)
    .bind(&decision.decision_mode)
    .bind(decision.policy_version)
    .bind(decision.model_version)
    .bind(&decision.occurred_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro adaptive decision: {e}"))?;

    for value in &decision.values {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_values
                (decision_id, value_key, previous_numeric_value, selected_numeric_value, value_unit)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&decision.id)
        .bind(&value.value_key)
        .bind(value.previous_numeric_value)
        .bind(value.selected_numeric_value)
        .bind(&value.value_unit)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert pomodoro adaptive decision value: {e}"))?;
    }

    for reason_code in &decision.reason_codes {
        sqlx::query(
            "INSERT INTO pomodoro_adaptive_decision_reasons (decision_id, reason_code)
             VALUES (?, ?)",
        )
        .bind(&decision.id)
        .bind(reason_code)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("insert pomodoro adaptive decision reason: {e}"))?;
    }

    sqlx::query(
        "INSERT INTO pomodoro_adaptive_decision_state_scores
            (decision_id, readiness, strain, recovery_debt, avoidance_pressure, momentum, confidence)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&decision.id)
    .bind(decision.state_scores.readiness)
    .bind(decision.state_scores.strain)
    .bind(decision.state_scores.recovery_debt)
    .bind(decision.state_scores.avoidance_pressure)
    .bind(decision.state_scores.momentum)
    .bind(decision.state_scores.confidence)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert pomodoro adaptive decision state scores: {e}"))?;

    Ok(())
}
