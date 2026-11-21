use super::*;
use crate::state::SESSION_TRANSITIONS;
use std::time::Duration;

fn turn_id(value: &str) -> ChatTurnId {
    ChatTurnId::new(value.to_string()).unwrap()
}

fn request_id(value: &str) -> ProviderRequestId {
    ProviderRequestId::new(value.to_string()).unwrap()
}

fn applied(
    mut phase: RuntimePhase,
    inputs: impl IntoIterator<Item = RuntimeInput>,
) -> RuntimePhase {
    for input in inputs {
        phase.apply(input).unwrap();
    }
    phase
}

fn running(turn: &str) -> RuntimePhase {
    applied(
        RuntimePhase::Ready,
        [
            RuntimeInput::TurnAccepted(turn_id(turn)),
            RuntimeInput::TurnStarted(turn_id(turn)),
        ],
    )
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

/// One phase of every variant, with and without a turn and open requests where they apply.
fn every_phase() -> Vec<RuntimePhase> {
    let waiting = applied(
        running("turn-1"),
        [RuntimeInput::RequestOpened {
            request_id: request_id("request-1"),
            kind: OpenRequestKind::Approval,
        }],
    );
    let dispatching = applied(
        RuntimePhase::Ready,
        [RuntimeInput::TurnAccepted(turn_id("turn-1"))],
    );
    let interrupt_requested = applied(
        waiting.clone(),
        [RuntimeInput::InterruptRequested(turn_id("turn-1"))],
    );
    let interrupting = applied(
        interrupt_requested.clone(),
        [RuntimeInput::InterruptAccepted {
            turn_id: turn_id("turn-1"),
            deadline: deadline(),
        }],
    );
    let stopping_turn = applied(running("turn-1"), [RuntimeInput::StopStarted]);
    let failed_turn = applied(running("turn-1"), [RuntimeInput::ProviderFailed]);
    vec![
        RuntimePhase::Stopped,
        RuntimePhase::Starting,
        RuntimePhase::Ready,
        dispatching,
        running("turn-1"),
        waiting,
        interrupt_requested,
        interrupting,
        RuntimePhase::Stopping { turn: None },
        stopping_turn,
        RuntimePhase::Failed { turn: None },
        failed_turn,
    ]
}

/// Every input, for the turn and request the phases above hold and for ones they do not.
fn every_input() -> Vec<RuntimeInput> {
    let mut inputs = vec![
        RuntimeInput::SessionStarting,
        RuntimeInput::SessionOpened,
        RuntimeInput::SessionOpenFailed,
        RuntimeInput::StopStarted,
        RuntimeInput::StopFinished { clean: true },
        RuntimeInput::StopFinished { clean: false },
        RuntimeInput::ProviderStopped,
        RuntimeInput::ProviderFailed,
        RuntimeInput::ProviderExited { expected: true },
        RuntimeInput::ProviderExited { expected: false },
    ];
    for turn in ["turn-1", "turn-other"] {
        inputs.extend([
            RuntimeInput::TurnAccepted(turn_id(turn)),
            RuntimeInput::TurnWithdrawn(turn_id(turn)),
            RuntimeInput::DispatchFailed(turn_id(turn)),
            RuntimeInput::TurnStarted(turn_id(turn)),
            RuntimeInput::TurnSettled(turn_id(turn)),
            RuntimeInput::InterruptRequested(turn_id(turn)),
            RuntimeInput::InterruptAccepted {
                turn_id: turn_id(turn),
                deadline: deadline(),
            },
            RuntimeInput::InterruptRejected(turn_id(turn)),
        ]);
    }
    for request in ["request-1", "request-2"] {
        for kind in [OpenRequestKind::Approval, OpenRequestKind::UserInput] {
            inputs.push(RuntimeInput::RequestOpened {
                request_id: request_id(request),
                kind,
            });
        }
        inputs.push(RuntimeInput::RequestClosed(request_id(request)));
    }
    inputs
}

#[test]
fn every_applied_move_reports_a_declared_session_transition() {
    let mut undeclared = std::collections::BTreeSet::new();
    for phase in every_phase() {
        for input in every_input() {
            let mut next = phase.clone();
            let from = phase.session_state();
            if next.apply(input.clone()).is_ok() {
                let to = next.session_state();
                if from != to && !SESSION_TRANSITIONS.contains(&(from, to)) {
                    undeclared.insert(format!("{from:?} to {to:?} on {}", input.name()));
                }
            }
        }
    }
    assert!(undeclared.is_empty(), "undeclared moves: {undeclared:#?}");
}

#[test]
fn rejected_moves_leave_the_phase_unchanged() {
    let mut rejected = 0;
    for phase in every_phase() {
        for input in every_input() {
            let mut next = phase.clone();
            if let Err(rejection) = next.apply(input.clone()) {
                rejected += 1;
                assert_eq!(next, phase, "{input:?} changed a phase it rejected");
                assert_eq!(rejection.from, phase.session_state());
            }
        }
    }
    assert!(rejected > 0);
}

#[test]
fn turn_events_only_move_the_turn_they_name() {
    for phase in every_phase() {
        for input in [
            RuntimeInput::TurnWithdrawn(turn_id("turn-other")),
            RuntimeInput::DispatchFailed(turn_id("turn-other")),
            RuntimeInput::TurnStarted(turn_id("turn-other")),
            RuntimeInput::TurnSettled(turn_id("turn-other")),
            RuntimeInput::InterruptRequested(turn_id("turn-other")),
            RuntimeInput::InterruptAccepted {
                turn_id: turn_id("turn-other"),
                deadline: deadline(),
            },
            RuntimeInput::InterruptRejected(turn_id("turn-other")),
        ] {
            let mut next = phase.clone();
            assert!(
                next.apply(input.clone()).is_err(),
                "{phase:?} took {input:?}"
            );
        }
    }
}

#[test]
fn derived_flags_follow_the_phase() {
    for phase in every_phase() {
        let state = phase.session_state();
        assert_eq!(
            phase.turn_active(),
            matches!(
                phase,
                RuntimePhase::Turn(_)
                    | RuntimePhase::InterruptRequested(_)
                    | RuntimePhase::Interrupting { .. }
            ),
            "{phase:?}"
        );
        if phase.turn_active() {
            assert!(phase.active_turn_id().is_some());
        }
        if state == ProviderSessionState::Ready {
            assert!(phase.active_turn_id().is_none());
            assert!(!phase.has_open_requests());
        }
        assert_eq!(
            phase.interrupt_deadline().is_some(),
            matches!(phase, RuntimePhase::Interrupting { .. })
        );
    }
}

#[test]
fn a_dispatched_turn_reports_active_and_settles_before_or_after_its_start() {
    let dispatching = applied(
        RuntimePhase::Ready,
        [RuntimeInput::TurnAccepted(turn_id("turn-1"))],
    );
    assert_eq!(dispatching.session_state(), ProviderSessionState::Active);
    assert_eq!(
        dispatching.turn().map(RuntimeTurn::progress),
        Some(TurnProgress::Dispatching)
    );
    let started = applied(
        dispatching.clone(),
        [RuntimeInput::TurnStarted(turn_id("turn-1"))],
    );
    assert_eq!(
        started.turn().map(RuntimeTurn::progress),
        Some(TurnProgress::Running)
    );
    for phase in [dispatching, started] {
        assert_eq!(
            applied(phase, [RuntimeInput::TurnSettled(turn_id("turn-1"))]),
            RuntimePhase::Ready
        );
    }
}

#[test]
fn open_requests_are_tracked_by_id_and_the_oldest_sets_the_waiting_state() {
    let mut phase = running("turn-1");
    for (id, kind) in [
        ("approval-a", OpenRequestKind::Approval),
        ("input-b", OpenRequestKind::UserInput),
        ("approval-c", OpenRequestKind::Approval),
    ] {
        phase
            .apply(RuntimeInput::RequestOpened {
                request_id: request_id(id),
                kind,
            })
            .unwrap();
    }
    assert_eq!(
        phase.session_state(),
        ProviderSessionState::WaitingForApproval
    );
    assert_eq!(
        phase.open_request(&request_id("input-b")),
        Some(OpenRequestKind::UserInput)
    );
    phase
        .apply(RuntimeInput::RequestOpened {
            request_id: request_id("approval-a"),
            kind: OpenRequestKind::Approval,
        })
        .unwrap();
    assert_eq!(
        phase.turn().unwrap().requests().len(),
        3,
        "reopening is idempotent"
    );

    phase
        .apply(RuntimeInput::RequestClosed(request_id("approval-a")))
        .unwrap();
    assert_eq!(
        phase.session_state(),
        ProviderSessionState::WaitingForUserInput
    );
    assert_eq!(phase.open_request(&request_id("approval-a")), None);
    phase
        .apply(RuntimeInput::RequestClosed(request_id("approval-a")))
        .unwrap();
    phase
        .apply(RuntimeInput::RequestClosed(request_id("input-b")))
        .unwrap();
    assert_eq!(
        phase.session_state(),
        ProviderSessionState::WaitingForApproval
    );
    phase
        .apply(RuntimeInput::RequestClosed(request_id("approval-c")))
        .unwrap();
    assert_eq!(phase.session_state(), ProviderSessionState::Active);
    assert!(!phase.has_open_requests());
}

#[test]
fn settling_a_turn_drops_its_open_requests() {
    let phase = applied(
        running("turn-1"),
        [
            RuntimeInput::RequestOpened {
                request_id: request_id("request-1"),
                kind: OpenRequestKind::Approval,
            },
            RuntimeInput::TurnSettled(turn_id("turn-1")),
        ],
    );
    assert_eq!(phase, RuntimePhase::Ready);
    assert!(!phase.has_open_requests());
}

#[test]
fn requests_open_only_inside_a_turn() {
    for mut phase in [
        RuntimePhase::Stopped,
        RuntimePhase::Starting,
        RuntimePhase::Ready,
        RuntimePhase::Stopping { turn: None },
        RuntimePhase::Failed { turn: None },
    ] {
        assert!(
            phase
                .apply(RuntimeInput::RequestOpened {
                    request_id: request_id("request-1"),
                    kind: OpenRequestKind::Approval,
                })
                .is_err()
        );
    }
}

#[test]
fn an_interrupt_waits_for_its_deadline_and_a_settle_clears_it() {
    let at = deadline();
    let interrupting = applied(
        running("turn-1"),
        [
            RuntimeInput::InterruptRequested(turn_id("turn-1")),
            RuntimeInput::InterruptAccepted {
                turn_id: turn_id("turn-1"),
                deadline: at,
            },
        ],
    );
    assert_eq!(interrupting.session_state(), ProviderSessionState::Stopping);
    assert!(interrupting.turn_active());
    assert_eq!(
        interrupting.interrupt_deadline(),
        Some((&turn_id("turn-1"), at))
    );
    let settled = applied(interrupting, [RuntimeInput::TurnSettled(turn_id("turn-1"))]);
    assert_eq!(settled, RuntimePhase::Ready);
    assert_eq!(settled.interrupt_deadline(), None);
}

#[test]
fn a_refused_interrupt_restores_the_turn_with_its_requests() {
    let waiting = applied(
        running("turn-1"),
        [RuntimeInput::RequestOpened {
            request_id: request_id("request-1"),
            kind: OpenRequestKind::UserInput,
        }],
    );
    let restored = applied(
        waiting.clone(),
        [
            RuntimeInput::InterruptRequested(turn_id("turn-1")),
            RuntimeInput::InterruptRejected(turn_id("turn-1")),
        ],
    );
    assert_eq!(restored, waiting);
}

#[test]
fn an_interrupt_answer_after_the_turn_settled_changes_nothing() {
    let mut settled = applied(
        running("turn-1"),
        [
            RuntimeInput::InterruptRequested(turn_id("turn-1")),
            RuntimeInput::TurnSettled(turn_id("turn-1")),
        ],
    );
    for input in [
        RuntimeInput::InterruptAccepted {
            turn_id: turn_id("turn-1"),
            deadline: deadline(),
        },
        RuntimeInput::InterruptRejected(turn_id("turn-1")),
    ] {
        assert!(settled.apply(input).is_err());
        assert_eq!(settled, RuntimePhase::Ready);
    }
}

#[test]
fn a_failed_dispatch_leaves_no_running_turn() {
    let failed = applied(
        RuntimePhase::Ready,
        [
            RuntimeInput::TurnAccepted(turn_id("turn-1")),
            RuntimeInput::DispatchFailed(turn_id("turn-1")),
        ],
    );
    assert_eq!(failed, RuntimePhase::Failed { turn: None });
    assert!(!failed.turn_active());
    assert_eq!(failed.active_turn_id(), None);
}

#[test]
fn a_withdrawn_compaction_returns_to_ready() {
    let phase = applied(
        RuntimePhase::Ready,
        [
            RuntimeInput::TurnAccepted(turn_id("compact-1")),
            RuntimeInput::TurnWithdrawn(turn_id("compact-1")),
        ],
    );
    assert_eq!(phase, RuntimePhase::Ready);
}

#[test]
fn a_stop_keeps_accepting_the_settlement_of_the_turn_it_interrupts() {
    let mut stopping = applied(running("turn-1"), [RuntimeInput::StopStarted]);
    assert_eq!(stopping.session_state(), ProviderSessionState::Stopping);
    assert!(!stopping.turn_active());
    assert_eq!(stopping.active_turn_id(), Some(&turn_id("turn-1")));
    stopping
        .apply(RuntimeInput::TurnSettled(turn_id("turn-1")))
        .unwrap();
    assert_eq!(stopping, RuntimePhase::Stopping { turn: None });
    for clean in [true, false] {
        let finished = applied(stopping.clone(), [RuntimeInput::StopFinished { clean }]);
        assert_eq!(
            finished.session_state() == ProviderSessionState::Stopped,
            clean
        );
    }
}

#[test]
fn a_provider_failure_keeps_the_unsettled_turn_until_it_settles() {
    let mut failed = applied(running("turn-1"), [RuntimeInput::ProviderFailed]);
    assert_eq!(failed.session_state(), ProviderSessionState::Failed);
    assert!(!failed.turn_active());
    assert_eq!(failed.active_turn_id(), Some(&turn_id("turn-1")));
    failed
        .apply(RuntimeInput::TurnSettled(turn_id("turn-1")))
        .unwrap();
    assert_eq!(failed, RuntimePhase::Failed { turn: None });
    assert!(failed.can_start());
}

#[test]
fn a_provider_exit_drops_the_turn() {
    for expected in [true, false] {
        let exited = applied(
            running("turn-1"),
            [RuntimeInput::ProviderExited { expected }],
        );
        assert_eq!(exited.active_turn_id(), None);
        assert_eq!(
            exited.session_state(),
            if expected {
                ProviderSessionState::Stopped
            } else {
                ProviderSessionState::Failed
            }
        );
    }
}

#[test]
fn sessions_start_only_when_none_runs() {
    for phase in every_phase() {
        let mut next = phase.clone();
        assert_eq!(
            next.apply(RuntimeInput::SessionStarting).is_ok(),
            phase.can_start(),
            "{phase:?}"
        );
    }
}
