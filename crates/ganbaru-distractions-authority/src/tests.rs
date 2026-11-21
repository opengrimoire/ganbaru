use super::*;
use serde_json::json;

fn active(phase: BlockingPhase, pause_reason: Option<PauseReason>) -> RuntimeAuthority {
    RuntimeAuthority::Active(ActivePhase {
        phase,
        pause_reason,
        run_id: "run-1".into(),
        occurrence_id: None,
        remaining_seconds: 60,
    })
}

/// Every representable authority: inactive, and each phase running or paused for each reason.
fn every_authority() -> Vec<RuntimeAuthority> {
    let mut states = vec![RuntimeAuthority::Inactive {}];
    for phase in BlockingPhase::ALL {
        states.push(active(phase, None));
        for reason in PauseReason::ALL {
            states.push(active(phase, Some(reason)));
        }
    }
    states
}

fn every_setting() -> Vec<PhaseBlockingSettings> {
    (0..16_u8)
        .map(|bits| PhaseBlockingSettings {
            block_during_focus: bits & 1 != 0,
            block_during_short_breaks: bits & 2 != 0,
            block_during_long_breaks: bits & 4 != 0,
            pause_during_focus_pause: bits & 8 != 0,
        })
        .collect()
}

fn valid_file() -> Value {
    json!({
        "authority": {
            "status": "active",
            "phase": "focus",
            "pauseReason": null,
            "runId": "run-1",
            "occurrenceId": null,
            "remainingSeconds": 60
        },
        "updatedAt": "2026-05-26T00:00:00.000Z",
        "validUntilMs": 1_000
    })
}

#[test]
fn enforcement_covers_every_state_and_setting_combination() {
    let states = every_authority();
    let settings = every_setting();
    assert_eq!(states.len(), 13);
    assert_eq!(settings.len(), 16);
    for state in &states {
        for setting in &settings {
            let expected = match state {
                RuntimeAuthority::Inactive {} => false,
                RuntimeAuthority::Active(active) => {
                    let phase_setting = match active.phase {
                        BlockingPhase::Focus => setting.block_during_focus,
                        BlockingPhase::ShortBreak => setting.block_during_short_breaks,
                        BlockingPhase::LongBreak => setting.block_during_long_breaks,
                    };
                    match active.pause_reason {
                        None | Some(PauseReason::Idle | PauseReason::Suspend) => phase_setting,
                        Some(PauseReason::Manual) => {
                            phase_setting && !setting.pause_during_focus_pause
                        }
                    }
                }
            };
            assert_eq!(
                state.enforces_blocking(*setting),
                expected,
                "{state:?} with {setting:?}"
            );
        }
    }
}

#[test]
fn strict_pauses_keep_blocking_and_manual_pause_respects_the_setting() {
    let defaults = PhaseBlockingSettings::default();
    assert!(active(BlockingPhase::Focus, None).enforces_blocking(defaults));
    assert!(active(BlockingPhase::Focus, Some(PauseReason::Idle)).enforces_blocking(defaults));
    assert!(active(BlockingPhase::Focus, Some(PauseReason::Suspend)).enforces_blocking(defaults));
    assert!(!active(BlockingPhase::Focus, Some(PauseReason::Manual)).enforces_blocking(defaults));
    let keep_blocking_on_pause = PhaseBlockingSettings {
        pause_during_focus_pause: false,
        ..defaults
    };
    assert!(
        active(BlockingPhase::Focus, Some(PauseReason::Manual))
            .enforces_blocking(keep_blocking_on_pause)
    );
    assert!(!RuntimeAuthority::Inactive {}.enforces_blocking(defaults));
}

#[test]
fn settings_default_on_when_missing_or_not_boolean() {
    assert_eq!(
        PhaseBlockingSettings::from_rules(&json!({})),
        PhaseBlockingSettings::default()
    );
    assert_eq!(
        PhaseBlockingSettings::from_rules(&json!({ "blockDuringFocus": "no" })),
        PhaseBlockingSettings::default()
    );
    assert_eq!(
        PhaseBlockingSettings::from_rules(&json!({
            "blockDuringFocus": false,
            "blockDuringShortBreaks": true,
            "blockDuringLongBreaks": false,
            "pauseDuringFocusPause": false
        })),
        PhaseBlockingSettings {
            block_during_focus: false,
            block_during_short_breaks: true,
            block_during_long_breaks: false,
            pause_during_focus_pause: false,
        }
    );
}

