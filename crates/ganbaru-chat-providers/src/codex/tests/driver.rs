use super::*;

#[test]
fn mcp_status_without_task_uses_standalone_app_server() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("mcp-status-draft");
        let (mut driver, fixture) = fixture_driver(workspace.path(), FixtureScenario::Healthy);

        let status = driver
            .read_mcp_status(
                McpStatusRequest {
                    session_id: None,
                    working_directory: Some(workspace.path().to_path_buf()),
                },
                &context("mcp-status-draft"),
            )
            .await
            .unwrap();

        assert_eq!(status.servers.len(), 1);
        assert_eq!(status.servers[0].name, "openaiDeveloperDocs");
        assert_eq!(
            status.servers[0].auth_status.as_deref(),
            Some("unsupported")
        );
        assert!(status.servers[0].enabled);
        assert_eq!(status.servers[0].runtime_status.as_deref(), Some("ready"));
        let received = fixture.received();
        let request = received
            .iter()
            .find(|message| message["method"] == "mcpServerStatus/list")
            .unwrap();
        assert!(request["params"]["threadId"].is_null());
        assert!(received.iter().all(|message| {
            !matches!(
                message["method"].as_str(),
                Some("thread/start" | "thread/resume")
            )
        }));
    });
}

#[test]
fn rejected_turn_start_releases_the_active_turn() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-turn-rejected");
        let (mut driver, _fixture) =
            fixture_driver(workspace.path(), FixtureScenario::TurnNotInProgress);
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(start_request(workspace.path()), sink, &context("start"))
            .await
            .unwrap();

        let error = driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("send"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Protocol);

        driver
            .compact_context(
                CompactContextRequest {
                    session_id: snapshot.session_id.clone(),
                    turn_id: identifier("chat-turn-compact", ChatTurnId::new),
                },
                &context("compact"),
            )
            .await
            .unwrap();
    });
}

#[test]
fn driver_fixture_covers_fresh_plan_interrupt_and_shutdown() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-fresh");
        let (mut driver, fixture) = fixture_driver(workspace.path(), FixtureScenario::Healthy);
        let sink = Arc::new(RecordingSink::default());
        let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink_trait,
                &context("start"),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot.provider_thread_id.as_ref().unwrap().as_str(),
            "fixture-thread-new"
        );
        assert_eq!(snapshot.state, ProviderSessionState::Ready);
        assert_eq!(
            snapshot.resume_cursor.as_ref().unwrap().value["threadId"],
            "fixture-thread-new"
        );

        let mcp_status = driver
            .read_mcp_status(
                McpStatusRequest {
                    session_id: Some(snapshot.session_id.clone()),
                    working_directory: None,
                },
                &context("mcp-status-live"),
            )
            .await
            .unwrap();
        assert_eq!(mcp_status.servers[0].name, "openaiDeveloperDocs");

        let receipt = driver
            .send_turn(fixture_turn(&snapshot.session_id, "plan"), &context("send"))
            .await
            .unwrap();
        assert_eq!(
            receipt.provider_turn_id.as_ref().unwrap().as_str(),
            "fixture-provider-turn"
        );
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
        wait_for_event(&sink, |event| {
            matches!(
                event.event,
                CanonicalEvent::TurnCompleted(ref turn)
                    if turn.state == ChatTurnState::Interrupted
            )
        })
        .await;
        driver
            .stop_session(
                StopSessionRequest {
                    session_id: snapshot.session_id,
                    force: false,
                },
                &context("stop"),
            )
            .await
            .unwrap();
        for _ in 0..100 {
            if fixture.closed.load(Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(fixture.closed.load(Ordering::Acquire));

        let received = fixture.received();
        assert!(
            received
                .iter()
                .any(|message| message["method"] == "thread/start")
        );
        let turn = received
            .iter()
            .find(|message| message["method"] == "turn/start")
            .unwrap();
        assert_eq!(turn["params"]["collaborationMode"]["mode"], "plan");
        assert_eq!(turn["params"]["approvalPolicy"], "on-request");
        assert!(
            received
                .iter()
                .any(|message| message["method"] == "turn/interrupt")
        );
        assert!(
            received
                .iter()
                .any(|message| message["method"] == "config/mcpServer/reload")
        );
        let mcp_request = received
            .iter()
            .find(|message| message["method"] == "mcpServerStatus/list")
            .unwrap();
        assert_eq!(mcp_request["params"]["threadId"], "fixture-thread-new");
    });
}

