use super::super::*;
use ganbaru_db::run_migrations;

pub(super) async fn migrated_pool_with_event() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO calendar_events (id, title, start_time, end_time)
             VALUES ('event-1', 'Focus block', '2026-05-29T10:00:00Z', '2026-05-29T11:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

pub(super) fn run_write(rhythm: PomodoroRunRhythm) -> PomodoroRunWrite {
    PomodoroRunWrite {
        id: "run-1".to_string(),
        event_id: "event-1".to_string(),
        event_date: "2026-05-29".to_string(),
        planned_start: "2026-05-29T10:00:00Z".to_string(),
        planned_end: "2026-05-29T11:00:00Z".to_string(),
        started_at: "2026-05-29T10:00:00Z".to_string(),
        rhythm,
        rhythm_source: "custom".to_string(),
        preset_key: None,
        idle_timeout_minutes: Some(3),
        event_title_snapshot: Some("Focus block".to_string()),
        inherited_focus_minutes: 0,
        inherited_rhythm_position: 1,
        inherited_from_run_id: None,
        start_trigger: "manual".to_string(),
        adaptive_snapshot: None,
    }
}

pub(super) fn initial_segment() -> PomodoroSegmentWrite {
    PomodoroSegmentWrite {
        id: "segment-1".to_string(),
        event_id: "event-1".to_string(),
        event_date: "2026-05-29".to_string(),
        run_id: "run-1".to_string(),
        rhythm_position: 1,
        phase: "focus".to_string(),
        planned_start: "2026-05-29T10:00:00Z".to_string(),
        planned_end: "2026-05-29T10:40:00Z".to_string(),
        actual_start: Some("2026-05-29T10:00:00Z".to_string()),
        actual_end: None,
        pauses: Vec::new(),
        status: "active".to_string(),
        end_reason: None,
    }
}

pub(super) fn adaptive_snapshot() -> PomodoroRunAdaptiveSnapshotWrite {
    PomodoroRunAdaptiveSnapshotWrite {
        policy_id: "local-adaptive-policy-v1".to_string(),
        policy_version: 1,
        model_version: 1,
        context_snapshot: PomodoroAdaptiveContextSnapshotWrite {
            id: "context-1".to_string(),
            run_id: "run-1".to_string(),
            segment_id: Some("segment-1".to_string()),
            local_started_at: "2026-05-29T10:00:00Z".to_string(),
            time_of_day: "morning".to_string(),
            session_position: "first".to_string(),
            event_length: "medium".to_string(),
            workload: "low".to_string(),
            energy: "unknown".to_string(),
            environment_id: None,
            features: vec![PomodoroAdaptiveFeatureWrite {
                feature_key: "comparable_opportunity_count".to_string(),
                numeric_value: Some(0.0),
                categorical_value: None,
                boolean_value: None,
                missing: false,
                source_kind: "pomodoro".to_string(),
            }],
            data_quality_flags: vec!["diary_missing".to_string()],
        },
        decision: PomodoroAdaptiveDecisionWrite {
            id: "decision-1".to_string(),
            policy_id: "local-adaptive-policy-v1".to_string(),
            run_id: "run-1".to_string(),
            segment_id: Some("segment-1".to_string()),
            context_snapshot_id: "context-1".to_string(),
            opportunity_kind: "run_start".to_string(),
            candidate_id: None,
            decision_mode: "fallback".to_string(),
            policy_version: 1,
            model_version: 1,
            occurred_at: "2026-05-29T10:00:00Z".to_string(),
            values: vec![
                adaptive_value("focus_duration_minutes", 40.0, "minutes"),
                adaptive_value("short_break_minutes", 5.0, "minutes"),
                adaptive_value("long_break_minutes", 10.0, "minutes"),
                adaptive_value("long_break_after_focus_count", 4.0, "count"),
            ],
            reason_codes: vec!["no_history".to_string(), "missing_diary_data".to_string()],
            state_scores: PomodoroAdaptiveStateScoresWrite {
                readiness: 0.0,
                strain: 0.0,
                recovery_debt: 0.0,
                avoidance_pressure: 0.0,
                momentum: 0.0,
                confidence: 0.0,
            },
        },
        planned_blocks: Vec::new(),
        experiment_updates: Vec::new(),
        experiment_assignments: Vec::new(),
    }
}

pub(super) fn adaptive_boundary_envelope(
    segment: &PomodoroSegmentWrite,
) -> PomodoroAdaptiveDecisionEnvelopeWrite {
    PomodoroAdaptiveDecisionEnvelopeWrite {
        policy_id: "local-adaptive-policy-v1".to_string(),
        policy_version: 1,
        model_version: 1,
        context_snapshot: PomodoroAdaptiveContextSnapshotWrite {
            id: "context-boundary-1".to_string(),
            run_id: segment.run_id.clone(),
            segment_id: Some(segment.id.clone()),
            local_started_at: "2026-05-29T10:40:00Z".to_string(),
            time_of_day: "morning".to_string(),
            session_position: "first".to_string(),
            event_length: "medium".to_string(),
            workload: "low".to_string(),
            energy: "unknown".to_string(),
            environment_id: None,
            features: vec![PomodoroAdaptiveFeatureWrite {
                feature_key: "comparable_opportunity_count".to_string(),
                numeric_value: Some(4.0),
                categorical_value: None,
                boolean_value: None,
                missing: false,
                source_kind: "pomodoro".to_string(),
            }],
            data_quality_flags: vec!["diary_missing".to_string()],
        },
        decision: PomodoroAdaptiveDecisionWrite {
            id: "decision-boundary-1".to_string(),
            policy_id: "local-adaptive-policy-v1".to_string(),
            run_id: segment.run_id.clone(),
            segment_id: Some(segment.id.clone()),
            context_snapshot_id: "context-boundary-1".to_string(),
            opportunity_kind: "break_start".to_string(),
            candidate_id: None,
            decision_mode: "hold".to_string(),
            policy_version: 1,
            model_version: 1,
            occurred_at: "2026-05-29T10:40:00Z".to_string(),
            values: vec![
                adaptive_value("focus_duration_minutes", 40.0, "minutes"),
                adaptive_value("short_break_minutes", 5.0, "minutes"),
                adaptive_value("long_break_minutes", 10.0, "minutes"),
                adaptive_value("long_break_after_focus_count", 4.0, "count"),
            ],
            reason_codes: vec!["hold_current_rhythm".to_string()],
            state_scores: PomodoroAdaptiveStateScoresWrite {
                readiness: 0.5,
                strain: 0.1,
                recovery_debt: 0.1,
                avoidance_pressure: 0.1,
                momentum: 0.6,
                confidence: 0.5,
            },
        },
        experiment_updates: Vec::new(),
        experiment_assignments: Vec::new(),
    }
}

pub(super) fn adaptive_snapshot_with_assignment() -> PomodoroRunAdaptiveSnapshotWrite {
    let mut snapshot = adaptive_snapshot();
    snapshot.decision.decision_mode = "explore".to_string();
    snapshot
        .decision
        .reason_codes
        .push("experiment_assignment".to_string());
    snapshot.decision.values[0].selected_numeric_value = 45.0;
    snapshot.experiment_assignments = vec![PomodoroAdaptiveExperimentAssignmentWrite {
        experiment: adaptive_experiment("active"),
        assignment: PomodoroAdaptiveAssignmentWrite {
            id: "assignment-1".to_string(),
            experiment_id: "run-focus-duration-40-vs-45-v1".to_string(),
            variant_key: "focus_45".to_string(),
            run_id: "run-1".to_string(),
            segment_id: Some("segment-1".to_string()),
            context_snapshot_id: "context-1".to_string(),
            assignment_seed: "seed-1".to_string(),
            assigned_at: "2026-05-29T10:00:00Z".to_string(),
        },
    }];
    snapshot
}

pub(super) fn adaptive_experiment(status: &str) -> PomodoroAdaptiveExperimentWrite {
    PomodoroAdaptiveExperimentWrite {
        id: "run-focus-duration-40-vs-45-v1".to_string(),
        policy_id: "local-adaptive-policy-v1".to_string(),
        parameter_key: "focus_duration_minutes".to_string(),
        assignment_unit: "run".to_string(),
        status: status.to_string(),
        started_at: Some("2026-05-29T10:00:00Z".to_string()),
        ended_at: if status == "completed" || status == "abandoned" {
            Some("2026-05-29T10:00:00Z".to_string())
        } else {
            None
        },
        variants: vec![
            PomodoroAdaptiveExperimentVariantWrite {
                variant_key: "control_40".to_string(),
                numeric_value: 40.0,
                is_control: true,
            },
            PomodoroAdaptiveExperimentVariantWrite {
                variant_key: "focus_45".to_string(),
                numeric_value: 45.0,
                is_control: false,
            },
        ],
    }
}

pub(super) fn adaptive_bundle_experiment(status: &str) -> PomodoroAdaptiveExperimentWrite {
    PomodoroAdaptiveExperimentWrite {
        id: "run-focus-short-break-support-40-5-vs-45-7-v1".to_string(),
        policy_id: "local-adaptive-policy-v1".to_string(),
        parameter_key: "rhythm_bundle".to_string(),
        assignment_unit: "run".to_string(),
        status: status.to_string(),
        started_at: Some("2026-05-29T10:00:00Z".to_string()),
        ended_at: if status == "completed" || status == "abandoned" {
            Some("2026-05-29T10:00:00Z".to_string())
        } else {
            None
        },
        variants: vec![
            PomodoroAdaptiveExperimentVariantWrite {
                variant_key: "control_40_5".to_string(),
                numeric_value: 0.0,
                is_control: true,
            },
            PomodoroAdaptiveExperimentVariantWrite {
                variant_key: "focus_45_short_break_7".to_string(),
                numeric_value: 1.0,
                is_control: false,
            },
        ],
    }
}

pub(super) fn adaptive_value(
    value_key: &str,
    selected_numeric_value: f64,
    value_unit: &str,
) -> PomodoroAdaptiveDecisionValueWrite {
    PomodoroAdaptiveDecisionValueWrite {
        value_key: value_key.to_string(),
        previous_numeric_value: Some(selected_numeric_value),
        selected_numeric_value,
        value_unit: value_unit.to_string(),
    }
}

pub(super) fn adaptive_planned_block() -> PomodoroAdaptivePlannedBlockWrite {
    PomodoroAdaptivePlannedBlockWrite {
        event_date: "2026-05-29".to_string(),
        event_id: Some("event-1::2026-05-29".to_string()),
        original_event_id: "event-1".to_string(),
        planned_start: "2026-05-29T10:00:00Z".to_string(),
        planned_end: "2026-05-29T11:00:00Z".to_string(),
        source_kind: "scheduler_snapshot".to_string(),
    }
}
