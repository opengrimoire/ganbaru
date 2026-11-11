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
        _ => return None,
    })
}

replay_scenarios!(Codex: simple, multi_turn, approval_request);

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
