//! Installs committed transitions and routes their effects to the browser host or the native
//! backend under captured delivery authority.

use super::*;

impl Owner {
    pub(super) async fn install(
        &mut self,
        next: SessionPolicy,
        transition: Transition,
    ) -> MusicLibraryResult<()> {
        let queue_changed = next.queue_revision != self.state.queue_revision
            || next.session_id != self.state.session_id;
        #[cfg(desktop)]
        let prior_status = self.state.status;
        self.state = next;
        #[cfg(desktop)]
        if self
            .completion_duck
            .as_ref()
            .is_some_and(|duck| !duck.matches(&self.state))
        {
            self.release_completion().await?;
        }
        for effect in transition.effects {
            if let Err(error) = self.apply_effect(effect).await {
                self.state.status = SessionStatus::Error;
                self.state.issue = Some(SessionIssue::SourceFailure);
                let source_failure = error.is_source_failure();
                self.state.error = Some(error.to_string());
                if self
                    .state
                    .current_entry()
                    .is_some_and(|entry| entry.backend == SessionBackend::Browser)
                {
                    self.state.browser_host = false;
                    self.state.status = SessionStatus::Paused;
                    self.state.autoplay_requested = false;
                    self.state.issue = Some(SessionIssue::BrowserHostUnavailable);
                    self.subscriptions
                        .effect(
                            SessionEffect::Stop {
                                generation: self.state.generation,
                            },
                            now_ms(),
                        )
                        .map_err(|error| {
                            MusicLibraryError::runtime("stop unavailable browser Music", error)
                        })?;
                    #[cfg(not(target_os = "ios"))]
                    {
                        // Switching to a browser source can fail while stopping
                        // the prior native decoder. Retain that native drain too.
                        self.native_drain_generation = Some(self.state.generation);
                    }
                    break;
                }
                #[cfg(desktop)]
                {
                    if source_failure {
                        self.backend_error_pending =
                            self.state.current_entry().map(|entry| SessionObservation {
                                session_id: self.state.session_id.clone(),
                                generation: self.state.generation,
                                sequence: self.state.last_sequence.saturating_add(1),
                                source_identity: entry.source.identity.clone(),
                                status: SessionStatus::Error,
                                position_ms: self.state.position_ms,
                                duration_ms: self.state.duration_ms,
                                error: self.state.error.clone(),
                            });
                    }
                }
                #[cfg(not(target_os = "ios"))]
                if !source_failure {
                    let error = self.state.error.take();
                    // A lost execution result does not prove the decoder is
                    // stopped. Revoke old observations and retain one Stop to
                    // drain before any later playback effect can be delivered.
                    self.state.interrupt_backend();
                    self.state.error = error;
                    self.native_drain_generation = Some(self.state.generation);
                }
                #[cfg(target_os = "ios")]
                {
                    self.state.status = SessionStatus::Paused;
                    self.state.issue = Some(SessionIssue::Interrupted);
                }
                break;
            }
        }
        #[cfg(desktop)]
        self.follow_music_with_soundscape(prior_status).await;
        self.publish(queue_changed)?;
        Ok(())
    }

    pub(super) async fn apply_effect(
        &mut self,
        effect: SessionEffect,
    ) -> Result<(), backend::Failure> {
        #[cfg(desktop)]
        let effect = self.attenuate_completion(effect);
        if self.lifecycle.is_revoked()
            && !matches!(
                effect,
                SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
            )
        {
            return Err("Music playback delivery is revoked while the vault is changing".into());
        }
        #[cfg(not(target_os = "ios"))]
        self.drain_native_delivery().await?;
        let browser = self
            .state
            .current_entry()
            .is_some_and(|entry| entry.backend == SessionBackend::Browser);
        #[cfg(not(target_os = "ios"))]
        let authority = if browser && matches!(effect, SessionEffect::Load { .. }) {
            self.native_delivery_authority(&SessionEffect::Stop {
                generation: backend::effect_generation(&effect),
            })?
        } else {
            self.native_delivery_authority(&effect)?
        };
        if browser {
            backend::apply(
                &self.app,
                Some(SessionBackend::Browser),
                effect.clone(),
                #[cfg(not(target_os = "ios"))]
                authority,
            )
            .await?;
            if !self.subscriptions.browser_available(now_ms())
                && !matches!(
                    effect,
                    SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
                )
            {
                // Intent and settings remain canonical while the browser cannot consume them.
                return Ok(());
            }
            self.subscriptions
                .effect(effect, now_ms())
                .map_err(|error| backend::Failure::Interrupted(error.to_string()))
        } else {
            if matches!(
                effect,
                SessionEffect::Load { .. } | SessionEffect::Stop { .. }
            ) {
                self.subscriptions
                    .effect(
                        SessionEffect::Stop {
                            generation: backend::effect_generation(&effect),
                        },
                        now_ms(),
                    )
                    .map_err(|error| error.to_string())?;
            }
            backend::apply(
                &self.app,
                self.state.current_entry().map(|entry| entry.backend),
                effect,
                #[cfg(not(target_os = "ios"))]
                authority,
            )
            .await
        }
    }

    /// An uncertain SDK result must stop before a later effect can reopen playback.
    #[cfg(not(target_os = "ios"))]
    pub(super) async fn drain_native_delivery(&mut self) -> Result<(), backend::Failure> {
        let Some(generation) = self.native_drain_generation else {
            return Ok(());
        };
        let effect = SessionEffect::Stop { generation };
        let authority = self.native_delivery_authority(&effect)?;
        backend::apply(
            &self.app,
            Some(SessionBackend::NativeAudio),
            effect,
            authority,
        )
        .await?;
        self.native_drain_generation = None;
        Ok(())
    }

    /// A queued SDK effect retains its admission identity through actual use.
    #[cfg(not(target_os = "ios"))]
    fn native_delivery_authority(
        &self,
        effect: &SessionEffect,
    ) -> Result<std::sync::Arc<dyn Fn() -> bool + Send + Sync>, String> {
        let draining = matches!(
            effect,
            SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
        ) || matches!(effect, SessionEffect::Settings { volume, .. }
            if *volume == 0.0 || !self.state.autoplay_requested);
        let lifecycle = self.lifecycle.clone();
        let admitted = lifecycle.current();
        let app = self.app.clone();
        let vault_id = self.vault_id.clone();
        let focus = if !draining && self.state.owner == SessionOwner::Pomodoro {
            let (proof, deadline) = crate::pomodoro::native_runtime::capture_music_delivery(&app)
                .ok_or("Committed Focus music authority is unavailable")?;
            let key = format!(
                "{}:{}:{}",
                proof.run_id.as_deref().unwrap_or_default(),
                proof.segment_id.as_deref().unwrap_or_default(),
                proof.event_id.as_deref().unwrap_or_default()
            );
            if self
                .state
                .context
                .as_ref()
                .is_none_or(|context| context.activation_key != key)
            {
                return Err("Committed Focus music phase changed before delivery".into());
            }
            Some((proof, deadline))
        } else {
            None
        };
        Ok(std::sync::Arc::new(move || {
            if !lifecycle.matches(admitted.revision, admitted.intent)
                || (!draining && lifecycle.is_revoked())
            {
                return false;
            }
            if draining {
                return true;
            }
            // A vault replacement freezes Music first, which revokes this lifecycle.
            vault_id.is_some()
                && focus.as_ref().is_none_or(|(proof, deadline)| {
                    crate::pomodoro::native_runtime::music_delivery_is_current(
                        &app, proof, *deadline,
                    )
                })
                && lifecycle.matches(admitted.revision, admitted.intent)
        }))
    }
}
