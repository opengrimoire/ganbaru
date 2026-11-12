use super::*;

async fn started(
    workspace: &Path,
    scenario: FixtureScenario,
) -> (
    CodexProviderDriver,
    Arc<AppServerFixtureState>,
    ProviderSessionSnapshot,
) {
    let (mut driver, fixture) = fixture_driver(workspace, scenario);
    let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
    let snapshot = driver
        .start_session(start_request(workspace), sink, &context("start"))
        .await
        .unwrap();
    (driver, fixture, snapshot)
}

fn rollback_request(session_id: &ProviderSessionId, cursor: Option<Value>) -> RollbackRequest {
    RollbackRequest {
        command: ChatCommandContext {
            client_command_id: identifier("rollback-command", ChatCommandId::new),
            expected_thread_revision: Some(4),
        },
        session_id: session_id.clone(),
        checkpoint_id: None,
        provider_cursor: cursor.map(|value| VersionedJson {
            schema_version: 1,
            value,
        }),
        target_turn_id: Some(identifier("chat-turn-kept", ChatTurnId::new)),
    }
}

fn sent(fixture: &AppServerFixtureState, method: &str) -> Vec<Value> {
    fixture
        .received()
        .into_iter()
        .filter(|message| message["method"] == method)
        .collect()
}

#[test]
fn rollback_reverts_before_the_turn_after_the_kept_turn_across_pages() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-pages");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;
        let thread_id = snapshot.provider_thread_id.clone().unwrap();

        let receipt = driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-3" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap();

        assert!(receipt.accepted);
        let pages = sent(&fixture, "thread/turns/list");
        assert_eq!(pages.len(), 2, "the kept turn opens the second page");
        assert_eq!(pages[0]["params"]["cursor"], Value::Null);
        assert_eq!(pages[1]["params"]["cursor"], "page-2");
        for page in &pages {
            assert_eq!(page["params"]["threadId"], thread_id.as_str());
            assert_eq!(page["params"]["sortDirection"], "desc");
            assert_eq!(page["params"]["itemsView"], "notLoaded");
        }
        let reverts = sent(&fixture, "thread/revert");
        assert_eq!(reverts.len(), 1);
        assert_eq!(
            reverts[0]["params"],
            json!({ "threadId": thread_id.as_str(), "beforeTurnId": "codex-turn-4" })
        );
    });
}

#[test]
fn rollback_to_the_newest_turn_leaves_the_thread_alone() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-latest");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;

        driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-5" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap();

        assert_eq!(sent(&fixture, "thread/turns/list").len(), 1);
        assert!(sent(&fixture, "thread/revert").is_empty());
    });
}

#[test]
fn rollback_to_the_oldest_turn_reverts_every_later_turn() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-oldest");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;

        driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-1" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap();

        assert_eq!(sent(&fixture, "thread/turns/list").len(), 3);
        assert_eq!(
            sent(&fixture, "thread/revert")[0]["params"]["beforeTurnId"],
            "codex-turn-2"
        );
    });
}

#[test]
fn rollback_to_a_turn_codex_no_longer_has_fails_without_reverting() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-missing");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;

        let error = driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-gone" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ChatErrorCode::NotFound);
        assert_eq!(sent(&fixture, "thread/turns/list").len(), 3);
        assert!(sent(&fixture, "thread/revert").is_empty());
    });
}

#[test]
fn rollback_stops_when_codex_repeats_a_turn_cursor() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-repeat");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::RepeatedTurnCursor).await;

        let error = driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-1" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ChatErrorCode::Protocol);
        assert_eq!(sent(&fixture, "thread/turns/list").len(), 2);
        assert!(sent(&fixture, "thread/revert").is_empty());
    });
}

#[test]
fn rollback_refuses_a_missing_or_invalid_cursor_before_any_request() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-cursor");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;

        for cursor in [
            None,
            Some(json!({ "method": "thread/started" })),
            Some(json!({ "method": "turn/completed", "turnId": "" })),
            Some(json!({ "method": "turn/completed", "turnId": "turn\nid" })),
        ] {
            let error = driver
                .rollback(
                    rollback_request(&snapshot.session_id, cursor),
                    &context("rollback"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation);
        }
        let mut stale_schema = rollback_request(
            &snapshot.session_id,
            Some(json!({ "method": "turn/completed", "turnId": "codex-turn-1" })),
        );
        stale_schema
            .provider_cursor
            .as_mut()
            .unwrap()
            .schema_version = 2;
        let error = driver
            .rollback(stale_schema, &context("rollback"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert!(sent(&fixture, "thread/turns/list").is_empty());
    });
}

#[test]
fn rollback_refuses_while_a_turn_is_running() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-active");
        let (mut driver, fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;
        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap();

        let error = driver
            .rollback(
                rollback_request(
                    &snapshot.session_id,
                    Some(json!({ "method": "turn/completed", "turnId": "codex-turn-3" })),
                ),
                &context("rollback"),
            )
            .await
            .unwrap_err();

        assert_eq!(error.code, ChatErrorCode::Busy);
        assert!(sent(&fixture, "thread/turns/list").is_empty());
    });
}

#[test]
fn codex_sessions_advertise_native_rollback() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("codex-rollback-capability");
        let (driver, _fixture, snapshot) =
            started(workspace.path(), FixtureScenario::Healthy).await;

        for capabilities in [&snapshot.capabilities, &driver.capabilities()] {
            assert!(capabilities.entries.iter().any(|entry| {
                entry.capability == ProviderCapability::NativeRollback && entry.supported
            }));
        }
    });
}
