//! Calendar Save shares the Focus owner queue, transaction and publication fence.

use super::*;
use ganbaru_calendar::events::commit::{CommitFailure, CommitReceipt, CommitReply, CommitRequest};

const DELETE_UNDO_LIFETIME: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DismissUndoRequest {
    vault_id: String,
    vault_generation: u64,
    delete_command_id: String,
}

/// Dismiss only the named process-local opportunity; a delayed older toast
/// cannot discard a newer deletion or mutate durable data.
#[tauri::command]
pub(crate) async fn calendar_dismiss_delete_undo(
    app: tauri::AppHandle,
    request: DismissUndoRequest,
) -> Result<(), String> {
    ganbaru_calendar::events::commit::validate_command_id(&request.delete_command_id)?;
    if request.vault_id.is_empty()
        || request.vault_id.len() > 1_024
        || request.vault_id.chars().any(char::is_control)
        || request.vault_generation == 0
    {
        return Err("Invalid Calendar Undo dismissal scope".into());
    }
    super::request(
        &app,
        Request::CalendarDismissUndo {
            vault_id: request.vault_id,
            generation: request.vault_generation,
            delete_command_id: request.delete_command_id,
        },
    )
    .await
    .map(|_| ())
    .map_err(|error| error.message)
}

/// One generation-fenced, monotonic process-local opportunity. A prospective
/// slot survives an uncertain COMMIT so the receipt can confirm its acceptance.
pub(super) struct UndoSlot {
    vault_id: String,
    generation: u64,
    delete_command_id: String,
    delete_intent_hash: String,
    expires_at: Instant,
    started_wall_ms: i64,
    preimage: Arc<ganbaru_calendar::events::deletion::UndoPreimage>,
}

impl UndoSlot {
    pub(super) fn deadline(&self) -> Instant {
        self.expires_at
    }

    fn verify(&self, request: &CommitRequest, now: Instant) -> Result<(), String> {
        self.remaining(now, now_ms().ok())
            .ok_or("Calendar Undo has expired or its native clock is unavailable")?;
        verify_undo_scope(
            request,
            &self.vault_id,
            self.generation,
            &self.delete_command_id,
            &self.preimage.review_revision,
            self.expires_at,
            now,
        )
    }

    fn remaining(&self, now: Instant, wall_ms: Option<i64>) -> Option<Duration> {
        undo_remaining(self.expires_at, self.started_wall_ms, now, wall_ms?)
    }

    async fn prepare(
        &self,
        request: &CommitRequest,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    ) -> Result<ganbaru_calendar::events::commit::PreparedCommit, String> {
        self.verify(request, Instant::now())?;
        confirm_delete_receipt(
            tx,
            &self.delete_command_id,
            &self.delete_intent_hash,
            &self.preimage.review_revision,
        )
        .await?;
        self.preimage
            .clone()
            .prepare(tx, request.command_id())
            .await
    }
}

/// A civil clock can shorten this lease, but cannot extend it across sleep or
/// clock rollback when the platform's monotonic clock does not include suspend.
fn undo_remaining(
    expires_at: Instant,
    started_wall_ms: i64,
    now: Instant,
    wall_ms: i64,
) -> Option<Duration> {
    let elapsed = wall_ms.checked_sub(started_wall_ms)?;
    let elapsed = Duration::from_millis(u64::try_from(elapsed).ok()?);
    let wall_remaining = DELETE_UNDO_LIFETIME.checked_sub(elapsed)?;
    let remaining = expires_at.checked_duration_since(now)?.min(wall_remaining);
    (!remaining.is_zero()).then_some(remaining)
}

fn verify_undo_scope(
    request: &CommitRequest,
    vault_id: &str,
    generation: u64,
    delete_command_id: &str,
    review_revision: &str,
    expires_at: Instant,
    now: Instant,
) -> Result<(), String> {
    if now >= expires_at {
        return Err("Calendar Undo has expired".into());
    }
    if request.vault_id != vault_id
        || request.vault_generation != generation
        || request.undo_target() != Some(delete_command_id)
    {
        return Err("Calendar Undo belongs to a different operation or vault generation".into());
    }
    request.verify_undo_review(review_revision)
}

