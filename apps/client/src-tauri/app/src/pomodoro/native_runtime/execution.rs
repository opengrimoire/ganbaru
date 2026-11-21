//! Transactional execution of Focus commands and observations, and publication of each committed
//! snapshot to subscribers, native controls, and effect authority.

use super::*;

impl Owner {
    pub(super) async fn execute(
        &mut self,
        command: &FocusCommand,
        observed_now: Result<i64, FocusExecutionError>,
        expected_generation: Option<u64>,
    ) -> Result<FocusExecutionSnapshot, FocusExecutionError> {
        self.verify_authority()?;
        self.refresh_foreground();
        self.projection
            .vault_id
            .as_deref()
            .ok_or_else(|| error(FocusErrorCode::Unavailable, "Focus has no active vault"))?;
        let _permit = self
            .app
            .state::<crate::vault::write_fence::VaultWriteFence>()
            .acquire_managed_write()?;
        let pool = self
            .pool
            .as_ref()
            .cloned()
            .ok_or_else(|| error(FocusErrorCode::Unavailable, "Focus vault is unavailable"))?;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| error.to_string())?;
        if let Some(receipt) = focus_read_command_receipt_tx(&mut tx, command).await? {
            let current = match &observed_now {
                Ok(now) => focus_read_execution_snapshot_tx(&mut tx, *now).await,
                Err(error) => Err(error.clone()),
            };
            self.verify_authority()?;
            tx.commit()
                .await
                .map_err(|error| format!("Finish native Focus retry: {error}"))?;
            self.refresh_retry_projection(current).await;
            return Ok(receipt);
        }
        if expected_generation
            .is_some_and(|generation| generation != self.projection.vault_generation)
        {
            return Err(error(
                FocusErrorCode::StaleGeneration,
                "Focus action belongs to a previous vault generation",
            ));
        }
        let mut now = observed_now?;
        let recovered = self
            .recovered_vault
            .as_ref()
            .is_some_and(|(id, generation)| {
                self.projection.vault_id.as_ref() == Some(id)
                    && self.projection.vault_generation == *generation
            });
        if !recovered {
            let generation = self.projection.vault_generation;
            tx.rollback()
                .await
                .map_err(|error| format!("Finish Focus receipt preflight: {error}"))?;
            self.ensure_context(true).await?;
            if generation != self.projection.vault_generation {
                return Err(error(
                    FocusErrorCode::StaleGeneration,
                    "Focus vault changed during recovery",
                ));
            }
            now = self.logical_now(now_ms()?);
            tx = pool
                .begin_with("BEGIN IMMEDIATE")
                .await
                .map_err(|error| format!("Begin recovered Focus action: {error}"))?;
        }
        let requested = match &command.intent {
            FocusIntent::StartScheduled { occurrence_id } => occurrence_id.as_deref(),
            _ => None,
        };
        let calendar = calendar::resolve(&mut tx, now, requested).await?;
        let calendar_commitment = calendar.commitment.is_some();
        let context = FocusExecutionContext {
            now_ms: now,
            platform: platform(),
            foreground: self.foreground,
            device_id: Some(self.device_id()?),
            commitment: calendar.commitment,
            local_time: Some(Arc::new(local_time::NativeLocalTime(self.app.clone()))),
            planned_blocks: calendar
                .planned_blocks
                .into_iter()
                .map(local_time::planned_block)
                .collect(),
        };
        let snapshot = focus_execute_command_tx(&mut tx, command, &context).await?;
        self.verify_authority()?;
        tx.commit()
            .await
            .map_err(|error| format!("Commit native Focus action: {error}"))?;
        self.next_boundary_ms = calendar.next_boundary_ms;
        self.calendar_commitment = calendar_commitment;
        self.publish_committed(snapshot.clone(), now).await?;
        Ok(snapshot)
    }

    /// Receipt success survives unavailable current evidence. Only a canonical
    /// current read after recovery may renew native effects.
    async fn refresh_retry_projection(
        &mut self,
        current: Result<FocusExecutionSnapshot, FocusExecutionError>,
    ) {
        let recovered = self
            .recovered_vault
            .as_ref()
            .is_some_and(|(id, generation)| {
                self.projection.vault_id.as_ref() == Some(id)
                    && self.projection.vault_generation == *generation
            });
        match current {
            Ok(snapshot) if recovered => {
                let now = snapshot.observed_at_ms;
                if let Err(error) = self.publish_committed(snapshot, now).await {
                    self.record_failure(error);
                }
            }
            Ok(snapshot) => {
                self.projection.snapshot = Some(snapshot);
                self.projection.error = Some("Native Focus startup recovery is pending".into());
                self.subscriptions.publish(&self.projection, Instant::now());
            }
            Err(error) => {
                self.projection.snapshot = None;
                #[cfg(desktop)]
                self.app
                    .state::<FocusRuntimeState>()
                    .controls
                    .send_replace(None);
                self.record_failure(error);
            }
        }
    }

    pub(super) async fn observe(
        &mut self,
        observation: FocusObservation,
        now: i64,
    ) -> Result<(), FocusExecutionError> {
        if self.frozen || self.pool.is_none() {
            return Ok(());
        }
        self.verify_authority()?;
        self.refresh_foreground();
        let now = self.logical_now(now);
        self.projection
            .vault_id
            .as_deref()
            .ok_or_else(|| error(FocusErrorCode::Unavailable, "Focus has no active vault"))?;
        let _permit = self
            .app
            .state::<crate::vault::write_fence::VaultWriteFence>()
            .acquire_managed_write()?;
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| error(FocusErrorCode::Unavailable, "Focus vault is unavailable"))?;
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| error.to_string())?;
        let device_id = self.device_id()?;
        let calendar = calendar::resolve_for_admission(&mut tx, now, &device_id).await?;
        let calendar_commitment = calendar.commitment.is_some();
        let context = FocusExecutionContext {
            now_ms: now,
            platform: platform(),
            foreground: self.foreground,
            device_id: Some(device_id),
            commitment: calendar.commitment,
            local_time: Some(Arc::new(local_time::NativeLocalTime(self.app.clone()))),
            planned_blocks: calendar
                .planned_blocks
                .into_iter()
                .map(local_time::planned_block)
                .collect(),
        };
        let snapshot = focus_apply_observation_tx(&mut tx, &observation, &context).await?;
        self.verify_authority()?;
        tx.commit()
            .await
            .map_err(|error| format!("Commit native Focus observation: {error}"))?;
        self.next_boundary_ms = calendar.next_boundary_ms;
        self.calendar_commitment = calendar_commitment;
        self.publish_committed(snapshot, now).await
    }

    /// Log each new degraded reason once while one keeps recurring. The transition
    /// already committed, so this runs before any check that can skip publishing.
    fn note_adaptive_degraded(&mut self, degraded: Option<String>) {
        if let Some(reason) = degraded.as_deref()
            && self.adaptive_degraded.as_deref() != Some(reason)
        {
            eprintln!("Focus skipped adaptive evidence: {reason}");
        }
        self.adaptive_degraded = degraded;
    }

    pub(super) async fn publish_committed(
        &mut self,
        mut snapshot: FocusExecutionSnapshot,
        now: i64,
    ) -> Result<(), FocusExecutionError> {
        self.note_adaptive_degraded(snapshot.adaptive_degraded.take());
        self.verify_authority()?;
        // A receipt can describe an earlier successful command. It cannot roll a
        // newer canonical projection or its native effects back to that revision.
        if self
            .projection
            .snapshot
            .as_ref()
            .is_some_and(|previous| previous.revision > snapshot.revision)
        {
            return Ok(());
        }
        let previous_run = self
            .projection
            .snapshot
            .as_ref()
            .and_then(|previous| previous.run.as_ref())
            .map(|run| run.id.as_str());
        if previous_run != snapshot.run.as_ref().map(|run| run.id.as_str()) {
            self.last_clock = Some(clock::ClockObservation {
                wall_ms: now,
                monotonic: Instant::now(),
            });
        }
        self.projection.snapshot = Some(snapshot);
        self.publish_effect_authority(now);
        self.idle_grace.update(
            self.projection.vault_generation,
            if platform() == FocusPlatform::Desktop {
                self.projection.snapshot.as_ref()
            } else {
                None
            },
            Instant::now(),
        );
        #[cfg(desktop)]
        self.app.state::<FocusRuntimeState>().controls.send_replace(
            (!self.freeze_requested())
                .then_some(&self.projection)
                .and_then(|projection| {
                    projection
                        .vault_id
                        .as_ref()
                        .zip(projection.snapshot.as_ref())
                        .map(|(vault_id, snapshot)| FocusNativeContext {
                            vault_id: vault_id.clone(),
                            vault_generation: self.projection.vault_generation,
                            revision: snapshot.revision,
                            mode: snapshot.mode,
                            phase: snapshot.segment.as_ref().map(|segment| segment.phase),
                            run_id: snapshot.run.as_ref().map(|run| run.id.clone()),
                            segment_id: snapshot.segment.as_ref().map(|segment| segment.id.clone()),
                            idle_detected_at_ms: snapshot.idle_detected_at_ms,
                        })
                }),
        );
        self.projection.error = self.effects.message(&self.projection);
        self.next_retry_ms = None;
        self.subscriptions.publish(&self.projection, Instant::now());
        self.deliver_effects(now);
        // Effect delivery cannot turn an already committed command into a failed
        // mutation. The canonical result includes any pending effect error.
        Ok(())
    }

    pub(super) fn deliver_effects(&mut self, now_ms: i64) {
        self.publish_effect_authority(now_ms);
        if self.freeze_requested() {
            return;
        }
        if self.projection.snapshot.is_none() {
            return;
        }
        if let Err(error) = self.effects.publish(&self.projection, self.pool.as_ref()) {
            self.projection.error = Some(error.to_string());
            self.subscriptions.publish(&self.projection, Instant::now());
        }
        self.sync_delivery_feedback();
    }

    pub(super) fn sync_delivery_feedback(&mut self) {
        let delivery_error = self.effects.message(&self.projection);
        if self.next_retry_ms.is_none() && self.projection.error != delivery_error {
            self.projection.error = delivery_error;
            self.subscriptions.publish(&self.projection, Instant::now());
        }
    }

    pub(super) fn record_failure(&mut self, failure: FocusExecutionError) {
        self.clear_effect_authority();
        self.projection.error = Some(failure.to_string());
        self.next_retry_ms = now_ms().ok().map(|now| now + ERROR_RETRY_INTERVAL_MS);
        self.subscriptions.publish(&self.projection, Instant::now());
    }

    fn publish_effect_authority(&self, now_ms: i64) {
        self.app
            .state::<FocusRuntimeState>()
            .authority
            .send_replace(
                (!self.freeze_requested())
                    .then(|| effects::committed_effect(&self.projection, now_ms))
                    .flatten()
                    .map(|effect| authority::EffectAuthority::new(effect, now_ms, Instant::now())),
            );
    }

    pub(super) fn clear_effect_authority(&self) {
        self.app
            .state::<FocusRuntimeState>()
            .authority
            .send_replace(None);
    }
}
