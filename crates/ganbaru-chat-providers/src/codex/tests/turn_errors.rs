use super::*;
use crate::events::TurnCompletedEvent;

/// Polls `condition` in virtual time, long enough to cover every capacity backoff.
async fn wait_until(condition: impl Fn() -> bool) {
    for _ in 0..2_000 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition was not reached");
}

fn turn_starts(fixture: &AppServerFixtureState) -> Vec<Value> {
    fixture
        .received()
        .into_iter()
        .filter(|message| message["method"] == "turn/start")
        .collect()
}

fn completed_turns(sink: &RecordingSink) -> Vec<TurnCompletedEvent> {
    sink.events()
        .into_iter()
        .filter_map(|event| match event.event {
            CanonicalEvent::TurnCompleted(turn) => Some(turn),
            _ => None,
        })
        .collect()
}

fn warnings(sink: &RecordingSink, code: &str) -> usize {
    sink.events()
        .into_iter()
        .filter(|event| {
            matches!(&event.event, CanonicalEvent::RuntimeWarning(warning) if warning.code == code)
        })
        .count()
}

async fn start_fixture_session(
    workspace: &Path,
    scenario: FixtureScenario,
    developer_instructions: Option<&str>,
) -> (
    CodexProviderDriver,
    Arc<AppServerFixtureState>,
    Arc<RecordingSink>,
    ProviderSessionSnapshot,
) {
    let (mut driver, fixture) = fixture_driver(workspace, scenario);
    let sink = Arc::new(RecordingSink::default());
    let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
    let mut request = start_request(workspace);
    request.developer_instructions = developer_instructions.map(str::to_string);
    let snapshot = driver
        .start_session(request, sink_trait, &context("start"))
        .await
        .unwrap();
    (driver, fixture, sink, snapshot)
}

#[test]
fn capacity_error_retries_three_times_then_fails() {
    crate::test_block_on(async {
        tokio::time::pause();
        let workspace = TestDirectory::new("driver-capacity");
        let (mut driver, fixture, sink, snapshot) =
            start_fixture_session(workspace.path(), FixtureScenario::CapacityFailure, None).await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();
        wait_until(|| !completed_turns(&sink).is_empty()).await;
        tokio::time::sleep(Duration::from_secs(30)).await;

        let starts = turn_starts(&fixture);
        assert_eq!(starts.len(), 4, "one turn and three retries");
        assert_ne!(starts[0]["params"]["input"], json!([]));
        for retry in &starts[1..] {
            assert_eq!(retry["params"]["input"], json!([]));
            assert_eq!(retry["params"]["threadId"], starts[0]["params"]["threadId"]);
        }
        assert_eq!(warnings(&sink, "codex_capacity_retry"), 3);
        let recoverable = sink
            .events()
            .into_iter()
            .filter_map(|event| match event.event {
                CanonicalEvent::RuntimeError(error) => Some(error.recoverable),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            recoverable,
            vec![true, true, true, false],
            "only the error the session will not retry ends the session"
        );
        let completed = completed_turns(&sink);
        assert_eq!(completed.len(), 1, "only the last failure settles the turn");
        assert_eq!(completed[0].state, ChatTurnState::Failed);
        assert_eq!(completed[0].stop_reason.as_deref(), Some("Fixture failure"));
        assert!(matches!(
            sink.events().last().map(|event| &event.event),
            Some(CanonicalEvent::TurnCompleted(_))
        ));
    });
}

#[test]
fn interrupt_during_capacity_backoff_settles_the_turn() {
    crate::test_block_on(async {
        tokio::time::pause();
        let workspace = TestDirectory::new("driver-capacity-interrupt");
        let (mut driver, fixture, sink, snapshot) =
            start_fixture_session(workspace.path(), FixtureScenario::CapacityFailure, None).await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();
        wait_until(|| warnings(&sink, "codex_capacity_retry") == 1).await;
        driver
            .interrupt_turn(
                InterruptTurnRequest {
                    command: ChatCommandContext {
                        client_command_id: identifier("interrupt-command", ChatCommandId::new),
                        expected_thread_revision: Some(3),
                    },
                    session_id: snapshot.session_id.clone(),
                    turn_id: identifier("chat-turn-fixture", ChatTurnId::new),
                },
                &context("interrupt"),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;

        assert_eq!(
            turn_starts(&fixture).len(),
            1,
            "the cancelled retry never starts"
        );
        assert!(
            !fixture
                .received()
                .iter()
                .any(|message| message["method"] == "turn/interrupt"),
            "no provider turn runs during the backoff"
        );
        let completed = completed_turns(&sink);
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].state, ChatTurnState::Interrupted);
    });
}