async fn confirm_delete_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command_id: &str,
    expected_hash: &str,
    review_revision: &str,
) -> Result<(), String> {
    let row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT
        CASE WHEN length(CAST(intent_hash AS BLOB))=64 THEN intent_hash END,
        CASE WHEN length(CAST(result_json AS BLOB))<=?2 THEN result_json END
        FROM calendar_edit_receipts WHERE command_id=?1",
    )
    .bind(command_id)
    .bind(ganbaru_calendar::events::commit::MAX_RECEIPT_BYTES as i64)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("confirm Calendar deletion before Undo: {error}"))?;
    let Some((Some(hash), Some(result))) = row else {
        return Err("Calendar deletion has no readable accepted receipt; its preimage cannot authorize Undo".into());
    };
    // Decode only the acceptance identity. Unrelated result vectors cannot
    // inflate allocations while checking this bounded persisted receipt.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct DeleteAcceptance {
        command_id: String,
        undo_review_revision: Option<String>,
    }
    let permit =
        receipt_worker_permit().map_err(|_| "Calendar deletion acceptance verification is busy")?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        serde_json::from_str::<DeleteAcceptance>(&result)
            .map_err(|error| format!("decode Calendar deletion acceptance before Undo: {error}"))
    });
    let receipt = tokio::time::timeout(
        ganbaru_calendar::events::scope::SCOPE_WORKER_TIMEOUT,
        worker,
    )
    .await
    .map_err(|_| "Calendar deletion acceptance verification timed out")?
    .map_err(|error| format!("Calendar deletion acceptance worker: {error}"))??;
    if hash != expected_hash
        || receipt.command_id != command_id
        || receipt.undo_review_revision.as_deref() != Some(review_revision)
    {
        return Err("Calendar deletion receipt belongs to a different preimage".into());
    }
    Ok(())
}

/// Capture the current owner and its effective clock, then let preview workers
/// proceed independently. Recurrence CPU work never runs in the execution queue.
pub(crate) async fn calendar_review_context(
    app: &tauri::AppHandle,
) -> Result<(String, u64, i64), String> {
    let projection = super::request(app, Request::CalendarReview)
        .await
        .map_err(|error| error.message)?
        .projection;
    let vault_id = projection
        .vault_id
        .ok_or("Calendar preview has no active vault")?;
    let clock = now_ms().map_err(|error| error.message)?.max(
        projection
            .snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.observed_at_ms),
    );
    Ok((vault_id, projection.vault_generation, clock))
}

/// Backpressure precedes queueing. Dropping IPC cannot cancel an accepted write;
/// its durable receipt resolves a retry after a lost response.
pub(crate) async fn commit_calendar_edit(
    app: &tauri::AppHandle,
    request: CommitRequest,
) -> Result<CommitReply, CommitFailure> {
    request.validate()?;
    let ownership = app
        .try_state::<crate::vault::ownership::VaultOwnershipManager>()
        .ok_or_else(|| CommitFailure::unknown("Calendar vault authorization is unavailable"))?;
    if ownership
        .cached_status(&request.vault_id)
        .map_err(CommitFailure::unknown)?
        .is_some_and(|status| !status.can_write)
    {
        return read_only_calendar_retry(app, &request)
            .await
            .map(|receipt| CommitReply {
                receipt,
                undo_available_for_ms: None,
            });
    }
    let state = app
        .try_state::<FocusRuntimeState>()
        .ok_or_else(|| CommitFailure::unknown("Native Focus owner is unavailable"))?;
    let (response, receive) = oneshot::channel();
    state
        .sender
        .try_send(Message {
            request: Request::CalendarEdit {
                request: Box::new(request),
                response,
            },
            response: None,
        })
        .map_err(|_| {
            CommitFailure::unknown("Native Focus command queue is busy; retry this Calendar save")
        })?;
    receive.await.map_err(|_| {
        CommitFailure::unknown("Native Calendar owner stopped before returning its receipt")
    })?
}