#[test]
fn organizational_fixture_verifies_permissions_and_mcp_before_dispatch() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-organizational");
        let (mut driver, fixture) =
            fixture_driver(workspace.path(), FixtureScenario::Organizational);
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink,
                &context("organizational-start"),
            )
            .await
            .unwrap();

        driver
            .send_turn(
                fixture_turn(&snapshot.session_id, "build"),
                &context("organizational-send"),
            )
            .await
            .unwrap();

        let received = fixture.received();
        let thread = received
            .iter()
            .find(|message| message["method"] == "thread/start")
            .unwrap();
        let turn = received
            .iter()
            .find(|message| message["method"] == "turn/start")
            .unwrap();
        for request in [thread, turn] {
            assert_eq!(
                request["params"]["permissions"],
                ORGANIZATIONAL_PERMISSION_PROFILE
            );
            assert_eq!(
                request["params"]["runtimeWorkspaceRoots"],
                json!([workspace.path()])
            );
            assert!(request["params"].get("sandbox").is_none());
            assert!(request["params"].get("sandboxPolicy").is_none());
        }
        let turn_position = received
            .iter()
            .position(|message| message["method"] == "turn/start")
            .unwrap();
        let mcp_status_positions = received
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (message["method"] == "mcpServerStatus/list").then_some(index)
            })
            .collect::<Vec<_>>();
        assert!(mcp_status_positions.len() >= 2);
        assert!(
            mcp_status_positions
                .iter()
                .any(|index| *index < turn_position)
        );
        let escalation_response = wait_for_fixture_message(&fixture, |message| {
            message["id"] == "fixture-organizational-escalation" && message.get("result").is_some()
        })
        .await;
        assert_eq!(escalation_response["result"]["decision"], "decline");

        driver
            .stop_session(
                StopSessionRequest {
                    session_id: snapshot.session_id,
                    force: true,
                },
                &context("organizational-stop"),
            )
            .await
            .unwrap();
    });
}

#[test]
fn driver_fixture_covers_native_resume_and_confirmed_missing_fallback() {
    crate::test_block_on(async {
        for (scenario, expected_thread, expects_fallback) in [
            (FixtureScenario::Healthy, "provider-thread-existing", false),
            (
                FixtureScenario::ResumeWithoutStarted,
                "provider-thread-existing",
                false,
            ),
            (FixtureScenario::ResumeMissing, "fixture-thread-new", true),
        ] {
            let workspace = TestDirectory::new("driver-resume");
            let (mut driver, fixture) = fixture_driver(workspace.path(), scenario);
            let direct_config = configuration(workspace.path(), None);
            let layout = resolve_codex_home_layout(
                &direct_config,
                &CodexProviderSettings::parse(&direct_config).unwrap(),
            )
            .unwrap();
            let sink = Arc::new(RecordingSink::default());
            let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
            let request: ResumeSessionRequest = serde_json::from_value(json!({
                "threadId": "chat-thread-fixture",
                "workspace": {
                    "workingFolderId": "workspace-fixture",
                    "canonicalPath": workspace.path(),
                    "repositoryKind": "none",
                    "repositoryIdentity": null
                },
                "providerInstanceId": "codex-instance-1",
                "providerThreadId": "provider-thread-existing",
                "continuationGroupId": layout.continuation_group_with_authority(false).unwrap(),
                "resumeCursor": {
                    "schemaVersion": 1,
                    "value": { "threadId": "provider-thread-existing" }
                },
                "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" }
            }))
            .unwrap();
            let snapshot = driver
                .resume_session(request, sink_trait, &context("resume"))
                .await
                .unwrap();
            assert_eq!(
                snapshot.provider_thread_id.as_ref().unwrap().as_str(),
                expected_thread
            );
            assert_eq!(
                sink.events().iter().any(|event| matches!(
                    &event.event,
                    CanonicalEvent::RuntimeWarning(warning)
                        if warning.code == "codex_resume_not_found_fresh_start"
                )),
                expects_fallback
            );
            let received = fixture.received();
            assert!(
                received
                    .iter()
                    .any(|message| message["method"] == "thread/resume")
            );
            assert_eq!(
                received
                    .iter()
                    .any(|message| message["method"] == "thread/start"),
                expects_fallback
            );
            driver
                .stop_session(
                    StopSessionRequest {
                        session_id: snapshot.session_id,
                        force: true,
                    },
                    &context("stop-resume"),
                )
                .await
                .unwrap();
        }
    });
}

