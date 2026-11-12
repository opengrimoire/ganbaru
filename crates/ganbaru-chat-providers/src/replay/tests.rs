//! Replay harness tests: the transcript loader, matching rules, failures, and every scenario.

use super::diff::json_diff;
use super::runner::{ReplayProvider, replay};
use super::scripts::{ReplayScript, ReplayStep};
use super::transcript::*;
use crate::models::{InteractionMode, SafetyMode};
use serde_json::{Value, json};

const MASK: ReplayMask = ReplayMask {
    request_id: "/id",
    presence_only: &["/id", "/items/*/stamp"],
};

fn header(scenario: &str) -> String {
    json!({
        "type": "transcript_start", "provider": "claude", "protocol": "stream-json",
        "cli_version": "2.1.322", "scenario": scenario, "generated_by": "authored"
    })
    .to_string()
}

fn transcript(entries: &[Value]) -> String {
    let mut lines = vec![header("unit")];
    lines.extend(entries.iter().map(Value::to_string));
    lines.join("\n")
}

fn load_error(entries: &[Value]) -> ReplayError {
    load_transcript(&transcript(entries)).unwrap_err()
}

#[test]
fn the_loader_keeps_line_numbers_and_skips_blank_lines() {
    let text = format!(
        "{}\n\n{}\n",
        header("unit"),
        json!({ "type": "expect_outbound", "label": "init", "frame": { "a": 1 } })
    );
    let transcript = load_transcript(&text).unwrap();
    assert_eq!(transcript.header.scenario, "unit");
    assert_eq!(transcript.header.launch_arguments, None);
    assert_eq!(transcript.lines.len(), 1);
    assert_eq!(transcript.lines[0].line, 3);
}

#[test]
fn the_loader_rejects_malformed_and_out_of_order_entries() {
    let expect = json!({ "type": "expect_outbound", "frame": {} });
    assert_eq!(
        load_transcript(&expect.to_string()).unwrap_err(),
        ReplayError::Malformed {
            line: 1,
            reason: "the first entry must be transcript_start".to_string()
        }
    );
    assert!(matches!(
        load_error(&[json!({ "type": "expect_outbound", "frame": {}, "extra": true })]),
        ReplayError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        load_error(&[json!({ "type": "emit_later", "frame": {} })]),
        ReplayError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        load_error(&[
            json!({ "type": "runtime_exit", "status": 0 }),
            expect.clone()
        ]),
        ReplayError::Malformed { line: 3, .. }
    ));
    let mut doubled = transcript(&[]);
    doubled.push('\n');
    doubled.push_str(&header("unit"));
    assert!(matches!(
        load_transcript(&doubled).unwrap_err(),
        ReplayError::Malformed { line: 2, .. }
    ));
    assert!(matches!(
        load_transcript("").unwrap_err(),
        ReplayError::Malformed { line: 1, .. }
    ));
}

#[test]
fn the_loader_enforces_the_byte_and_entry_bounds() {
    let oversized = format!("{}\n{}", header("unit"), " ".repeat(MAX_TRANSCRIPT_BYTES));
    assert_eq!(
        load_transcript(&oversized).unwrap_err(),
        ReplayError::TooLarge
    );
    let entry = json!({ "type": "emit_inbound", "frame": {} });
    let at_limit = vec![entry.clone(); MAX_TRANSCRIPT_ENTRIES - 1];
    assert!(load_transcript(&transcript(&at_limit)).is_ok());
    let over_limit = vec![entry; MAX_TRANSCRIPT_ENTRIES];
    assert_eq!(
        load_transcript(&transcript(&over_limit)).unwrap_err(),
        ReplayError::TooLarge
    );
}

