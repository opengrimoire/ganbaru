use super::super::*;

#[test]
fn validates_segment_enums() {
    assert!(validate_phase("focus").is_ok());
    assert!(validate_phase("short_break").is_ok());
    assert!(validate_phase("wrong").is_err());
}

#[test]
fn validates_pause_reason() {
    assert!(validate_pause_reason("idle").is_ok());
    assert!(validate_pause_reason("manual").is_ok());
    assert!(validate_pause_reason("suspend").is_ok());
    assert!(validate_pause_reason("unknown").is_err());
}

#[test]
fn validates_runtime_rhythm_limits() {
    assert!(
        validate_run_rhythm(&PomodoroRunRhythm::Count {
            focus_duration_minutes: 120,
            short_break_minutes: 30,
            long_break_minutes: 60,
            long_break_after_focus_count: 12,
        })
        .is_ok()
    );
    assert!(
        validate_run_rhythm(&PomodoroRunRhythm::Count {
            focus_duration_minutes: 121,
            short_break_minutes: 5,
            long_break_minutes: 10,
            long_break_after_focus_count: 4,
        })
        .is_err()
    );
    assert!(
        validate_run_rhythm(&PomodoroRunRhythm::Sequence {
            steps: vec![PomodoroRunSequenceStep {
                focus_duration_minutes: 25,
                break_phase: "short_break".to_string(),
                break_duration_minutes: 31,
            }],
        })
        .is_err()
    );
}

#[test]
fn validates_run_event_types() {
    assert!(validate_event_type("skip_break").is_ok());
    assert!(validate_event_type("extend_focus").is_ok());
    assert!(validate_event_type("go_to_break_now").is_ok());
    assert!(validate_event_type("start_focus_now").is_ok());
    assert!(validate_event_type("focus_failed").is_ok());
    assert!(validate_event_type("unknown").is_err());
}

#[test]
fn canonicalizes_recurring_instance_ids() {
    assert_eq!(canonical_event_id("event-1").unwrap(), "event-1");
    assert_eq!(
        canonical_event_id("event-1::2026-05-09").unwrap(),
        "event-1"
    );
    assert!(canonical_event_id("::2026-05-09").is_err());
}