/// Confirm an existing result independently of execution or mutation authority.
async fn read_only_calendar_retry(
    app: &tauri::AppHandle,
    request: &CommitRequest,
) -> Result<CommitReceipt, CommitFailure> {
    let _transition =
        crate::vault::quiescence::reserve_vault_transition().map_err(CommitFailure::unknown)?;
    let expected = receipt_read_authority(app, &request.vault_id).await?;
    if expected.1 {
        return Err(CommitFailure::unknown(
            "Calendar write ownership changed; retry this Save",
        ));
    }
    let receipt = tokio::time::timeout(
        ganbaru_calendar::events::scope::SCOPE_WORKER_TIMEOUT,
        async {
            let permit = receipt_worker_permit()?;
            let pool = crate::db::connect_active_vault_read_only(app, permit)
                .await
                .map_err(CommitFailure::unknown)?;
            request.read_only_retry(&pool).await
        },
    )
    .await
    .map_err(|_| CommitFailure::unknown("Calendar receipt lookup timed out; retry this Save"))??;
    if receipt_read_authority(app, &request.vault_id).await? != expected {
        return Err(CommitFailure::unknown(
            "Calendar read authority changed during retry",
        ));
    }
    receipt
        .ok_or_else(|| "This vault is read-only and has no accepted receipt for this Save".into())
}

/// Filesystem identity and ledger refresh stay off the execution/runtime threads.
async fn receipt_read_authority(
    app: &tauri::AppHandle,
    expected_vault_id: &str,
) -> Result<(u64, bool), CommitFailure> {
    let app = app.clone();
    let expected_vault_id = expected_vault_id.to_owned();
    let permit = receipt_worker_permit()?;
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let vault_id = crate::vault::active_vault_id(&app).map_err(CommitFailure::unknown)?;
        if vault_id != expected_vault_id {
            return Err(CommitFailure::unknown(
                "Calendar receipt belongs to a different vault",
            ));
        }
        let status = app
            .state::<crate::vault::ownership::VaultOwnershipManager>()
            .status(&vault_id)
            .map_err(CommitFailure::unknown)?;
        Ok((status.generation, status.can_write))
    });
    tokio::time::timeout(
        ganbaru_calendar::events::scope::SCOPE_WORKER_TIMEOUT,
        worker,
    )
    .await
    .map_err(|_| CommitFailure::unknown("Calendar receipt authority timed out; retry this Save"))?
    .map_err(|error| {
        CommitFailure::unknown(format!("Calendar receipt authority worker: {error}"))
    })?
}

fn receipt_worker_permit() -> Result<tokio::sync::OwnedSemaphorePermit, CommitFailure> {
    ganbaru_calendar::events::scope::SCOPE_GATE
        .try_acquire()
        .map_err(|_| {
            CommitFailure::unknown("Calendar receipt verification is busy; retry this Save")
        })
}

impl Owner {
    pub(super) fn dismiss_calendar_undo(
        &mut self,
        vault_id: &str,
        generation: u64,
        command_id: &str,
    ) {
        if self.calendar_undo.as_ref().is_some_and(|slot| {
            dismissal_matches(
                &slot.vault_id,
                slot.generation,
                &slot.delete_command_id,
                vault_id,
                generation,
                command_id,
            )
        }) {
            self.calendar_undo = None;
        }
    }
    fn calendar_reply(&self, receipt: CommitReceipt) -> CommitReply {
        let undo_available_for_ms = self
            .calendar_undo
            .as_ref()
            .filter(|slot| {
                slot.delete_command_id == receipt.command_id
                    && slot.generation == self.projection.vault_generation
                    && self.projection.vault_id.as_deref() == Some(slot.vault_id.as_str())
                    && receipt.undo_review_revision.as_deref()
                        == Some(slot.preimage.review_revision.as_str())
                    && !self.frozen
                    && !self.freeze_requested()
            })
            .and_then(|slot| slot.remaining(Instant::now(), now_ms().ok()))
            .and_then(|remaining| u64::try_from(remaining.as_millis()).ok())
            .filter(|remaining| *remaining > 0);
        CommitReply {
            receipt,
            undo_available_for_ms,
        }
    }

