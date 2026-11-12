//! Cursor scenario scripts.

use super::*;

/// Interrupts `turn-1`.
const INTERRUPT: ReplayStep = ReplayStep::Interrupt { turn: "turn-1" };

/// Waits until `count` turns ended interrupted in total.
const fn aborted(count: usize) -> ReplayStep {
    awaited("turn_aborted", count)
}

/// The script of the Cursor `scenario`, when one exists.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        "multi_turn" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            send("turn-2", "Say goodbye"),
            settled(2),
        ]),
        "turn_interrupt" => direct(vec![
            START,
            send("turn-1", "Count slowly"),
            awaited("content_delta", 1),
            INTERRUPT,
            aborted(1),
        ]),
        "turn_interrupt_mid_tool" => direct(vec![
            START,
            send("turn-1", "Edit the source"),
            awaited("item_started", 2),
            INTERRUPT,
            aborted(1),
        ]),
        // The driver refuses a second prompt while one runs; queueing belongs to the caller.
        "queued_turn" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            send("turn-2", "Say goodbye"),
            settled(1),
            send("turn-2", "Say goodbye"),
            settled(2),
        ]),
        "approval_request" => direct(vec![
            START,
            send("turn-1", "Run the tests"),
            awaited("request_opened", 1),
            ReplayStep::Resolve {
                decision: ApprovalDecisionKind::AllowOnce,
            },
            settled(1),
        ]),
        "exit_before_first_frame" => direct(vec![START, ReplayStep::AwaitTranscript]),
        // A frame that is not JSON closes the transport, so the turn ends interrupted.
        "malformed_frame" => direct(vec![START, send("turn-1", "Say hello"), aborted(1)]),
        _ => return None,
    })
}

replay_scenarios!(
    Cursor: simple,
    multi_turn,
    turn_interrupt,
    turn_interrupt_mid_tool,
    queued_turn,
    approval_request,
    exit_before_first_frame,
    malformed_frame,
);
