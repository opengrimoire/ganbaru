//! OpenCode scenario scripts.

use super::*;

/// The script of the OpenCode `scenario`, when one exists. Every OpenCode replay runs on the
/// wall clock, since the driver reaches the replay server over loopback sockets.
pub(super) fn script(scenario: &str) -> Option<ReplayScript> {
    Some(match scenario {
        "simple" => real_time(vec![START, send("turn-1", "Say hello"), settled(1)]),
        "multi_turn" => real_time(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            send("turn-2", "Say goodbye"),
            settled(2),
        ]),
        "turn_interrupt" => real_time(vec![
            START,
            send("turn-1", "Count slowly"),
            awaited("content_delta", 1),
            ReplayStep::Interrupt { turn: "turn-1" },
            awaited("turn_aborted", 1),
            ReplayStep::AwaitTranscript,
        ]),
        // A stop while a tool runs aborts the run at OpenCode before the session ends.
        "mid_run_stop" => real_time(vec![
            START,
            send("turn-1", "Run the long build"),
            awaited("item_started", 2),
            ReplayStep::Stop,
            ReplayStep::AwaitTranscript,
        ]),
        "approval_request" => real_time(vec![
            START,
            send("turn-1", "Run the tests"),
            awaited("request_opened", 1),
            ReplayStep::Resolve {
                decision: ApprovalDecisionKind::AllowOnce,
            },
            settled(1),
        ]),
        // The session grant answers the second matching request with `once` and never opens it.
        "session_approval" => real_time(vec![
            START,
            send("turn-1", "Fix both files"),
            awaited("request_opened", 1),
            ReplayStep::Resolve {
                decision: ApprovalDecisionKind::AllowSession,
            },
            settled(1),
        ]),
        "exit_before_first_frame" => real_time(vec![START]),
        // The stream goes quiet mid-turn, so the driver reads the history and subscribes again.
        "silent_stream" => stalled(
            1_000,
            vec![
                START,
                send("turn-1", "Think quietly"),
                settled(1),
                ReplayStep::AwaitTranscript,
            ],
        ),
        // A subscription that keeps going quiet ends the session after the bounded reconnects.
        "stalled_subscription" => stalled(
            300,
            vec![
                START,
                send("turn-1", "Keep working"),
                awaited("runtime_error", 1),
                ReplayStep::AwaitTranscript,
            ],
        ),
        // Heartbeats keep a slash command turn alive past the busy bound while its request runs.
        "long_command_turn" => stalled(
            500,
            vec![
                START,
                send("turn-1", "/review src/main.rs"),
                settled(1),
                ReplayStep::AwaitTranscript,
            ],
        ),
        "deleting_edits" => real_time(vec![
            START,
            send("turn-1", "Remove the old module"),
            settled(1),
        ]),
        // The usage part read again from the paged history is not counted twice.
        "bounded_usage" => real_time(vec![
            START,
            send("turn-1", "Summarize"),
            settled(1),
            ReplayStep::AwaitTranscript,
        ]),
        // A question OpenCode asks with no turn running is shown as a notice and rejected.
        "self_woken_question" => real_time(vec![
            START,
            send("turn-1", "Say hello"),
            settled(1),
            awaited("runtime_warning", 1),
            ReplayStep::AwaitTranscript,
        ]),
        _ => return None,
    })
}

replay_scenarios!(OpenCode:
    simple,
    multi_turn,
    turn_interrupt,
    mid_run_stop,
    approval_request,
    session_approval,
    exit_before_first_frame,
    silent_stream,
    stalled_subscription,
    long_command_turn,
    self_woken_question,
);

/// A wall-clock script whose busy event stream counts as silent after `timeout_ms`.
fn stalled(timeout_ms: u64, steps: Vec<ReplayStep>) -> ReplayScript {
    ReplayScript {
        busy_stream_timeout: Some(std::time::Duration::from_millis(timeout_ms)),
        ..real_time(steps)
    }
}

/// Rows whose fixes live in fields the snapshot leaves out, so their full events are checked too.
#[cfg(test)]
mod event_fields {
    use crate::events::CanonicalEvent;
    use crate::replay::runner::{ReplayProvider, ReplayReport, run_scenario};

    fn replay(scenario: &str) -> ReplayReport {
        run_scenario(ReplayProvider::OpenCode, scenario)
            .unwrap_or_else(|failure| panic!("{failure}"))
    }

    /// A deleted file keeps its status and removed lines, so a deleting edit is not shown as an
    /// empty change.
    #[test]
    fn deleting_edits() {
        let report = replay("deleting_edits");
        let files = report
            .events
            .iter()
            .find_map(|event| match &event.event {
                CanonicalEvent::DiffUpdated(diff) => Some(&diff.files),
                _ => None,
            })
            .expect("the diff should be reported");
        let deleted = files
            .iter()
            .find(|file| file.relative_path == "src/old.rs")
            .expect("the deleted file should be listed");
        assert_eq!(deleted.status, "deleted");
        assert_eq!(deleted.deletions, Some(12));
        assert_eq!(deleted.additions, Some(0));
        let modified = files
            .iter()
            .find(|file| file.relative_path == "src/main.rs")
            .expect("the edited file should be listed");
        assert_eq!(modified.status, "modified");
    }

    /// The usage of one step is reported once with its exact values, even when the paged history
    /// read after a dropped stream carries the same step again.
    #[test]
    fn bounded_usage() {
        let report = replay("bounded_usage");
        let usage = report
            .events
            .iter()
            .filter_map(|event| match &event.event {
                CanonicalEvent::ThreadUsageUpdated(usage) => Some(usage),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].input_tokens, Some(100));
        assert_eq!(usage[0].output_tokens, Some(50));
        assert_eq!(usage[0].cached_input_tokens, Some(20));
        assert_eq!(usage[0].context_tokens, Some(150));
        let cost = usage[0].cost.as_ref().expect("the cost should be reported");
        assert_eq!(cost.amount, 0.0125);
        assert_eq!(cost.currency, "USD");
    }
}