    pub(super) fn prune_calendar_undo(&mut self) {
        if self.calendar_undo.as_ref().is_some_and(|slot| {
            slot.remaining(Instant::now(), now_ms().ok()).is_none()
                || self.projection.vault_id.as_deref() != Some(slot.vault_id.as_str())
                || self.projection.vault_generation != slot.generation
                || self.frozen
                || self.freeze_requested()
        }) {
            self.calendar_undo = None;
        }
    }

    pub(super) async fn calendar_edit(
        &mut self,
        request: CommitRequest,
    ) -> Result<CommitReply, CommitFailure> {
        let mut verified_uncommitted = false;
        self.calendar_edit_checked(request, &mut verified_uncommitted)
            .await
            .map_err(|error| {
                if verified_uncommitted {
                    error
                } else {
                    error.mark_uncertain()
                }
            })
    }

    async fn calendar_edit_checked(
        &mut self,
        request: CommitRequest,
        verified_uncommitted: &mut bool,
    ) -> Result<CommitReply, CommitFailure> {
        if self.frozen || self.freeze_requested() {
            return Err("Calendar saving is suspended while the vault changes".into());
        }
        self.ensure_context(false)
            .await
            .map_err(|error| error.message)?;
        self.verify_authority().map_err(|error| error.message)?;
        if self.projection.vault_id.as_deref() != Some(&request.vault_id) {
            return Err("Calendar save belongs to a different vault".into());
        }
        let _permit = self
            .app
            .state::<crate::vault::ownership::VaultOwnershipManager>()
            .acquire_managed_write(&request.vault_id)?;
        let pool = self
            .pool
            .as_ref()
            .cloned()
            .ok_or("Calendar vault is unavailable")?;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| format!("begin Calendar Save: {error}"))?;
        if let Some(receipt) = request
            .read_receipt(&mut tx)
            .await
            .map_err(CommitFailure::unknown)?
        {
            self.verify_authority()
                .map_err(|error| CommitFailure::unknown(error.message))?;
            tx.commit().await.map_err(|error| {
                CommitFailure::unknown(format!("finish Calendar retry: {error}"))
            })?;
            if request.undo_target().is_some_and(|target| {
                self.calendar_undo
                    .as_ref()
                    .is_some_and(|slot| slot.delete_command_id == target)
            }) {
                self.calendar_undo = None;
            }
            return Ok(self.calendar_reply(receipt));
        }
        // Only this authorized receipt lookup proves a failed retry did not
        // already commit. Earlier vault/queue failures leave its outcome unknown.
        *verified_uncommitted = true;
        if request.vault_generation != self.projection.vault_generation {
            return Err("Calendar save belongs to a previous vault generation".into());
        }
        if request.is_delete() {
            // Beginning a new deletion finalizes the previous opportunity.
            self.calendar_undo = None;
        }
        // Recovery can reconcile old execution. Finish preflight before recovery
        // opens its own transaction, then prepare against the recovered source.
        tx.rollback()
            .await
            .map_err(|error| format!("finish Calendar receipt preflight: {error}"))?;
        self.ensure_context(true)
            .await
            .map_err(|error| error.message)?;
        if request.vault_generation != self.projection.vault_generation || self.freeze_requested() {
            return Err("Calendar vault changed during recovery".into());
        }
        let now = self.logical_now(now_ms().map_err(|error| error.message)?);
        tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| format!("begin recovered Calendar Save: {error}"))?;
        let before = focus_read_execution_snapshot_tx(&mut tx, now)
            .await
            .map_err(|error| error.message)?;
        let prepared = if request.undo_target().is_some() {
            self.calendar_undo
                .as_ref()
                .ok_or("Calendar Undo is unavailable in this process")?
                .prepare(&request, &mut tx)
                .await?
        } else {
            request
                .prepare(
                    crate::calendar::device_date::source(&self.app),
                    &mut tx,
                    now,
                )
                .await?
        };
        let clock_fence = prepared.clock_fence;
        let completion_window = prepared.completion_window;
        let start_occurrence = prepared.start_occurrence.clone();
        let start_command = start_occurrence
            .as_ref()
            .map(|id| request.start_focus_command(id.clone(), &before))
            .transpose()?;
        // Explicit completion uses the acceptance clock for both durable domains.
        // Ordinary saves use a fresh clock so preparation cannot extend expiry.
        let now = if completion_window.is_some() {
            now
        } else {
            self.logical_now(now_ms().map_err(|error| error.message)?)
        };
        clock_fence
            .verify(crate::calendar::device_date::source(&self.app), now)
            .await?;
        let mut change = prepared.active_change(&before)?;
        let retarget_expiry = change
            .as_ref()
            .and_then(|_| before.run.as_ref().map(|run| run.planned_end_ms));
        if let Some(command) = prepared.stop_focus_command(&request, &before)? {
            let calendar = calendar::resolve(
                &mut tx,
                now,
                before.run.as_ref().map(|run| run.occurrence_id.as_str()),
            )
            .await
            .map_err(|error| error.message)?;
            let context = FocusExecutionContext {
                now_ms: now,
                platform: platform(),
                foreground: self.foreground,
                commitment: calendar.commitment,
                local_time: Some(Arc::new(local_time::NativeLocalTime(self.app.clone()))),
                planned_blocks: calendar
                    .planned_blocks
                    .into_iter()
                    .map(local_time::planned_block)
                    .collect(),
            };
            focus_execute_command_tx(&mut tx, &command, &context)
                .await
                .map_err(|error| error.message)?;
        }
        let (prepared, mut undo) = prepared.into_write();
        let receipt = prepared.write(&mut tx).await?;
        self.refresh_foreground();
        let (snapshot, next_boundary) = if receipt.changed {
            let calendar = calendar::resolve(
                &mut tx,
                now,
                start_occurrence.as_deref().or_else(|| {
                    change
                        .as_ref()
                        .map(|change| change.target_occurrence_id.as_str())
                }),
            )
            .await
            .map_err(|error| error.message)?;
            if completion_window.is_none()
                && let (Some(change), Some(commitment)) = (&mut change, &calendar.commitment)
            {
                change.target_event_date.clone_from(&commitment.event_date);
            }
            let context = FocusExecutionContext {
                now_ms: now,
                platform: platform(),
                foreground: self.foreground,
                commitment: calendar.commitment,
                local_time: Some(Arc::new(local_time::NativeLocalTime(self.app.clone()))),
                planned_blocks: calendar
                    .planned_blocks
                    .into_iter()
                    .map(local_time::planned_block)
                    .collect(),
            };
            let snapshot = if let Some(command) = start_command {
                focus_execute_command_tx(&mut tx, &command, &context).await
            } else if let Some(change) = change {
                if let Some((start_ms, end_ms)) = completion_window {
                    focus_complete_calendar_tx(
                        &mut tx,
                        before.revision,
                        &FocusCalendarCompletion {
                            reference: change,
                            start_ms,
                            end_ms,
                        },
                        &context,
                    )
                    .await
                } else {
                    focus_retarget_calendar_tx(&mut tx, before.revision, &change, &context).await
                }
            } else {
                focus_apply_observation_tx(&mut tx, &FocusObservation::CalendarChanged, &context)
                    .await
            }
            .map_err(|error| error.message)?;
            (snapshot, calendar.next_boundary_ms)
        } else {
            (before, self.next_boundary_ms)
        };
        request.record_receipt(&mut tx, &receipt, now).await?;
        if let Some(undo) = &mut undo {
            undo.seal(&mut tx).await?;
        }
        self.verify_authority().map_err(|error| error.message)?;
        if self.freeze_requested() {
            return Err("Calendar vault began freezing before commit".into());
        }
        let accepted_now = self.logical_now(now_ms().map_err(|error| error.message)?);
        if completion_window.is_none()
            && retarget_expiry.is_some_and(|deadline| accepted_now >= deadline)
        {
            return Err(
                "Focus expired during Calendar Save; refresh before changing its reference".into(),
            );
        }
        clock_fence
            .verify(
                crate::calendar::device_date::source(&self.app),
                accepted_now,
            )
            .await?;
        self.verify_authority().map_err(|error| error.message)?;
        if self.freeze_requested() {
            return Err("Calendar vault began freezing before commit".into());
        }
        if self.foreground
            != self
                .app
                .state::<FocusRuntimeState>()
                .foreground
                .borrow()
                .foreground
        {
            return Err("Calendar protection or application foreground changed before commit; retry after refresh".into());
        }
        if request.undo_target().is_some() {
            self.calendar_undo
                .as_ref()
                .ok_or("Calendar Undo expired during restoration")?
                .verify(&request, Instant::now())?;
        }
        if let Some(preimage) = undo {
            self.calendar_undo = Some(UndoSlot {
                vault_id: request.vault_id.clone(),
                generation: request.vault_generation,
                delete_command_id: request.command_id().to_owned(),
                delete_intent_hash: request.receipt_identity()?,
                expires_at: Instant::now() + DELETE_UNDO_LIFETIME,
                started_wall_ms: now_ms().map_err(|error| error.message)?,
                preimage: Arc::new(preimage),
            });
        }
        tx.commit()
            .await
            .map_err(|error| CommitFailure::unknown(format!("commit Calendar Save: {error}")))?;
        if request.undo_target().is_some() {
            self.calendar_undo = None;
        } else if request.is_delete()
            && let Some(slot) = &mut self.calendar_undo
        {
            // Start the normal opportunity after a confirmed commit. Receipt
            // retries never renew it; an uncertain commit keeps its earlier cap.
            slot.expires_at = Instant::now() + DELETE_UNDO_LIFETIME;
            if let Ok(wall_ms) = now_ms() {
                slot.started_wall_ms = wall_ms;
            } else {
                self.calendar_undo = None;
            }
        }
        self.next_boundary_ms = next_boundary;
        // The edit may create a current commitment; probe again until a heartbeat resolves it.
        self.calendar_commitment = true;
        if let Err(error) = self.publish_committed(snapshot, now).await {
            self.record_failure(error);
        }
        self.effects.invalidate_preferences();
        Ok(self.calendar_reply(receipt))
    }
}