#[test]
fn the_loader_rejects_unknown_placeholders_and_forward_references() {
    assert_eq!(
        load_error(&[json!({ "type": "emit_inbound", "frame": { "path": "<cache>/x" } })]),
        ReplayError::UnknownPlaceholder {
            line: 2,
            placeholder: "<cache>".to_string()
        }
    );
    assert_eq!(
        load_error(&[json!({ "type": "emit_inbound", "frame": { "id": "<ref:later>" } })]),
        ReplayError::UnknownPlaceholder {
            line: 2,
            placeholder: "<ref:later>".to_string()
        }
    );
    let accepted = load_transcript(&transcript(&[
        json!({ "type": "expect_outbound", "label": "ask", "frame": { "cwd": "<workspace>" } }),
        json!({ "type": "emit_inbound", "frame": {
            "id": "<ref:ask>", "text": "a < b and <c d> and <Upper>", "who": "<redacted>"
        } }),
    ]));
    assert!(accepted.is_ok(), "{accepted:?}");
}

#[test]
fn masked_pointers_compare_by_presence_and_wildcards_match_any_index() {
    let mut bindings = ReplayBindings::new("/work", None);
    let expected = json!({ "id": 0, "items": [{ "stamp": "x", "text": "a" }] });
    let actual = json!({ "id": 41, "items": [{ "stamp": "2026-11-11", "text": "a" }] });
    assert_eq!(
        match_frame(&expected, &actual, &MASK, &mut bindings),
        Ok(())
    );

    let missing = json!({ "items": [{ "stamp": "x", "text": "a" }] });
    assert_eq!(
        match_frame(&expected, &missing, &MASK, &mut bindings),
        Err("/id: a missing field".to_string())
    );
    let extra = json!({ "id": 1, "items": [{ "stamp": "x", "text": "a" }], "more": true });
    assert_eq!(
        match_frame(&expected, &extra, &MASK, &mut bindings),
        Err("/more: an unexpected field".to_string())
    );
    let changed = json!({ "id": 1, "items": [{ "stamp": "x", "text": "b" }] });
    assert_eq!(
        match_frame(&expected, &changed, &MASK, &mut bindings),
        Err("/items/0/text: a different value".to_string())
    );
}

#[test]
fn placeholders_bind_the_workspace_session_and_redacted_values() {
    let mut bindings = ReplayBindings::new("/work", None);
    let expected = json!({ "cwd": "<workspace>/src", "session": "<session>", "who": "<redacted>" });
    let actual = json!({ "cwd": "/work/src", "session": "abc-123", "who": "someone" });
    assert_eq!(
        match_frame(&expected, &actual, &MASK, &mut bindings),
        Ok(())
    );
    assert_eq!(bindings.session.as_deref(), Some("abc-123"));

    let other_session = json!({ "cwd": "/work/src", "session": "def-456", "who": "x" });
    assert_eq!(
        match_frame(&expected, &other_session, &MASK, &mut bindings),
        Err("/session: a different value".to_string()),
        "the first frame binds the session for the rest of the replay"
    );
    assert_eq!(
        bindings.resolve(&json!({ "resume": "<session>", "path": "<workspace>" })),
        Ok(json!({ "resume": "abc-123", "path": "/work" }))
    );
}

#[test]
fn the_home_placeholder_resolves_only_when_a_provider_home_is_bound() {
    let bindings = ReplayBindings::new("/work", None).with_home("/replay/home");
    assert_eq!(
        bindings.resolve(&json!({ "codexHome": "<home>", "log": "<home>/log" })),
        Ok(json!({ "codexHome": "/replay/home", "log": "/replay/home/log" }))
    );
    assert!(
        ReplayBindings::new("/work", None)
            .resolve(&json!("<home>"))
            .is_err(),
        "an unbound home never resolves to a guessed path"
    );
}