#[test]
fn interrupt_during_retry_request_stops_the_started_turn() {
    crate::test_block_on(async {
        tokio::time::pause();
        let workspace = TestDirectory::new("driver-capacity-in-flight");
        let (mut driver, fixture, sink, snapshot) = start_fixture_session(
            workspace.path(),
            FixtureScenario::CapacityRetryInFlight,
            None,
        )
        .await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();
        wait_until(|| turn_starts(&fixture).len() == 2).await;
        driver
            .interrupt_turn(
                InterruptTurnRequest {
                    command: ChatCommandContext {
                        client_command_id: identifier("interrupt-command", ChatCommandId::new),
                        expected_thread_revision: Some(3),
                    },
                    session_id: snapshot.session_id.clone(),
                    turn_id: identifier("chat-turn-fixture", ChatTurnId::new),
                },
                &context("interrupt"),
            )
            .await
            .unwrap();
        wait_until(|| {
            fixture
                .received()
                .iter()
                .any(|message| message["method"] == "turn/interrupt")
        })
        .await;
        tokio::time::sleep(Duration::from_secs(30)).await;

        let interrupts = fixture
            .received()
            .into_iter()
            .filter(|message| message["method"] == "turn/interrupt")
            .collect::<Vec<_>>();
        assert_eq!(interrupts.len(), 1);
        assert_eq!(interrupts[0]["params"]["turnId"], "fixture-provider-turn-2");
        let chat_terminals = sink
            .events()
            .into_iter()
            .filter(|event| {
                matches!(event.event, CanonicalEvent::TurnCompleted(_))
                    && event.turn_id.as_ref().map(ChatTurnId::as_str) == Some("chat-turn-fixture")
            })
            .collect::<Vec<_>>();
        assert_eq!(chat_terminals.len(), 1);
        assert!(matches!(
            &chat_terminals[0].event,
            CanonicalEvent::TurnCompleted(turn) if turn.state == ChatTurnState::Interrupted
        ));
    });
}

#[test]
fn usage_limit_never_retries() {
    crate::test_block_on(async {
        tokio::time::pause();
        let workspace = TestDirectory::new("driver-usage-limit");
        let (mut driver, fixture, sink, snapshot) =
            start_fixture_session(workspace.path(), FixtureScenario::UsageLimit, None).await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();
        wait_until(|| !completed_turns(&sink).is_empty()).await;
        tokio::time::sleep(Duration::from_secs(30)).await;

        assert_eq!(turn_starts(&fixture).len(), 1);
        assert_eq!(warnings(&sink, "codex_capacity_retry"), 0);
        let limited = sink
            .events()
            .into_iter()
            .find_map(|event| match event.event {
                CanonicalEvent::RateLimitStatus(status) => Some(status),
                _ => None,
            });
        let limited = limited.expect("a usage limit reports the rate limit status");
        assert!(limited.limited);
        assert_eq!(limited.detail.as_deref(), Some("Fixture failure"));
        assert_eq!(completed_turns(&sink)[0].state, ChatTurnState::Failed);
    });
}

#[test]
fn elicitation_request_is_declined() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-elicitation");
        let (mut driver, fixture, sink, snapshot) =
            start_fixture_session(workspace.path(), FixtureScenario::Elicitation, None).await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();
        let response =
            wait_for_fixture_message(&fixture, |message| message["id"] == "fixture-elicitation")
                .await;
        assert_eq!(response["result"], json!({ "action": "decline" }));
        let warning = wait_for_event(&sink, |event| {
            matches!(
                &event.event,
                CanonicalEvent::RuntimeWarning(warning)
                    if warning.code == "codex_elicitation_declined"
            )
        })
        .await;
        let CanonicalEvent::RuntimeWarning(warning) = warning.event else {
            unreachable!();
        };
        assert!(warning.detail.unwrap().contains("fixture-docs"));
        assert_eq!(
            warning_turn(&sink, "codex_elicitation_declined").as_deref(),
            Some("chat-turn-fixture")
        );
    });
}

fn warning_turn(sink: &RecordingSink, code: &str) -> Option<String> {
    sink.events()
        .into_iter()
        .find_map(|event| match &event.event {
            CanonicalEvent::RuntimeWarning(warning) if warning.code == code => {
                event.turn_id.as_ref().map(|turn| turn.as_str().to_string())
            }
            _ => None,
        })
}

#[test]
fn thread_open_sends_developer_instructions() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-open-instructions");
        let (mut driver, fixture, _sink, snapshot) = start_fixture_session(
            workspace.path(),
            FixtureScenario::Healthy,
            Some("Fixture instructions"),
        )
        .await;
        let open = fixture
            .received()
            .into_iter()
            .find(|message| message["method"] == "thread/start")
            .unwrap();
        assert_eq!(
            open["params"]["developerInstructions"],
            "Fixture instructions"
        );

        driver
            .send_turn(fixture_turn(&snapshot.session_id, "plan"), &context("send"))
            .await
            .unwrap();
        let turn =
            wait_for_fixture_message(&fixture, |message| message["method"] == "turn/start").await;
        let settings = &turn["params"]["collaborationMode"]["settings"];
        assert_eq!(turn["params"]["collaborationMode"]["mode"], "plan");
        assert!(
            settings.get("developer_instructions").is_none(),
            "the thread already carries the instructions"
        );
    });
}

#[test]
fn collaboration_copy_only_without_open_instructions() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-turn-instructions");
        let (mut driver, fixture, _sink, snapshot) =
            start_fixture_session(workspace.path(), FixtureScenario::Healthy, None).await;
        driver
            .send_turn(fixture_turn(&snapshot.session_id, "plan"), &context("send"))
            .await
            .unwrap();
        let turn =
            wait_for_fixture_message(&fixture, |message| message["method"] == "turn/start").await;
        assert_eq!(
            turn["params"]["collaborationMode"]["settings"]["developer_instructions"],
            "Fixture instructions"
        );
    });
}

#[test]
fn changed_turn_instructions_still_reach_the_turn() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-changed-instructions");
        let (mut driver, fixture, _sink, snapshot) = start_fixture_session(
            workspace.path(),
            FixtureScenario::Healthy,
            Some("Instructions at open"),
        )
        .await;
        driver
            .send_turn(fixture_turn(&snapshot.session_id, "plan"), &context("send"))
            .await
            .unwrap();
        let turn =
            wait_for_fixture_message(&fixture, |message| message["method"] == "turn/start").await;
        assert_eq!(
            turn["params"]["collaborationMode"]["settings"]["developer_instructions"],
            "Fixture instructions"
        );
    });
}
