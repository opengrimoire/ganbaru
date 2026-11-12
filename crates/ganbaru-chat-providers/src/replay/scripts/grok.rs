//! Grok scenario scripts.

use super::*;

/// The script of the Grok `scenario`, when one exists.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" | "usage_update" | "prompt_complete" => {
            direct(vec![START, send("turn-1", "Say hello"), settled(1)])
        }
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
            ReplayStep::Interrupt { turn: "turn-1" },
            awaited("turn_aborted", 1),
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
        // A frame that is not JSON closes the transport, so the turn ends interrupted.
        "malformed_frame" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            awaited("turn_aborted", 1),
        ]),
        _ => return None,
    })
}

replay_scenarios!(
    Grok: simple,
    multi_turn,
    turn_interrupt,
    approval_request,
    prompt_complete,
    usage_update,
    malformed_frame,
);