#[test]
fn references_quote_the_id_of_the_labeled_outbound_frame() {
    let mut bindings = ReplayBindings::new("/work", None);
    bindings.remember_reference("ask", &json!({ "id": 7, "method": "x" }), &MASK);
    bindings.remember_reference("named", &json!({ "id": "req-3" }), &MASK);
    assert_eq!(
        bindings.resolve(&json!({ "id": "<ref:ask>", "note": "answer to <ref:named>" })),
        Ok(json!({ "id": 7, "note": "answer to req-3" })),
        "a whole-string reference keeps the numeric id type"
    );
    assert_eq!(
        ReplayBindings::new("/work", None).resolve(&json!("<session>")),
        Ok(json!(REPLAY_SESSION)),
        "an unbound session resolves to the replay session"
    );
}

#[test]
fn a_failing_replay_names_the_line_label_and_diff() {
    let text = transcript(&[json!({
        "type": "expect_outbound", "label": "initialize",
        "frame": { "type": "control_request", "request_id": "x", "request": { "subtype": "nothing", "supportedDialogKinds": [] } }
    })]);
    let failure = replay(
        ReplayProvider::Claude,
        load_transcript(&text).unwrap(),
        &ReplayScript {
            organizational: false,
            steps: vec![ReplayStep::Start {
                safety: SafetyMode::AskForApproval,
                interaction: InteractionMode::Build,
            }],
        },
    )
    .unwrap_err();
    assert_eq!(failure.line, Some(2));
    assert_eq!(failure.label.as_deref(), Some("initialize"));
    let rendered = failure.to_string();
    assert!(rendered.starts_with("transcript line 2 (initialize): outbound frame differs at "));
    assert!(
        rendered.contains("-     \"subtype\": \"nothing\""),
        "{rendered}"
    );
    assert!(
        rendered.contains("+     \"subtype\": \"initialize\""),
        "{rendered}"
    );
}

#[test]
fn unconsumed_expectations_fail_the_replay() {
    let text = transcript(&[json!({
        "type": "expect_outbound", "label": "never", "frame": { "type": "user" }
    })]);
    let failure = replay(
        ReplayProvider::Claude,
        load_transcript(&text).unwrap(),
        &ReplayScript {
            organizational: false,
            steps: Vec::new(),
        },
    )
    .unwrap_err();
    assert_eq!(failure.line, Some(2));
    assert_eq!(failure.label.as_deref(), Some("never"));
}

#[test]
fn unexpected_outbound_frames_fail_the_replay() {
    let failure = replay(
        ReplayProvider::Claude,
        load_transcript(&transcript(&[])).unwrap(),
        &ReplayScript {
            organizational: false,
            steps: vec![ReplayStep::Start {
                safety: SafetyMode::AskForApproval,
                interaction: InteractionMode::Build,
            }],
        },
    )
    .unwrap_err();
    assert_eq!(failure.line, None);
    assert!(
        failure.message.contains("after the transcript ended"),
        "{failure}"
    );
    assert_eq!(
        failure
            .actual
            .as_ref()
            .and_then(|frame| frame.pointer("/request/subtype")),
        Some(&json!("initialize"))
    );
}

#[test]
fn the_diff_marks_expected_and_actual_lines() {
    let diff = json_diff(&json!({ "a": 1, "b": 2 }), &json!({ "a": 1, "b": 3 }));
    assert_eq!(
        diff,
        "--- expected\n+++ actual\n  {\n    \"a\": 1,\n+   \"b\": 3\n-   \"b\": 2\n  }\n"
    );
}

#[test]
fn line_and_paragraph_separators_round_trip_through_every_jsonl_reader() {
    let frame = json!({ "type": "assistant", "text": "one\u{2028}two\u{2029}three" });
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains('\u{2028}'),
        "serde_json writes both separators unescaped"
    );
    bytes.push(b'\n');
    bytes.extend_from_slice(&bytes.clone());
    let readers = crate::test_block_on(async {
        [
            crate::claude::replay::read_jsonl_frames(&bytes).await,
            crate::codex::replay::read_jsonl_frames(&bytes).await,
            crate::cursor::replay::read_jsonl_frames(&bytes).await,
        ]
    });
    for frames in readers {
        assert_eq!(frames, vec![frame.clone(), frame.clone()]);
    }
}
