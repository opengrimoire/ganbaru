use super::*;
use crate::models::ChatErrorCode;

/// Every legal local assignment move, written out so a table change is deliberate.
const LEGAL_ASSIGNMENT_MOVES: &[(&str, &[&str])] = &[
    (
        "queued",
        &[
            "working",
            "waiting_for_answer",
            "waiting_for_approval",
            "ready_for_review",
            "completed",
            "failed",
            "cancelled",
        ],
    ),
    (
        "working",
        &[
            "working",
            "waiting_for_answer",
            "waiting_for_approval",
            "ready_for_review",
            "completed",
            "failed",
            "cancelled",
        ],
    ),
    (
        "waiting_for_answer",
        &[
            "queued",
            "working",
            "waiting_for_answer",
            "waiting_for_approval",
            "ready_for_review",
            "completed",
            "failed",
            "cancelled",
        ],
    ),
    (
        "waiting_for_approval",
        &[
            "working",
            "waiting_for_answer",
            "waiting_for_approval",
            "ready_for_review",
            "completed",
            "failed",
            "cancelled",
        ],
    ),
    ("ready_for_review", &["completed", "failed", "cancelled"]),
    ("completed", &["cancelled"]),
    ("failed", &["queued", "cancelled"]),
    ("cancelled", &["queued"]),
];

#[test]
fn the_assignment_table_admits_exactly_the_listed_moves() {
    for from in ChatWorkAssignmentState::ALL {
        let (_, targets) = LEGAL_ASSIGNMENT_MOVES
            .iter()
            .find(|(state, _)| *state == from.as_str())
            .expect("every state lists its moves");
        for to in ChatWorkAssignmentState::ALL {
            let legal = targets.contains(&to.as_str());
            assert_eq!(
                can_transition_assignment(from, to),
                legal,
                "{from:?} -> {to:?}"
            );
            match transition_assignment(from, to) {
                Ok(next) => assert!(legal && next == to, "{from:?} -> {to:?}"),
                Err(error) => {
                    assert!(!legal, "{from:?} -> {to:?}");
                    assert_eq!(error.code, ChatErrorCode::InvalidStateTransition);
                }
            }
            assert_eq!(
                assignment_sources(to).any(|source| source == from),
                legal,
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn completed_assignments_never_reopen_and_only_questions_and_retries_requeue() {
    for to in ChatWorkAssignmentState::ALL {
        assert_eq!(
            can_transition_assignment(ChatWorkAssignmentState::Completed, to),
            to == ChatWorkAssignmentState::Cancelled,
            "{to:?}"
        );
    }
    let requeue_sources = assignment_sources(ChatWorkAssignmentState::Queued).collect::<Vec<_>>();
    assert_eq!(
        requeue_sources,
        [
            ChatWorkAssignmentState::WaitingForAnswer,
            ChatWorkAssignmentState::Failed,
            ChatWorkAssignmentState::Cancelled,
        ]
    );
}

#[test]
fn settled_runs_are_absorbing_and_no_run_returns_to_queued_or_starting() {
    for from in ChatAgentRunState::ALL {
        assert_eq!(ChatAgentRunState::parse(from.as_str()), Some(from));
        for to in ChatAgentRunState::ALL {
            let legal = !from.is_settled()
                && !matches!(to, ChatAgentRunState::Queued | ChatAgentRunState::Starting);
            assert_eq!(can_transition_run(from, to), legal, "{from:?} -> {to:?}");
            assert_eq!(
                transition_run(from, to).is_ok(),
                legal,
                "{from:?} -> {to:?}"
            );
            assert_eq!(
                run_sources(to).any(|source| source == from),
                legal,
                "{from:?} -> {to:?}"
            );
        }
    }
    assert_eq!(
        run_sources(ChatAgentRunState::Cancelled)
            .map(ChatAgentRunState::as_str)
            .collect::<Vec<_>>(),
        ["queued", "starting", "working", "waiting"]
    );
}

#[test]
fn dispatch_jobs_are_claimed_only_from_the_queue_and_ended_jobs_only_requeue() {
    use ChatDispatchJobState::{Cancelled, Claimed, Completed, Failed, Queued};
    let legal: &[(ChatDispatchJobState, ChatDispatchJobState)] = &[
        (Queued, Claimed),
        (Queued, Completed),
        (Queued, Failed),
        (Queued, Cancelled),
        (Claimed, Queued),
        (Claimed, Completed),
        (Claimed, Failed),
        (Claimed, Cancelled),
        (Completed, Queued),
        (Failed, Queued),
        (Cancelled, Queued),
    ];
    for from in ChatDispatchJobState::ALL {
        assert_eq!(ChatDispatchJobState::parse(from.as_str()), Some(from));
        for to in ChatDispatchJobState::ALL {
            let expected = legal.contains(&(from, to));
            assert_eq!(
                can_transition_dispatch_job(from, to),
                expected,
                "{from:?} -> {to:?}"
            );
            assert_eq!(
                transition_dispatch_job(from, to).is_ok(),
                expected,
                "{from:?} -> {to:?}"
            );
            assert_eq!(
                dispatch_job_sources(to).any(|source| source == from),
                expected,
                "{from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn secret_requests_and_queued_followups_resolve_once_from_their_open_state() {
    use ChatQueuedFollowupState::{Cancelled, Dispatched, Queued};
    use ChatSecretRequestState::{Declined, Expired, Pending, Provided};
    for from in ChatSecretRequestState::ALL {
        assert_eq!(ChatSecretRequestState::parse(from.as_str()), Some(from));
        for to in ChatSecretRequestState::ALL {
            let expected = from == Pending && matches!(to, Provided | Declined | Expired);
            assert_eq!(
                can_transition_secret_request(from, to),
                expected,
                "{from:?} -> {to:?}"
            );
        }
    }
    for from in ChatQueuedFollowupState::ALL {
        assert_eq!(ChatQueuedFollowupState::parse(from.as_str()), Some(from));
        for to in ChatQueuedFollowupState::ALL {
            let expected = from == Queued && matches!(to, Dispatched | Cancelled);
            assert_eq!(
                can_transition_queued_followup(from, to),
                expected,
                "{from:?} -> {to:?}"
            );
        }
    }
}
