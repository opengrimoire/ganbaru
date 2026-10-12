use super::value::*;
use super::*;

impl OpenCodeEventNormalizer {
    pub(super) fn permission_asked(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(properties, "id", "permission ID")?;
        state.pending_permissions.insert(id.to_string());
        let permission =
            text(properties, "permission").ok_or_else(|| protocol_error("permission kind"))?;
        let patterns = properties
            .get("patterns")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("permission patterns"))?;
        if patterns.len() > MAX_SAFE_COLLECTION_ITEMS
            || patterns.iter().any(|value| value.as_str().is_none())
        {
            return Err(protocol_error("permission patterns"));
        }
        let mut decisions = vec![
            ApprovalDecisionOption {
                id: "once".to_string(),
                label: "Allow once".to_string(),
                decision_kind: ApprovalDecisionKind::AllowOnce,
                description: None,
            },
            ApprovalDecisionOption {
                id: "reject".to_string(),
                label: "Deny".to_string(),
                decision_kind: ApprovalDecisionKind::Deny,
                description: None,
            },
        ];
        if properties
            .get("always")
            .and_then(Value::as_array)
            .is_some_and(|always| !always.is_empty())
        {
            decisions.insert(
                1,
                ApprovalDecisionOption {
                    id: "always".to_string(),
                    label: "Allow for session".to_string(),
                    decision_kind: ApprovalDecisionKind::AllowSession,
                    description: None,
                },
            );
        }
        let detail = patterns
            .iter()
            .filter_map(Value::as_str)
            .map(|value| bounded_text(value, 4096))
            .collect::<Vec<_>>()
            .join("\n");
        let request_id =
            ProviderRequestId::new(id.to_string()).map_err(|_| protocol_error("permission ID"))?;
        Ok(vec![self.event_with_request(
            state,
            "permission.asked",
            None,
            None,
            Some(request_id.clone()),
            CanonicalEvent::RequestOpened(RequestOpenedEvent {
                request_id,
                kind: request_kind(permission),
                title: format!("OpenCode requested {permission}"),
                detail: (!detail.is_empty()).then_some(detail),
                allowed_decisions: decisions,
                safe_payload: safe_shape(&Value::Object(properties.clone())),
            }),
        )?])
    }

    pub(super) fn permission_replied(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(properties, "requestID", "permission reply ID")?;
        state.pending_permissions.remove(id);
        let reply = text(properties, "reply").ok_or_else(|| protocol_error("permission reply"))?;
        let decision_kind = match reply {
            "once" => ApprovalDecisionKind::AllowOnce,
            "always" => ApprovalDecisionKind::AllowSession,
            "reject" => ApprovalDecisionKind::Deny,
            _ => return Err(protocol_error("permission reply")),
        };
        let request_id = ProviderRequestId::new(id.to_string())
            .map_err(|_| protocol_error("permission reply ID"))?;
        Ok(vec![self.event_with_request(
            state,
            "permission.replied",
            None,
            None,
            Some(request_id.clone()),
            CanonicalEvent::RequestResolved(RequestResolvedEvent {
                request_id,
                state: RequestResolutionState::Resolved,
                decision: Some(ApprovalDecision {
                    kind: decision_kind,
                    provider_option_id: Some(reply.to_string()),
                    updated_tool_input: None,
                }),
            }),
        )?])
    }

    pub(super) fn question_asked(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(properties, "id", "question request ID")?;
        let questions = properties
            .get("questions")
            .and_then(Value::as_array)
            .ok_or_else(|| protocol_error("questions"))?;
        if questions.is_empty() || questions.len() > MAX_QUESTIONS {
            return Err(protocol_error("questions"));
        }
        let mut normalized_questions = Vec::with_capacity(questions.len());
        let mut mappings = Vec::with_capacity(questions.len());
        for (index, question) in questions.iter().enumerate() {
            let question = question
                .as_object()
                .ok_or_else(|| protocol_error("question"))?;
            let prompt = text(question, "question")
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| protocol_error("question prompt"))?;
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .ok_or_else(|| protocol_error("question options"))?;
            if options.len() > MAX_OPTIONS {
                return Err(protocol_error("question options"));
            }
            let mut normalized_options = Vec::with_capacity(options.len());
            let mut option_labels = Vec::with_capacity(options.len());
            for (option_index, option) in options.iter().enumerate() {
                let option = option
                    .as_object()
                    .ok_or_else(|| protocol_error("question option"))?;
                let label = text(option, "label")
                    .filter(|label| !label.trim().is_empty())
                    .ok_or_else(|| protocol_error("question option label"))?;
                option_labels.push(bounded_text(label, 512));
                normalized_options.push(UserInputOption {
                    id: format!("option-{option_index}"),
                    label: bounded_text(label, 512),
                    description: text(option, "description")
                        .map(|description| bounded_text(description, 2048)),
                });
            }
            let question_id = format!("question-{index}");
            let free_form_allowed = question
                .get("custom")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            mappings.push(OpenCodeQuestionMapping {
                id: question_id.clone(),
                option_labels,
                free_form_allowed,
            });
            normalized_questions.push(UserInputQuestion {
                id: question_id,
                header: text(question, "header").map(|header| bounded_text(header, 256)),
                question: bounded_text(prompt, 4096),
                options: normalized_options,
                multiple: question
                    .get("multiple")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                free_form_allowed,
                required: true,
            });
        }
        let request_id = ProviderRequestId::new(id.to_string())
            .map_err(|_| protocol_error("question request ID"))?;
        state.pending_questions.insert(id.to_string(), mappings);
        Ok(vec![self.event_with_request(
            state,
            "question.asked",
            None,
            None,
            Some(request_id.clone()),
            CanonicalEvent::UserInputRequested(UserInputRequestedEvent {
                request_id,
                questions: normalized_questions,
            }),
        )?])
    }

    pub(super) fn question_replied(
        &self,
        state: &mut OpenCodeRouteState,
        properties: &Map<String, Value>,
        rejected: bool,
    ) -> ChatResult<Vec<CanonicalRuntimeEvent>> {
        let id = identifier(properties, "requestID", "question reply ID")?;
        state.pending_questions.remove(id);
        let answers = if rejected {
            Vec::new()
        } else {
            properties
                .get("answers")
                .and_then(Value::as_array)
                .ok_or_else(|| protocol_error("question answers"))?
                .iter()
                .enumerate()
                .map(|(index, answer)| {
                    let selected = answer
                        .as_array()
                        .ok_or_else(|| protocol_error("question answer"))?
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .map(|value| bounded_text(value, 512))
                                .ok_or_else(|| protocol_error("question answer value"))
                        })
                        .collect::<ChatResult<Vec<_>>>()?;
                    Ok(UserInputAnswer {
                        question_id: format!("question-{index}"),
                        selected_option_ids: selected,
                        free_form_text: None,
                    })
                })
                .collect::<ChatResult<Vec<_>>>()?
        };
        let request_id = ProviderRequestId::new(id.to_string())
            .map_err(|_| protocol_error("question reply ID"))?;
        Ok(vec![self.event_with_request(
            state,
            if rejected {
                "question.rejected"
            } else {
                "question.replied"
            },
            None,
            None,
            Some(request_id.clone()),
            CanonicalEvent::UserInputResolved(UserInputResolvedEvent {
                request_id,
                state: if rejected {
                    RequestResolutionState::Interrupted
                } else {
                    RequestResolutionState::Resolved
                },
                answers,
            }),
        )?])
    }
}