#[test]
fn block_event_phase_names_the_phase_or_the_pause() {
    assert_eq!(RuntimeAuthority::Inactive {}.block_event_phase(), None);
    for phase in BlockingPhase::ALL {
        assert_eq!(
            active(phase, None).block_event_phase(),
            Some(phase.as_str())
        );
    }
    assert_eq!(
        active(BlockingPhase::LongBreak, Some(PauseReason::Manual)).block_event_phase(),
        Some("manual_pause")
    );
    assert_eq!(
        active(BlockingPhase::Focus, Some(PauseReason::Idle)).block_event_phase(),
        Some("idle_pause")
    );
    assert_eq!(
        active(BlockingPhase::Focus, Some(PauseReason::Suspend)).block_event_phase(),
        Some("suspend_pause")
    );
}

#[test]
fn every_state_round_trips_through_the_file_format() {
    for authority in every_authority() {
        let state = DistractionsRuntimeState {
            authority,
            updated_at: "2026-05-26T00:00:00.000Z".into(),
            valid_until_ms: 1_000,
        };
        let encoded = serde_json::to_string(&state).unwrap();
        assert_eq!(DistractionsRuntimeState::parse(&encoded).unwrap(), state);
    }
    let inactive = json!({
        "authority": { "status": "inactive" },
        "updatedAt": "2026-05-26T00:00:00.000Z",
        "validUntilMs": 0
    });
    assert_eq!(
        DistractionsRuntimeState::parse(&inactive.to_string())
            .unwrap()
            .authority,
        RuntimeAuthority::Inactive {}
    );
    assert!(DistractionsRuntimeState::parse(&valid_file().to_string()).is_ok());
}

#[test]
fn rejects_malformed_state_files() {
    let mutations: [(&str, fn(&mut Value)); 16] = [
        ("unknown status", |file| {
            file["authority"]["status"] = json!("running")
        }),
        ("unknown phase", |file| {
            file["authority"]["phase"] = json!("inactive")
        }),
        ("unknown pause reason", |file| {
            file["authority"]["pauseReason"] = json!("network")
        }),
        ("missing pause reason", |file| {
            file["authority"]
                .as_object_mut()
                .unwrap()
                .remove("pauseReason");
        }),
        ("missing occurrence", |file| {
            file["authority"]
                .as_object_mut()
                .unwrap()
                .remove("occurrenceId");
        }),
        ("missing run id", |file| {
            file["authority"].as_object_mut().unwrap().remove("runId");
        }),
        ("empty run id", |file| {
            file["authority"]["runId"] = json!(" ")
        }),
        ("missing remaining time", |file| {
            file["authority"]
                .as_object_mut()
                .unwrap()
                .remove("remainingSeconds");
        }),
        ("negative remaining time", |file| {
            file["authority"]["remainingSeconds"] = json!(-1)
        }),
        ("unknown phase field", |file| {
            file["authority"]["paused"] = json!(true)
        }),
        ("phase data on an inactive state", |file| {
            file["authority"]["status"] = json!("inactive")
        }),
        ("missing validity", |file| {
            file.as_object_mut().unwrap().remove("validUntilMs");
        }),
        ("negative validity", |file| file["validUntilMs"] = json!(-1)),
        ("empty update time", |file| file["updatedAt"] = json!("")),
        ("unknown top-level field", |file| {
            file["active"] = json!(true)
        }),
        ("flat legacy shape", |file| {
            *file = json!({
                "active": true,
                "paused": false,
                "pauseReason": null,
                "phase": "focus",
                "activeRunId": "run-1",
                "activeOccurrenceId": null,
                "remainingSeconds": 60,
                "updatedAt": "2026-05-26T00:00:00.000Z",
                "validUntilMs": 1_000
            })
        }),
    ];
    for (label, mutate) in mutations {
        let mut file = valid_file();
        mutate(&mut file);
        assert!(
            DistractionsRuntimeState::parse(&file.to_string()).is_err(),
            "{label} must be rejected"
        );
    }
}

#[test]
fn validity_ends_at_the_deadline_or_when_a_running_phase_elapses() {
    let running = DistractionsRuntimeState {
        authority: active(BlockingPhase::Focus, None),
        updated_at: "2026-05-26T00:00:00.000Z".into(),
        valid_until_ms: 100_000,
    };
    assert!(!running.validity_expired(0, 59_999));
    assert!(running.validity_expired(0, 60_000));
    let paused = DistractionsRuntimeState {
        authority: active(BlockingPhase::Focus, Some(PauseReason::Manual)),
        ..running.clone()
    };
    assert!(!paused.validity_expired(0, 99_999));
    assert!(paused.validity_expired(0, 100_000));
    let inactive = DistractionsRuntimeState {
        authority: RuntimeAuthority::Inactive {},
        ..running
    };
    assert!(!inactive.validity_expired(0, 99_999));
    assert!(inactive.validity_expired(0, 100_000));
}
