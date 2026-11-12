//! Generic ACP agent scenario scripts.

use super::*;

/// The script of the generic ACP `scenario`, when one exists.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" | "usage_update" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
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
        "bounded_notifications" => direct(vec![
            START,
            send("turn-1", "Stream a long answer"),
            settled(1),
        ]),
        "child_tool_output" => direct(vec![
            START,
            send("turn-1", "Delegate the tests"),
            settled(1),
        ]),
        "native_form_question" => direct(vec![START, send("turn-1", "Ask me first"), settled(1)]),
        // The agent answers the prompt after fifteen silent minutes on the paused clock.
        "silent_prompt" => direct(vec![
            START,
            send("turn-1", "Run the long migration"),
            ReplayStep::Elapse { seconds: 960 },
            settled(1),
        ]),
        // The terminal runs a real command, so the replay runs on the wall clock.
        "terminal_utf8" => real_time(vec![START, send("turn-1", "Print a snowman"), settled(1)]),
        _ => return None,
    })
}

replay_scenarios!(
    Acp: simple,
    multi_turn,
    turn_interrupt,
    approval_request,
    usage_update,
    malformed_frame,
    bounded_notifications,
    child_tool_output,
    native_form_question,
    silent_prompt,
    terminal_utf8,
);
