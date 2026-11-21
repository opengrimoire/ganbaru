//! Codex scenario scripts.

use super::*;

/// The script of the Codex `scenario`, when one exists.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        "multi_turn" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            send("turn-2", "Now say goodbye"),
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
        "turn_interrupt" => direct(vec![
            START,
            send("turn-1", "Count slowly"),
            awaited("content_delta", 1),
            ReplayStep::Interrupt { turn: "turn-1" },
            settled(1),
        ]),
        "turn_interrupt_mid_tool" => direct(vec![
            START,
            send("turn-1", "Run the tests"),
            awaited("item_started", 1),
            ReplayStep::Interrupt { turn: "turn-1" },
            settled(1),
        ]),
        // A turn sent while one runs is refused as busy, and goes through once the first settles.
        "queued_turn" => direct(vec![
            START,
            send("turn-1", "List the files"),
            awaited("content_delta", 1),
            send("turn-2", "Read it"),
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
        ]),
        "compaction" => direct(vec![
            START,
            send("turn-1", "Summarize the project"),
            settled(1),
        ]),
        // The limit reset survives a later sparse update that leaves the exhausted window out.
        "usage_limit" => direct(vec![START, send("turn-1", "Keep going"), settled(1)]),
        // A CLI that exits before its first frame fails the start with a typed error.
        "exit_before_first_frame" => direct(vec![START]),
        "malformed_frame" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        // An MCP elicitation is declined without asking, and the turn runs on.
        "elicitation_decline" => direct(vec![START, send("turn-1", "Check the docs"), settled(1)]),
        // An overloaded model is retried on the same turn after a backoff.
        "capacity_retry" => direct(vec![START, send("turn-1", "Say hello"), settled(1)]),
        // A checkpoint restore to the first turn reverts the native thread before the second.
        "rollback" => direct(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            send("turn-2", "Now say goodbye"),
            settled(2),
            ReplayStep::Rollback {
                provider_turn: "provider-turn-1",
            },
            ReplayStep::AwaitTranscript,
        ]),
        // A tool that starts while reasoning streams shows before the reasoning completes.
        "reasoning_then_tool" => direct(vec![START, send("turn-1", "Fix the test"), settled(1)]),
        _ => return None,
    })
}

replay_scenarios!(
    Codex: simple,
    multi_turn,
    approval_request,
    turn_interrupt,
    turn_interrupt_mid_tool,
    queued_turn,
    message_steering,
    compaction,
    usage_limit,
    exit_before_first_frame,
    malformed_frame,
    elicitation_decline,
    capacity_retry,
    rollback,
    reasoning_then_tool,
);

#[cfg(test)]
mod schema {
    use crate::codex::replay::vendored_schema_methods;
    use crate::replay::runner::ReplayProvider;
    use serde_json::Value;

    #[test]
    fn authored_codex_frames_exist_in_schema() {
        let known = vendored_schema_methods();
        let mut checked = 0;
        let directory = ReplayProvider::Codex.scenario_directory();
        for entry in std::fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
                continue;
            }
            for (index, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                let entry: Value = serde_json::from_str(line).unwrap();
                let Some(method) = entry
                    .get("frame")
                    .and_then(|frame| frame.get("method"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                assert!(
                    known.contains(method),
                    "{} line {}: method {method} is not in the vendored schema",
                    path.display(),
                    index + 1
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "the Codex scenarios carry methods");
    }
}
