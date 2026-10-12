use super::codec::*;
use super::*;

pub(in crate::repository) async fn apply_projection(
    transaction: &mut Transaction<'_, Sqlite>,
    sequence: u64,
    runtime: &CanonicalRuntimeEvent,
) -> ChatResult<Vec<String>> {
    let sequence = i64_value(sequence)?;
    let thread_id = runtime.thread_id.as_str();
    let mut changed = Vec::new();
    match &runtime.event {
        CanonicalEvent::SessionExited(_) => {
            if expire_open_requests_for_thread(transaction, runtime, "interrupted").await? {
                changed.push("pending_requests".to_string());
            }
        }
        CanonicalEvent::ThreadStarted(event) => {
            sqlx::query(
                "UPDATE chat_threads
                 SET provider_thread_id = ?, title = COALESCE(?, title), title_source = CASE
                     WHEN ? IS NULL THEN title_source ELSE 'provider' END
                 WHERE id = ?",
            )
            .bind(event.provider_thread_id.as_str())
            .bind(event.title.as_deref())
            .bind(event.title.as_deref())
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("thread".to_string());
        }
        CanonicalEvent::ThreadStateChanged(event) => {
            sqlx::query("UPDATE chat_threads SET state = ? WHERE id = ?")
                .bind(wire_literal(&event.state)?)
                .bind(thread_id)
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
            changed.push("thread".to_string());
        }
        CanonicalEvent::ThreadMetadataUpdated(event) => {
            let resume = versioned_parts(event.resume_cursor.as_ref())?;
            sqlx::query(
                "UPDATE chat_threads
                 SET title = COALESCE(?, title),
                     title_source = CASE WHEN ? IS NULL THEN title_source ELSE 'provider' END,
                     provider_thread_id = COALESCE(?, provider_thread_id),
                     resume_cursor_schema_version = COALESCE(?, resume_cursor_schema_version),
                     resume_cursor_data = COALESCE(?, resume_cursor_data)
                 WHERE id = ?",
            )
            .bind(event.title.as_deref())
            .bind(event.title.as_deref())
            .bind(
                event
                    .provider_thread_id
                    .as_ref()
                    .map(|value| value.as_str()),
            )
            .bind(resume.0)
            .bind(resume.1)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("thread".to_string());
        }
        CanonicalEvent::TurnStarted(event) => {
            let turn_id = required_turn_id(runtime)?;
            let ordinal: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM chat_turns WHERE thread_id = ?",
            )
            .bind(thread_id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            let model_selection = serde_json::to_string(&json!({
                "modelId": event.model_id,
                "options": event.model_options,
            }))
            .map_err(serialization_error)?;
            sqlx::query(
                "INSERT INTO chat_turns
                    (id, thread_id, ordinal, provider_turn_id, state, started_at,
                     model_selection_data, safety_mode, interaction_mode, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET
                    provider_turn_id = excluded.provider_turn_id,
                    state = excluded.state,
                    started_at = excluded.started_at,
                    model_selection_data = excluded.model_selection_data,
                    safety_mode = excluded.safety_mode,
                    interaction_mode = excluded.interaction_mode,
                    updated_at = excluded.updated_at",
            )
            .bind(turn_id)
            .bind(thread_id)
            .bind(ordinal)
            .bind(event.provider_turn_id.as_ref().map(|value| value.as_str()))
            .bind(wire_literal(&event.state)?)
            .bind(runtime.created_at.as_str())
            .bind(model_selection)
            .bind(wire_literal(&event.modes.safety_mode)?)
            .bind(wire_literal(&event.modes.interaction_mode)?)
            .bind(runtime.created_at.as_str())
            .bind(runtime.created_at.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            sqlx::query(
                "UPDATE chat_threads SET latest_turn_state = ?, state = 'active' WHERE id = ?",
            )
            .bind(wire_literal(&event.state)?)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.extend(["turns".to_string(), "thread".to_string()]);
        }
        CanonicalEvent::TurnCompleted(event) => {
            let turn_id = required_turn_id(runtime)?;
            let usage = event
                .usage
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(serialization_error)?;
            let current_files: Option<String> = sqlx::query_scalar(
                "SELECT changed_file_summary_data FROM chat_turns WHERE id = ? AND thread_id = ?",
            )
            .bind(turn_id)
            .bind(thread_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .flatten();
            let mut files = current_files
                .as_deref()
                .map(serde_json::from_str::<Vec<ChangedFileSummary>>)
                .transpose()
                .map_err(serialization_error)?
                .unwrap_or_default();
            merge_changed_file_summaries(&mut files, &event.changed_files);
            let files = serde_json::to_string(&files).map_err(serialization_error)?;
            let updated = sqlx::query(
                "UPDATE chat_turns
                 SET state = ?, completed_at = ?, stop_reason = ?,
                     usage_schema_version = CASE WHEN ? IS NULL THEN NULL ELSE 1 END,
                     usage_data = ?, changed_file_summary_schema_version = 1,
                     changed_file_summary_data = ?, updated_at = ?
                 WHERE id = ? AND thread_id = ?",
            )
            .bind(wire_literal(&event.state)?)
            .bind(runtime.created_at.as_str())
            .bind(event.stop_reason.as_deref())
            .bind(usage.as_deref())
            .bind(usage.as_deref())
            .bind(files)
            .bind(runtime.created_at.as_str())
            .bind(turn_id)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if updated.rows_affected() != 1 {
                return Err(ChatError::validation(
                    "event.turnId",
                    "Canonical turn completion references an unknown turn",
                ));
            }
            sqlx::query(
                "UPDATE chat_threads SET latest_turn_state = ?, state = 'idle' WHERE id = ?",
            )
            .bind(wire_literal(&event.state)?)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.extend(["turns".to_string(), "thread".to_string()]);
            if expire_open_requests_for_turn(transaction, runtime, "stale").await? {
                changed.push("pending_requests".to_string());
            }
        }
        CanonicalEvent::TurnAborted(event) => {
            let turn_id = required_turn_id(runtime)?;
            let updated = sqlx::query(
                "UPDATE chat_turns SET state = ?, completed_at = ?, stop_reason = ?, updated_at = ?
                 WHERE id = ? AND thread_id = ?",
            )
            .bind(wire_literal(&event.state)?)
            .bind(runtime.created_at.as_str())
            .bind(&event.reason)
            .bind(runtime.created_at.as_str())
            .bind(turn_id)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            if updated.rows_affected() != 1 {
                return Err(ChatError::validation(
                    "event.turnId",
                    "Canonical turn abort references an unknown turn",
                ));
            }
            sqlx::query(
                "UPDATE chat_threads SET latest_turn_state = ?, state = 'idle' WHERE id = ?",
            )
            .bind(wire_literal(&event.state)?)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.extend(["turns".to_string(), "thread".to_string()]);
            if expire_open_requests_for_turn(transaction, runtime, "interrupted").await? {
                changed.push("pending_requests".to_string());
            }
        }
        CanonicalEvent::DiffUpdated(event) => {
            let turn_id = required_turn_id(runtime)?;
            let current: Option<String> = sqlx::query_scalar(
                "SELECT changed_file_summary_data FROM chat_turns WHERE id = ? AND thread_id = ?",
            )
            .bind(turn_id)
            .bind(thread_id)
            .fetch_optional(&mut **transaction)
            .await
            .map_err(persistence_error)?
            .flatten();
            let mut files = current
                .as_deref()
                .map(serde_json::from_str::<Vec<ChangedFileSummary>>)
                .transpose()
                .map_err(serialization_error)?
                .unwrap_or_default();
            merge_changed_file_summaries(&mut files, &event.files);
            sqlx::query(
                "UPDATE chat_turns
                 SET changed_file_summary_schema_version = 1,
                     changed_file_summary_data = ?, updated_at = ?
                 WHERE id = ? AND thread_id = ?",
            )
            .bind(serde_json::to_string(&files).map_err(serialization_error)?)
            .bind(runtime.created_at.as_str())
            .bind(turn_id)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("turns".to_string());
        }
        CanonicalEvent::ContentDelta(event) => {
            let item_id = &event.item_id;
            let stream_kind = wire_literal(&event.stream_kind)?;
            if stream_kind == "assistant_text" {
                sqlx::query(
                    "INSERT INTO chat_messages
                        (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
                         streaming_state, provider_item_id, created_at, updated_at)
                     VALUES (?, ?, ?, ?, 'assistant', ?, 'streaming', ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET
                        normalized_markdown = chat_messages.normalized_markdown || excluded.normalized_markdown,
                        updated_at = excluded.updated_at",
                )
                .bind(item_id)
                .bind(thread_id)
                .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
                .bind(sequence)
                .bind(&event.delta)
                .bind(item_id)
                .bind(runtime.created_at.as_str())
                .bind(runtime.created_at.as_str())
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                let message_count: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM chat_messages WHERE thread_id = ?")
                        .bind(thread_id)
                        .fetch_one(&mut **transaction)
                        .await
                        .map_err(persistence_error)?;
                sqlx::query(
                    "UPDATE chat_threads
                     SET message_count = ?,
                         latest_preview = (
                             SELECT substr(normalized_markdown, -2000)
                             FROM chat_messages
                             WHERE id = ?
                         )
                     WHERE id = ?",
                )
                .bind(message_count)
                .bind(item_id)
                .bind(thread_id)
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                changed.extend(["messages".to_string(), "thread".to_string()]);
            } else {
                upsert_activity_detail(
                    transaction,
                    sequence,
                    runtime,
                    item_id,
                    &stream_kind,
                    &event.delta,
                )
                .await?;
                changed.push("activities".to_string());
            }
        }
        CanonicalEvent::ItemStarted(event)
        | CanonicalEvent::ItemUpdated(event)
        | CanonicalEvent::ItemCompleted(event) => {
            let metadata = event
                .safe_metadata
                .as_ref()
                .map(|value| serde_json::to_string(&value.value))
                .transpose()
                .map_err(serialization_error)?
                .unwrap_or_else(|| "{}".to_string());
            let item_kind = wire_literal(&event.kind)?;
            if item_kind == "assistant_message" {
                sqlx::query(
                    "INSERT INTO chat_messages
                        (id, thread_id, turn_id, sequence_anchor, role, normalized_markdown,
                         streaming_state, provider_item_id, content_metadata_data,
                         created_at, updated_at)
                     VALUES (?, ?, ?, ?, 'assistant', ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(id) DO UPDATE SET
                        normalized_markdown = CASE
                            WHEN excluded.normalized_markdown = '' THEN chat_messages.normalized_markdown
                            ELSE excluded.normalized_markdown
                        END,
                        streaming_state = excluded.streaming_state,
                        content_metadata_data = excluded.content_metadata_data,
                        updated_at = excluded.updated_at",
                )
                .bind(&event.item_id)
                .bind(thread_id)
                .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
                .bind(sequence)
                .bind(event.detail.as_deref().unwrap_or(""))
                .bind(message_streaming_state(&event.status)?)
                .bind(&event.item_id)
                .bind(&metadata)
                .bind(runtime.created_at.as_str())
                .bind(runtime.created_at.as_str())
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                sqlx::query("DELETE FROM chat_activities WHERE id = ? AND thread_id = ?")
                    .bind(&event.item_id)
                    .bind(thread_id)
                    .execute(&mut **transaction)
                    .await
                    .map_err(persistence_error)?;
                changed.push("messages".to_string());
            } else if item_kind != "user_message" {
                sqlx::query(
                    "INSERT INTO chat_activities
                    (id, thread_id, turn_id, sequence_anchor, item_kind, status, title,
                     detail, provider_item_id, safe_metadata_data, started_at,
                     source_event_type, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET status = excluded.status,
                    title = excluded.title,
                    detail = COALESCE(NULLIF(excluded.detail, ''), chat_activities.detail),
                    safe_metadata_data = excluded.safe_metadata_data,
                    updated_at = excluded.updated_at",
                )
                .bind(&event.item_id)
                .bind(thread_id)
                .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
                .bind(sequence)
                .bind(item_kind)
                .bind(wire_literal(&event.status)?)
                .bind(event.title.as_deref().unwrap_or(""))
                .bind(event.detail.as_deref())
                .bind(&event.item_id)
                .bind(metadata)
                .bind(runtime.created_at.as_str())
                .bind(event_type(&runtime.event)?)
                .bind(runtime.created_at.as_str())
                .bind(runtime.created_at.as_str())
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
                changed.push("activities".to_string());
            }
        }
        CanonicalEvent::RequestOpened(event) => {
            sqlx::query(
                "INSERT INTO chat_pending_requests
                    (id, thread_id, turn_id, provider_request_id, request_kind,
                     safe_display_schema_version, safe_display_data,
                     allowed_decisions_schema_version, allowed_decisions_data,
                     opened_sequence, opened_at)
                 VALUES (?, ?, ?, ?, 'approval', ?, ?, 1, ?, ?, ?)",
            )
            .bind(event.request_id.as_str())
            .bind(thread_id)
            .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
            .bind(event.request_id.as_str())
            .bind(1_i64)
            .bind(
                serde_json::to_string(&json!({
                    "title": event.title,
                    "detail": event.detail,
                    "kind": event.kind,
                    "payload": event.safe_payload,
                }))
                .map_err(serialization_error)?,
            )
            .bind(serde_json::to_string(&event.allowed_decisions).map_err(serialization_error)?)
            .bind(sequence)
            .bind(runtime.created_at.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("pending_requests".to_string());
        }
        CanonicalEvent::UserInputRequested(event) => {
            sqlx::query(
                "INSERT INTO chat_pending_requests
                    (id, thread_id, turn_id, provider_request_id, request_kind,
                     safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
                 VALUES (?, ?, ?, ?, 'user_input', ?, '[]', ?, ?)",
            )
            .bind(event.request_id.as_str())
            .bind(thread_id)
            .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
            .bind(event.request_id.as_str())
            .bind(serde_json::to_string(&event.questions).map_err(serialization_error)?)
            .bind(sequence)
            .bind(runtime.created_at.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("pending_requests".to_string());
        }
        CanonicalEvent::RequestResolved(event) => {
            resolve_request(
                transaction,
                runtime,
                event.request_id.as_str(),
                wire_literal(&event.state)?,
                &event.decision,
            )
            .await?;
            changed.push("pending_requests".to_string());
        }
        CanonicalEvent::UserInputResolved(event) => {
            resolve_request(
                transaction,
                runtime,
                event.request_id.as_str(),
                wire_literal(&event.state)?,
                &event.answers,
            )
            .await?;
            changed.push("pending_requests".to_string());
        }
        CanonicalEvent::ProposedPlanDelta(event) => {
            sqlx::query(
                "INSERT INTO chat_plans
                    (id, thread_id, origin_turn_id, sequence_anchor, markdown,
                     state, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, 'proposed', ?, ?)
                 ON CONFLICT(id) DO UPDATE SET markdown = chat_plans.markdown || excluded.markdown,
                    updated_at = excluded.updated_at",
            )
            .bind(&event.plan_id)
            .bind(thread_id)
            .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
            .bind(sequence)
            .bind(&event.delta)
            .bind(runtime.created_at.as_str())
            .bind(runtime.created_at.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("plans".to_string());
        }
        CanonicalEvent::ProposedPlanCompleted(event) => {
            sqlx::query(
                "UPDATE chat_plans SET markdown = ?, updated_at = ?
                 WHERE id = ? AND thread_id = ?",
            )
            .bind(&event.markdown)
            .bind(runtime.created_at.as_str())
            .bind(&event.plan_id)
            .bind(thread_id)
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.push("plans".to_string());
        }
        CanonicalEvent::ThreadReverted(event) => {
            if event.provider_history_action == "fork_required" {
                sqlx::query(
                    "UPDATE chat_threads
                     SET provider_thread_id = NULL, resume_cursor_schema_version = NULL,
                         resume_cursor_data = NULL
                     WHERE id = ?",
                )
                .bind(thread_id)
                .execute(&mut **transaction)
                .await
                .map_err(persistence_error)?;
            }
            sqlx::query(
                "INSERT INTO chat_activities
                    (id, thread_id, sequence_anchor, item_kind, status, title, detail,
                     safe_metadata_data, source_event_type, completed_at, created_at, updated_at)
                 VALUES (?, ?, ?, 'notice', 'completed', 'thread_reverted', NULL, ?,
                         'thread_reverted', ?, ?, ?)
                 ON CONFLICT(id) DO UPDATE SET
                    safe_metadata_data = excluded.safe_metadata_data,
                    updated_at = excluded.updated_at",
            )
            .bind(format!("activity:{}", runtime.event_id.as_str()))
            .bind(thread_id)
            .bind(sequence)
            .bind(
                serde_json::to_string(&json!({
                    "checkpointId": event.checkpoint_id,
                    "revertedTurnCount": event.reverted_turn_ids.len(),
                    "providerHistoryAction": event.provider_history_action,
                }))
                .map_err(serialization_error)?,
            )
            .bind(runtime.created_at.as_str())
            .bind(runtime.created_at.as_str())
            .bind(runtime.created_at.as_str())
            .execute(&mut **transaction)
            .await
            .map_err(persistence_error)?;
            changed.extend(["thread".to_string(), "activities".to_string()]);
        }
        _ => {}
    }
    Ok(changed)
}

async fn upsert_activity_detail(
    transaction: &mut Transaction<'_, Sqlite>,
    sequence: i64,
    runtime: &CanonicalRuntimeEvent,
    item_id: &str,
    item_kind: &str,
    delta: &str,
) -> ChatResult<()> {
    let detail = if item_kind == "reasoning_text" {
        None
    } else {
        Some(delta)
    };
    sqlx::query(
        "INSERT INTO chat_activities
            (id, thread_id, turn_id, sequence_anchor, item_kind, status, title,
             detail, provider_item_id, source_event_type, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'active', '', ?, ?, 'content_delta', ?, ?)
         ON CONFLICT(id) DO UPDATE SET
            item_kind = CASE
                WHEN excluded.item_kind = 'reasoning_summary' THEN excluded.item_kind
                ELSE chat_activities.item_kind
            END,
            detail = CASE
                WHEN excluded.item_kind = 'reasoning_text' THEN chat_activities.detail
                ELSE COALESCE(chat_activities.detail, '') || COALESCE(excluded.detail, '')
            END,
            updated_at = excluded.updated_at",
    )
    .bind(item_id)
    .bind(runtime.thread_id.as_str())
    .bind(runtime.turn_id.as_ref().map(|value| value.as_str()))
    .bind(sequence)
    .bind(item_kind)
    .bind(detail)
    .bind(item_id)
    .bind(runtime.created_at.as_str())
    .bind(runtime.created_at.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn message_streaming_state(status: &ActivityStatus) -> ChatResult<&'static str> {
    Ok(match wire_literal(status)?.as_str() {
        "completed" => "complete",
        "interrupted" => "interrupted",
        "failed" => "failed",
        "pending" => "pending",
        _ => "streaming",
    })
}

fn merge_changed_file_summaries(
    current: &mut Vec<ChangedFileSummary>,
    incoming: &[ChangedFileSummary],
) {
    for file in incoming {
        if let Some(existing) = current
            .iter_mut()
            .find(|candidate| candidate.relative_path == file.relative_path)
        {
            *existing = file.clone();
        } else {
            current.push(file.clone());
        }
    }
}

async fn resolve_request<T: Serialize>(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
    request_id: &str,
    state: String,
    resolution: &T,
) -> ChatResult<()> {
    let resolution = serde_json::to_string(resolution).map_err(serialization_error)?;
    let result = sqlx::query(
        "UPDATE chat_pending_requests
         SET resolution_state = ?, resolution_schema_version = 1,
             resolution_data = ?, resolved_at = ?
         WHERE thread_id = ? AND provider_request_id = ? AND resolution_state = 'open'",
    )
    .bind(state)
    .bind(resolution)
    .bind(runtime.created_at.as_str())
    .bind(runtime.thread_id.as_str())
    .bind(request_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if result.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Chat request is no longer open",
            true,
        ));
    }
    Ok(())
}

async fn expire_open_requests_for_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
    state: &str,
) -> ChatResult<bool> {
    let Some(turn_id) = runtime.turn_id.as_ref() else {
        return Ok(false);
    };
    let result = sqlx::query(
        "UPDATE chat_pending_requests
         SET resolution_state = ?, resolved_at = ?
         WHERE thread_id = ? AND turn_id = ? AND resolution_state = 'open'",
    )
    .bind(state)
    .bind(runtime.created_at.as_str())
    .bind(runtime.thread_id.as_str())
    .bind(turn_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(result.rows_affected() > 0)
}

async fn expire_open_requests_for_thread(
    transaction: &mut Transaction<'_, Sqlite>,
    runtime: &CanonicalRuntimeEvent,
    state: &str,
) -> ChatResult<bool> {
    let result = sqlx::query(
        "UPDATE chat_pending_requests
         SET resolution_state = ?, resolved_at = ?
         WHERE thread_id = ? AND resolution_state = 'open'",
    )
    .bind(state)
    .bind(runtime.created_at.as_str())
    .bind(runtime.thread_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(result.rows_affected() > 0)
}
