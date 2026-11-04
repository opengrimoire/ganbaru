use super::*;

#[test]
fn permission_approvals_return_only_requested_subset_and_scope() {
    crate::test_block_on(async {
        let (client_reader, _server_writer) = tokio::io::duplex(4096);
        let (server_reader, client_writer) = tokio::io::duplex(4096);
        let connection = CodexRpcConnection::from_test_io(client_reader, client_writer);
        let pending_id = identifier("provider-request-1", ProviderRequestId::new);
        let requested = json!({
            "fileSystem": { "write": ["/redacted/workspace"] },
            "network": { "enabled": true }
        });
        let pending: PendingCodexRequests = Arc::new(Mutex::new(HashMap::from([(
            pending_id.clone(),
            PendingCodexRequest {
                rpc_id: json!(61),
                provider_request_id: pending_id.clone(),
                chat_turn_id: Some(identifier("chat-turn-1", ChatTurnId::new)),
                provider_turn_id: Some(identifier("provider-turn-1", ProviderTurnId::new)),
                provider_item_id: Some(identifier("provider-item-1", ProviderItemId::new)),
                kind: PendingCodexRequestKind::Approval {
                    allowed_provider_decisions: vec![
                        "accept".to_string(),
                        "acceptForSession".to_string(),
                        "decline".to_string(),
                        "cancel".to_string(),
                    ],
                    response: CodexApprovalResponse::Permissions {
                        requested: requested.clone(),
                    },
                },
            },
        )])));
        let normalizer = CodexEventNormalizer::new(
            identifier("codex-instance-1", ProviderInstanceId::new),
            identifier("chat-thread-1", ChatThreadId::new),
            identifier("session-1", ProviderSessionId::new),
        );
        let route = Arc::new(Mutex::new(CodexRouteState::new(
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            None,
        )));
        let sink = Arc::new(RecordingSink::default());
        let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
        let request: ResolveApprovalRequest = serde_json::from_value(json!({
            "command": { "clientCommandId": "approval-command-1", "expectedThreadRevision": 4 },
            "sessionId": "session-1",
            "requestId": "chat-request-1",
            "providerRequestId": "provider-request-1",
            "decision": {
                "kind": "allow_session",
                "providerOptionId": "acceptForSession",
                "updatedToolInput": null
            }
        }))
        .unwrap();
        resolve_codex_approval(
            &connection.client(),
            &pending,
            &normalizer,
            &route,
            &sink_trait,
            &request,
            &context("approve"),
        )
        .await
        .unwrap();
        let mut reader = BufReader::new(server_reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();

        assert_eq!(response["id"], 61);
        assert_eq!(response["result"]["scope"], "session");
        assert_eq!(response["result"]["permissions"], requested);
        assert!(matches!(
            sink.events().as_slice(),
            [CanonicalRuntimeEvent {
                event: CanonicalEvent::RequestResolved(_),
                ..
            }]
        ));
    });
}

#[test]
fn secret_structured_answers_are_sent_to_codex_but_not_canonicalized() {
    crate::test_block_on(async {
        let (client_reader, _server_writer) = tokio::io::duplex(4096);
        let (server_reader, client_writer) = tokio::io::duplex(4096);
        let connection = CodexRpcConnection::from_test_io(client_reader, client_writer);
        let pending_id = identifier("provider-request-secret", ProviderRequestId::new);
        let pending: PendingCodexRequests = Arc::new(Mutex::new(HashMap::from([(
            pending_id.clone(),
            PendingCodexRequest {
                rpc_id: json!(72),
                provider_request_id: pending_id,
                chat_turn_id: Some(identifier("chat-turn-1", ChatTurnId::new)),
                provider_turn_id: None,
                provider_item_id: None,
                kind: PendingCodexRequestKind::UserInput {
                    option_labels: HashMap::from([("token".to_string(), HashMap::new())]),
                    secret_question_ids: BTreeSet::from(["token".to_string()]),
                },
            },
        )])));
        let normalizer = CodexEventNormalizer::new(
            identifier("codex-instance-1", ProviderInstanceId::new),
            identifier("chat-thread-1", ChatThreadId::new),
            identifier("session-1", ProviderSessionId::new),
        );
        let route = Arc::new(Mutex::new(CodexRouteState::new(
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            None,
        )));
        let sink = Arc::new(RecordingSink::default());
        let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
        let request: ResolveUserInputRequest = serde_json::from_value(json!({
            "command": { "clientCommandId": "input-command-1", "expectedThreadRevision": 5 },
            "sessionId": "session-1",
            "requestId": "chat-request-secret",
            "providerRequestId": "provider-request-secret",
            "answers": [{
                "questionId": "token",
                "selectedOptionIds": [],
                "freeFormText": "ganbaru-sensitive-answer"
            }]
        }))
        .unwrap();
        resolve_codex_user_input(
            &connection.client(),
            &pending,
            &normalizer,
            &route,
            &sink_trait,
            &request,
        )
        .await
        .unwrap();
        let mut reader = BufReader::new(server_reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("ganbaru-sensitive-answer"));
        assert!(
            !serde_json::to_string(&sink.events())
                .unwrap()
                .contains("ganbaru-sensitive-answer")
        );
    });
}

#[test]
fn file_change_approvals_show_the_write_root_that_allow_for_session_grants() {
    let request = |grant_root: Value| {
        json!({ "threadId": "t", "turnId": "u", "itemId": "i", "grantRoot": grant_root })
            .as_object()
            .unwrap()
            .clone()
    };
    let method = "item/fileChange/requestApproval";

    let granted = approval_display(method, &request(json!("/home/user/elsewhere")));
    assert!(granted.allow_session);
    assert_eq!(
        granted
            .subject
            .as_ref()
            .map(|subject| (subject.kind, subject.text.as_str())),
        Some((ApprovalSubjectKind::Path, "/home/user/elsewhere"))
    );
    assert_eq!(
        granted.session_grants,
        vec![SessionPermissionGrant::Directory {
            path: "/home/user/elsewhere".to_string()
        }]
    );

    let plain = approval_display(method, &request(Value::Null));
    assert!(plain.allow_session && plain.subject.is_none() && plain.session_grants.is_empty());

    for unshowable in [json!(""), json!(["/"]), json!("/".repeat(4 * 1024 + 1))] {
        let display = approval_display(method, &request(unshowable));
        assert!(!display.allow_session);
        assert!(display.session_grants.is_empty());
    }
}

#[test]
fn command_approvals_show_the_command_and_its_working_directory() {
    let object = json!({
        "threadId": "t",
        "turnId": "u",
        "itemId": "i",
        "command": "rm -rf build",
        "cwd": "/srv/other-project"
    });
    let display = approval_display(
        "item/commandExecution/requestApproval",
        object.as_object().unwrap(),
    );
    let subject = display.subject.unwrap();
    assert_eq!(subject.kind, ApprovalSubjectKind::Command);
    assert_eq!(subject.text, "rm -rf build");
    assert_eq!(subject.location.as_deref(), Some("/srv/other-project"));
    assert!(!subject.truncated);
    assert!(display.allow_session && display.session_grants.is_empty());
}
