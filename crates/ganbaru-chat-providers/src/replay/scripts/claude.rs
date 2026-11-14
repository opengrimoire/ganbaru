//! Claude scenario scripts.

use super::*;

/// Waits until `count` turns aborted in total.
const fn aborted(count: usize) -> ReplayStep {
    awaited("turn_aborted", count)
}

/// Sends a Plan turn under `safety`.
const fn plan(turn: &'static str, prompt: &'static str, safety: SafetyMode) -> ReplayStep {
    ReplayStep::Send {
        turn,
        prompt,
        safety,
        interaction: InteractionMode::Plan,
    }
}

const ALLOW: ReplayStep = ReplayStep::Resolve {
    decision: ApprovalDecisionKind::AllowOnce,
};

/// The script of the Claude `scenario`, when one exists.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        "multi_turn" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            send("turn-2", "Say goodbye"),
            settled(2),
            ReplayStep::Stop,
        ]),
        "turn_interrupt" => direct(vec![
            START,
            send("turn-1", "Count slowly"),
            awaited("content_delta", 1),
            ReplayStep::Interrupt { turn: "turn-1" },
            aborted(1),
        ]),
        "turn_interrupt_mid_tool" => direct(vec![
            START,
            send("turn-1", "Run the tests"),
            awaited("item_started", 1),
            ReplayStep::Interrupt { turn: "turn-1" },
            aborted(1),
        ]),
        // A turn sent while one runs is refused as busy, and goes through once the first settles.
        "queued_turn" => direct(vec![
            START,
            send("turn-1", "List the files"),
            awaited("request_opened", 1),
            send("turn-2", "Read it"),
            ALLOW,
            settled(1),
            send("turn-2", "Read it"),
            settled(2),
        ]),
        "message_steering" => direct(vec![
            START,
            send("turn-1", "Write a poem"),
            awaited("content_delta", 1),
            ReplayStep::Steer {
                turn: "turn-1",
                prompt: "Make it short",
            },
            settled(1),
            awaited("thread_usage_updated", 1),
        ]),
        "approval_request" => direct(vec![
            START,
            send("turn-1", "Show the date"),
            awaited("request_opened", 1),
            ALLOW,
            settled(1),
        ]),
        "approval_withdrawn" => direct(vec![
            START,
            send("turn-1", "Show the date"),
            awaited("request_resolved", 1),
            settled(1),
        ]),
        // Steering while the CLI compacts leaves the turn running to completion.
        "compaction" => direct(vec![
            START,
            send("turn-1", "Summarize the project"),
            awaited("thread_metadata_updated", 1),
            ReplayStep::Steer {
                turn: "turn-1",
                prompt: "Focus on the tests",
            },
            settled(1),
        ]),
        "usage_limit" => direct(vec![START, send("turn-1", "Keep going"), aborted(1)]),
        // A CLI that exits before its first frame fails the start with a typed error.
        "exit_before_first_frame" => direct(vec![START]),
        "malformed_frame" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        "result_error" => direct(vec![START, send("turn-1", "Say hello"), aborted(1)]),
        // Plan turns ask before writes whatever the safety mode.
        "plan_mode" => direct(vec![
            START,
            plan("turn-1", "Plan the change", SafetyMode::FullAccess),
            settled(1),
            plan("turn-2", "Plan the tests", SafetyMode::ApproveForMe),
            settled(2),
        ]),
        // A result that absorbed a background task's wake settles the turn, so the next one runs.
        "background_task" => direct(vec![
            START,
            send("turn-1", "Watch the build"),
            settled(1),
            send("turn-2", "Thanks"),
            settled(2),
        ]),
        "organizational_launch" => {
            organizational(vec![START, send("turn-1", "Review the notes"), settled(1)])
        }
        // The approval a subagent asked for is answered with that subagent's request id.
        "subagent_stream" => direct(vec![
            START,
            send("turn-1", "Delegate the check"),
            awaited("request_opened", 1),
            ALLOW,
            settled(1),
        ]),
        _ => return None,
    })
}

replay_scenarios!(
    Claude: simple,
    multi_turn,
    turn_interrupt,
    turn_interrupt_mid_tool,
    queued_turn,
    message_steering,
    approval_request,
    approval_withdrawn,
    compaction,
    usage_limit,
    exit_before_first_frame,
    malformed_frame,
    result_error,
    plan_mode,
    background_task,
    organizational_launch,
    subagent_stream,
);
