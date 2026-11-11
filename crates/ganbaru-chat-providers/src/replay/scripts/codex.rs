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
    use crate::replay::runner::ReplayProvider;
    use serde_json::Value;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every `method` enum value the schema declares under an object's `properties`.
    fn schema_methods(schema: &Value, methods: &mut BTreeSet<String>) {
        match schema {
            Value::Object(fields) => {
                if let Some(Value::Array(values)) = fields
                    .get("properties")
                    .and_then(|properties| properties.get("method"))
                    .and_then(|method| method.get("enum"))
                {
                    methods.extend(values.iter().filter_map(Value::as_str).map(str::to_string));
                }
                for value in fields.values() {
                    schema_methods(value, methods);
                }
            }
            Value::Array(items) => {
                for item in items {
                    schema_methods(item, methods);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn authored_codex_frames_exist_in_schema() {
        let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/codex/compat/codex_app_server_protocol.schemas.json");
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(&schema_path).unwrap()).unwrap();
        let mut known = BTreeSet::new();
        schema_methods(&schema, &mut known);
        assert!(
            known.contains("turn/start"),
            "the schema walk finds methods"
        );

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
