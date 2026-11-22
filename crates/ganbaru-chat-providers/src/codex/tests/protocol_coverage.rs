use super::super::normalizer::CODEX_NOTIFICATION_METHODS;
use super::*;
use crate::codex::replay::vendored_schema_methods;

/// Every Codex driver source file outside tests, fixtures, and the vendored schema.
fn driver_sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/codex");
    let mut pending = vec![root.clone()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if name != "tests" && name != "fixtures" && name != "compat" {
                    pending.push(path);
                }
            } else if name.ends_with(".rs") && name != "tests.rs" && name != "replay.rs" {
                let text = fs::read_to_string(&path).unwrap();
                sources.push((path, text));
            }
        }
    }
    sources
}

/// The first string literal shaped like a Codex method in `line`.
fn method_literal(line: &str) -> Option<&str> {
    line.split('"').skip(1).step_by(2).find(|literal| {
        let mut segments = literal.split('/');
        segments.next().is_some_and(|first| {
            first.starts_with(|character: char| character.is_ascii_lowercase())
                && first
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        }) && segments.all(|segment| {
            !segment.is_empty()
                && segment
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        })
    })
}

#[test]
fn every_dispatched_method_exists_in_schema() {
    let known = vendored_schema_methods();
    for method in CODEX_NOTIFICATION_METHODS
        .iter()
        .chain(CODEX_CLIENT_REQUEST_METHODS)
    {
        assert!(
            known.contains(*method),
            "{method} is not in the vendored Codex schema"
        );
    }
}

#[test]
fn every_sent_method_literal_is_listed() {
    const OPENERS: [&str; 3] = [".request(", ".notify(", "execute_thread_request("];
    const WINDOW: usize = 4;
    let mut checked = 0;
    for (path, text) in driver_sources() {
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if !OPENERS.iter().any(|opener| line.contains(opener)) {
                continue;
            }
            let Some(method) = lines[index..lines.len().min(index + WINDOW)]
                .iter()
                .find_map(|line| method_literal(line))
            else {
                continue;
            };
            assert!(
                CODEX_CLIENT_REQUEST_METHODS.contains(&method),
                "{} line {}: {method} is missing from CODEX_CLIENT_REQUEST_METHODS",
                path.display(),
                index + 1
            );
            checked += 1;
        }
    }
    assert!(checked >= 20, "the scan finds the driver's requests");
}

#[test]
fn every_listed_notification_has_a_handler() {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    for method in CODEX_NOTIFICATION_METHODS {
        let mut state = CodexRouteState::new(
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            Some(identifier("gpt-5.4", ModelId::new)),
        );
        let Ok(events) = normalizer.normalize_notification(&mut state, method, json!({})) else {
            continue;
        };
        assert!(
            !events.iter().any(|event| matches!(
                &event.event,
                CanonicalEvent::Unknown(unknown) if unknown.source_type == *method
            )),
            "{method} is listed but falls through to an unknown event"
        );
    }
    let mut state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        None,
    );
    let events = normalizer
        .normalize_notification(&mut state, "thread/realtime/item/started", json!({}))
        .unwrap();
    assert!(matches!(
        &events[0].event,
        CanonicalEvent::Unknown(unknown) if unknown.source_type == "thread/realtime/item/started"
    ));
}

#[test]
fn new_and_unknown_item_types_render_as_generic_items() {
    let normalizer = CodexEventNormalizer::new(
        identifier("codex-instance-1", ProviderInstanceId::new),
        identifier("chat-thread-1", ChatThreadId::new),
        identifier("session-1", ProviderSessionId::new),
    );
    let mut state = CodexRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        Some(identifier("gpt-5.4", ModelId::new)),
    );
    state
        .transition_turn(TurnRouteEvent::Begin {
            chat_turn: identifier("chat-turn-1", ChatTurnId::new),
            retry: None,
        })
        .unwrap();
    for (item_type, kind) in [
        ("imageGeneration", CanonicalItemKind::ImageView),
        ("functionCallOutput", CanonicalItemKind::Unknown),
        ("hookPrompt", CanonicalItemKind::Unknown),
        ("futureItemType", CanonicalItemKind::Unknown),
    ] {
        for method in ["item/started", "item/completed"] {
            let events = normalizer
                .normalize_notification(
                    &mut state,
                    method,
                    json!({
                        "threadId": "provider-thread-1",
                        "turnId": "provider-turn-1",
                        "item": { "id": format!("{item_type}-1"), "type": item_type }
                    }),
                )
                .unwrap();
            let lifecycle = events
                .iter()
                .find_map(|event| match &event.event {
                    CanonicalEvent::ItemStarted(item) | CanonicalEvent::ItemCompleted(item) => {
                        Some(item)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{item_type} {method} emits an item"));
            assert_eq!(lifecycle.kind, kind, "{item_type}");
            assert_eq!(lifecycle.item_id, format!("{item_type}-1"));
            assert!(lifecycle.title.is_some(), "{item_type} has a title");
        }
    }
}