fn dismissal_matches(
    slot_vault: &str,
    slot_generation: u64,
    slot_command: &str,
    vault: &str,
    generation: u64,
    command: &str,
) -> bool {
    slot_vault == vault && slot_generation == generation && slot_command == command
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_delayed_dismissal_cannot_revoke_a_newer_operation_or_vault_generation() {
        assert!(dismissal_matches(
            "vault", 2, "delete", "vault", 2, "delete"
        ));
        for (vault, generation, command) in [
            ("other", 2, "delete"),
            ("vault", 3, "delete"),
            ("vault", 2, "previous"),
        ] {
            assert!(!dismissal_matches(
                "vault", 2, "delete", vault, generation, command
            ));
        }
    }

    #[test]
    fn undo_cannot_gain_time_from_suspend_clock_rollback_or_delayed_replies() {
        let start = Instant::now();
        let deadline = start + DELETE_UNDO_LIFETIME;
        let wall_start = 100_000;
        assert_eq!(
            undo_remaining(
                deadline,
                wall_start,
                start + Duration::from_secs(2),
                wall_start + 1_000
            ),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            undo_remaining(
                deadline,
                wall_start,
                start + Duration::from_secs(1),
                wall_start + 3_000
            ),
            Some(Duration::from_secs(2))
        );
        assert!(undo_remaining(deadline, wall_start, start, wall_start + 5_000).is_none());
        assert!(undo_remaining(deadline, wall_start, start, wall_start - 1).is_none());
        assert!(undo_remaining(deadline, wall_start, deadline, wall_start).is_none());
    }

    #[test]
    fn undo_scope_expires_on_monotonic_time_and_rejects_other_vaults_generations_or_preimages() {
        let revision = "a".repeat(64);
        let start = Instant::now();
        let deadline = start + DELETE_UNDO_LIFETIME;
        let request_value = json!({ "vaultId":"vault", "vaultGeneration":3, "commandId":"undo",
            "reviewRevision":revision, "edit":{"kind":"undo_delete", "deleteCommandId":"delete"} });
        let request: CommitRequest = serde_json::from_value(request_value.clone()).unwrap();
        assert!(
            verify_undo_scope(
                &request,
                "vault",
                3,
                "delete",
                &revision,
                deadline,
                deadline - Duration::from_nanos(1)
            )
            .is_ok()
        );
        assert!(
            verify_undo_scope(
                &request, "vault", 3, "delete", &revision, deadline, deadline
            )
            .unwrap_err()
            .contains("expired")
        );
        for (field, value) in [
            ("vaultId", json!("other")),
            ("vaultGeneration", json!(4)),
            ("reviewRevision", json!("b".repeat(64))),
        ] {
            let mut changed = request_value.clone();
            changed[field] = value;
            let changed: CommitRequest = serde_json::from_value(changed).unwrap();
            assert!(
                verify_undo_scope(&changed, "vault", 3, "delete", &revision, deadline, start)
                    .is_err()
            );
        }
        let mut changed = request_value;
        changed["edit"]["deleteCommandId"] = json!("previous-delete");
        let changed: CommitRequest = serde_json::from_value(changed).unwrap();
        assert!(
            verify_undo_scope(&changed, "vault", 3, "delete", &revision, deadline, start).is_err()
        );
    }

    #[test]
    fn prospective_undo_requires_the_exact_readable_accepted_deletion_receipt() {
        tauri::async_runtime::block_on(async {
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            ganbaru_db::run_migrations(&pool).await.unwrap();
            let hash = "a".repeat(64);
            let revision = "b".repeat(64);
            let mut tx = pool.begin().await.unwrap();
            assert!(
                confirm_delete_receipt(&mut tx, "delete", &hash, &revision)
                    .await
                    .is_err()
            );
            sqlx::query("INSERT INTO calendar_edit_receipts VALUES ('delete', ?1, ?2, 1)")
                .bind(&hash)
                .bind(
                    json!({ "commandId":"delete", "editedId":"source", "changed":true,
                    "preservedIds":[], "undoReviewRevision":revision })
                    .to_string(),
                )
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(
                confirm_delete_receipt(&mut tx, "delete", &hash, &revision)
                    .await
                    .is_ok()
            );
            assert!(
                confirm_delete_receipt(&mut tx, "delete", &"c".repeat(64), &revision)
                    .await
                    .is_err()
            );
            assert!(
                confirm_delete_receipt(&mut tx, "delete", &hash, &"d".repeat(64))
                    .await
                    .is_err()
            );
            for malformed in ["{", "{}"] {
                sqlx::query("UPDATE calendar_edit_receipts SET result_json=?")
                    .bind(malformed)
                    .execute(&mut *tx)
                    .await
                    .unwrap();
                assert!(
                    confirm_delete_receipt(&mut tx, "delete", &hash, &revision)
                        .await
                        .is_err()
                );
            }
            tx.rollback().await.unwrap();
        });
    }
}