#[test]
fn driver_fixture_routes_native_approval_and_structured_question() {
    crate::test_block_on(async {
        for scenario in [
            FixtureScenario::CommandApproval,
            FixtureScenario::StructuredQuestion,
        ] {
            let workspace = TestDirectory::new("driver-request");
            let (mut driver, fixture) = fixture_driver(workspace.path(), scenario);
            let sink = Arc::new(RecordingSink::default());
            let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
            let snapshot = driver
                .start_session(
                    start_request(workspace.path()),
                    sink_trait,
                    &context("start"),
                )
                .await
                .unwrap();
            driver
                .send_turn(
                    fixture_turn(&snapshot.session_id, "build"),
                    &context("send"),
                )
                .await
                .unwrap();

            match scenario {
                FixtureScenario::CommandApproval => {
                    let opened = wait_for_event(&sink, |event| {
                        matches!(event.event, CanonicalEvent::RequestOpened(_))
                    })
                    .await;
                    let provider_request_id = opened.provider_request_id.unwrap();
                    driver
                        .resolve_approval(
                            ResolveApprovalRequest {
                                command: ChatCommandContext {
                                    client_command_id: identifier(
                                        "approval-command",
                                        ChatCommandId::new,
                                    ),
                                    expected_thread_revision: Some(3),
                                },
                                session_id: snapshot.session_id.clone(),
                                request_id: identifier("chat-request", ChatRequestId::new),
                                provider_request_id,
                                decision: ApprovalDecision {
                                    kind: ApprovalDecisionKind::AllowOnce,
                                    provider_option_id: Some("accept".to_string()),
                                    updated_tool_input: None,
                                },
                            },
                            &context("approve"),
                        )
                        .await
                        .unwrap();
                    wait_for_event(&sink, |event| {
                        matches!(event.event, CanonicalEvent::RequestResolved(_))
                    })
                    .await;
                    let response = wait_for_fixture_message(&fixture, |message| {
                        message["id"] == "fixture-approval" && message.get("result").is_some()
                    })
                    .await;
                    assert_eq!(response["result"]["decision"], "accept");
                }
                FixtureScenario::StructuredQuestion => {
                    let opened = wait_for_event(&sink, |event| {
                        matches!(event.event, CanonicalEvent::UserInputRequested(_))
                    })
                    .await;
                    let provider_request_id = opened.provider_request_id.unwrap();
                    let CanonicalEvent::UserInputRequested(requested) = opened.event else {
                        unreachable!();
                    };
                    assert!(!requested.questions[0].multiple);
                    assert!(requested.questions[0].free_form_allowed);
                    driver
                        .resolve_user_input(
                            ResolveUserInputRequest {
                                command: ChatCommandContext {
                                    client_command_id: identifier(
                                        "question-command",
                                        ChatCommandId::new,
                                    ),
                                    expected_thread_revision: Some(3),
                                },
                                session_id: snapshot.session_id.clone(),
                                request_id: identifier("chat-question", ChatRequestId::new),
                                provider_request_id,
                                answers: vec![UserInputAnswer {
                                    question_id: "strategy".to_string(),
                                    selected_option_ids: vec!["q0-option-0".to_string()],
                                    free_form_text: None,
                                }],
                            },
                            &context("answer"),
                        )
                        .await
                        .unwrap();
                    wait_for_event(&sink, |event| {
                        matches!(event.event, CanonicalEvent::UserInputResolved(_))
                    })
                    .await;
                    let response = wait_for_fixture_message(&fixture, |message| {
                        message["id"] == "fixture-question" && message.get("result").is_some()
                    })
                    .await;
                    assert_eq!(
                        response["result"]["answers"]["strategy"]["answers"][0],
                        "Focused"
                    );
                }
                _ => unreachable!(),
            }
            driver
                .stop_session(
                    StopSessionRequest {
                        session_id: snapshot.session_id,
                        force: true,
                    },
                    &context("stop-request"),
                )
                .await
                .unwrap();
        }
    });
}

#[test]
fn driver_probe_distinguishes_authentication_and_protocol_failure() {
    crate::test_block_on(async {
        for (scenario, expected_state) in [
            (
                FixtureScenario::AuthenticationRequired,
                ProbeState::AuthenticationRequired,
            ),
            (
                FixtureScenario::MalformedInitialize,
                ProbeState::UnsupportedVersion,
            ),
        ] {
            let workspace = TestDirectory::new("driver-probe");
            let (mut driver, _fixture) = fixture_driver(workspace.path(), scenario);
            let probe = driver.probe(&context("probe")).await.unwrap();
            assert_eq!(probe.state, expected_state);
        }
    });
}

#[test]
fn healthy_probe_retains_the_discovered_model_catalog() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver-probe-models");
        let (mut driver, _fixture) = fixture_driver(workspace.path(), FixtureScenario::Healthy);

        let probe = driver.probe(&context("probe-models")).await.unwrap();
        let catalog = driver.cached_model_catalog().unwrap();

        assert_eq!(probe.state, ProbeState::Healthy);
        assert!(!catalog.models.is_empty());
        assert_eq!(catalog.instance_id, probe.instance_id);
    });
}

#[test]
fn process_environment_cannot_override_codex_home() {
    let shared = TestDirectory::new("environment-shared");
    let mut config = configuration(shared.path(), None);
    config.environment =
        BTreeMap::from([("CODEX_HOME".to_string(), "/untrusted/override".to_string())]);
    let layout = CodexHomeLayout {
        shared_home: shared.path().to_path_buf(),
        effective_home: shared.path().to_path_buf(),
        shadowed: false,
    };
    assert!(codex_process_environment(&config, &layout).is_err());
}

#[cfg(unix)]
#[test]
fn provider_path_falls_back_to_the_standard_pnpm_user_directory() {
    use std::os::unix::fs::PermissionsExt;

    let home = TestDirectory::new("provider-path-home");
    let bin = home.path().join(".local").join("share").join("pnpm");
    fs::create_dir_all(&bin).unwrap();
    let executable = bin.join("codex");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&executable, permissions).unwrap();
    let mut environment = BTreeMap::from([("PATH".to_string(), "/usr/bin".to_string())]);

    append_fallback_executable_directories(&mut environment, home.path());
    let resolved = resolve_codex_executable("codex", &environment).unwrap();

    assert_eq!(resolved.executable, executable);
    assert!(resolved.prefix_arguments.is_empty());
}
