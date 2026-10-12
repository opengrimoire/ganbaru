//! Music effects admitted from committed Focus execution, with manual-intent supersession.

use super::*;
use ganbaru_music::assignments::MusicActivityPhase;
use ganbaru_pomodoro::{CommittedFocusEffect, FocusMode, FocusPhase};
use std::time::Instant;

#[derive(Clone)]
pub(super) struct FocusLease {
    effect: CommittedFocusEffect,
    activation_key: String,
    manual_revision: u64,
    paused_by_focus: bool,
    applied: bool,
    expired: bool,
    expires_at: Instant,
}

impl FocusLease {
    #[cfg(desktop)]
    pub(super) fn completion_identity(&self) -> String {
        format!("{}:{:?}", self.activation_key, self.effect.mode)
    }

    fn renew(&mut self, effect: &CommittedFocusEffect, wall_ms: i64, monotonic: Instant) {
        if effect.valid_until_ms > self.effect.valid_until_ms {
            self.expires_at = lease_deadline(effect, wall_ms, monotonic);
            self.effect.valid_until_ms = effect.valid_until_ms;
        }
    }
}

impl Owner {
    pub(super) async fn reconcile_focus(
        &mut self,
        effect: CommittedFocusEffect,
    ) -> MusicLibraryResult<()> {
        if self
            .vault_id
            .as_deref()
            .is_none_or(|vault_id| !effect.matches_vault_authority(vault_id, self.vault_generation))
        {
            return Ok(());
        }
        if !self.focus_effect_is_current(&effect).await? {
            return Ok(());
        }
        if let Some(lease) = &mut self.focus {
            if effect.vault_generation < lease.effect.vault_generation
                || (effect.vault_generation == lease.effect.vault_generation
                    && effect.execution_revision < lease.effect.execution_revision)
            {
                return Ok(());
            }
            if effect.vault_generation == lease.effect.vault_generation
                && effect.execution_revision == lease.effect.execution_revision
            {
                if effect.effective_valid_until_ms() <= now_ms() {
                    lease.effect.valid_until_ms = effect.valid_until_ms;
                    self.expire_focus().await?;
                    return Ok(());
                }
                lease.renew(&effect, now_ms(), Instant::now());
                if lease.applied
                    || self.state.owner == SessionOwner::Review
                    || effect.mode != FocusMode::Running
                {
                    return Ok(());
                }
            }
        }
        let key = format!(
            "{}:{}:{}",
            effect.run_id.as_deref().unwrap_or_default(),
            effect.segment_id.as_deref().unwrap_or_default(),
            effect.event_id.as_deref().unwrap_or_default()
        );
        let prior = self.focus.clone();
        let new_phase = prior
            .as_ref()
            .is_none_or(|prior| prior.activation_key != key || !prior.applied);
        let mut lease = FocusLease {
            effect: effect.clone(),
            activation_key: key.clone(),
            manual_revision: self.state.manual_revision,
            paused_by_focus: false,
            applied: prior
                .as_ref()
                .is_some_and(|prior| prior.activation_key == key && prior.applied),
            expired: false,
            expires_at: lease_deadline(&effect, now_ms(), Instant::now()),
        };
        if effect.effective_valid_until_ms() <= now_ms()
            || matches!(
                effect.mode,
                FocusMode::Stopped | FocusMode::Expired | FocusMode::ReturnWait
            )
        {
            if self.state.owner == SessionOwner::Pomodoro
                && prior
                    .as_ref()
                    .is_some_and(|prior| prior.manual_revision == self.state.manual_revision)
            {
                self.focus_pause(false).await?;
            }
        } else if effect.mode != FocusMode::Running {
            let preference = self.pause_with_focus().await?;
            if !self.focus_effect_is_current(&effect).await? {
                return Ok(());
            }
            let was_playing = matches!(
                self.state.status,
                SessionStatus::Playing | SessionStatus::Loading
            );
            if preference && effect.phase == Some(FocusPhase::Focus) && was_playing {
                self.focus_pause(false).await?;
                lease.paused_by_focus = true;
            } else if let Some(prior) =
                prior.filter(|prior| prior.manual_revision == self.state.manual_revision)
            {
                lease.paused_by_focus = prior.paused_by_focus;
            }
        } else if new_phase {
            lease.applied = self.apply_focus_assignment(&effect, &key, None).await?;
            lease.manual_revision = self.state.manual_revision;
        } else if let Some(prior) = prior
            && prior.paused_by_focus
            && prior.manual_revision == self.state.manual_revision
            && self.pause_with_focus().await?
        {
            if !self.focus_effect_is_current(&effect).await? {
                return Ok(());
            }
            let mut next = self.state.clone();
            let transition = next.apply(SessionIntent::Play, now_ms());
            self.commit(next, transition, None, true).await?;
        }
        self.focus = Some(lease);
        Ok(())
    }

    pub(super) async fn expire_focus(&mut self) -> MusicLibraryResult<()> {
        if self.focus.as_ref().is_none_or(|lease| lease.expired) {
            return Ok(());
        }
        let app = self.app.clone();
        let current = tauri::async_runtime::spawn_blocking(move || {
            crate::pomodoro::native_runtime::current_effect(&app, now_ms())
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("read current Focus authority", error))?
        .map_err(|error| MusicLibraryError::runtime("check Focus music lease", error))?;
        let expired = self.focus.as_ref().is_some_and(|lease| {
            lease.effect.effective_valid_until_ms() <= now_ms()
                || Instant::now() >= lease.expires_at
                || current.as_ref().is_none_or(|current| {
                    !crate::pomodoro::native_runtime::allows_existing_lease(current, &lease.effect)
                })
        });
        if !expired {
            return Ok(());
        }
        let owned = self
            .focus
            .as_ref()
            .is_some_and(|lease| lease.manual_revision == self.state.manual_revision);
        if owned && self.state.owner == SessionOwner::Pomodoro {
            self.focus_pause(true).await?;
        }
        if let Some(lease) = &mut self.focus {
            lease.expired = true;
        }
        Ok(())
    }

    async fn focus_pause(&mut self, expired: bool) -> MusicLibraryResult<()> {
        let mut next = self.state.clone();
        let transition = next.apply(SessionIntent::Pause, now_ms());
        if expired {
            next.issue = Some(SessionIssue::Interrupted);
        }
        if let Some(context) = &mut next.context {
            context.state = "paused".into();
        }
        self.commit(next, transition, None, true).await
    }

    async fn pause_with_focus(&self) -> MusicLibraryResult<bool> {
        let app = self.app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let raw = crate::vault::vault_read_config(app).map_err(|error| {
                MusicLibraryError::runtime("read focus music preference", error)
            })?;
            let config: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
                MusicLibraryError::runtime("decode focus music preference", error)
            })?;
            Ok(config
                .get("preferences")
                .and_then(|preferences| preferences.get("musicPauseOnPomodoroPause"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true))
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("read focus music preference", error))?
    }

    pub(super) async fn apply_focus_assignment(
        &mut self,
        effect: &CommittedFocusEffect,
        key: &str,
        action: Option<(&str, &str)>,
    ) -> MusicLibraryResult<bool> {
        let (Some(event_id), Some(phase)) = (&effect.event_id, effect.phase) else {
            return Ok(false);
        };
        let activation = super::context::ContextActivation {
            event_id: event_id.clone(),
            event_title: effect.event_title.clone().unwrap_or_default(),
            key: key.into(),
            phase: match phase {
                FocusPhase::Focus => MusicActivityPhase::Focus,
                FocusPhase::ShortBreak => MusicActivityPhase::ShortBreak,
                FocusPhase::LongBreak => MusicActivityPhase::LongBreak,
            },
            authority: super::context::AssignmentAuthority::Focus(Box::new(effect.clone())),
        };
        let permit = self.write_permit().await?;
        let transaction = self
            .pool
            .as_ref()
            .expect("initialized music pool")
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| MusicLibraryError::database("begin Focus music assignment", error))?;
        self.apply_context_assignment(activation, transaction, permit, action)
            .await
    }

    async fn focus_effect_is_current(
        &self,
        effect: &CommittedFocusEffect,
    ) -> MusicLibraryResult<bool> {
        let app = self.app.clone();
        let effect = effect.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::pomodoro::native_runtime::effect_is_current(&app, &effect, now_ms())
        })
        .await
        .map_err(|error| MusicLibraryError::runtime("read Focus revision", error))?
        .map_err(|error| MusicLibraryError::runtime("check committed Focus revision", error))
    }
}

/// Capture the producer's remaining allowance once; duplicate delivery cannot reset it.
fn lease_deadline(effect: &CommittedFocusEffect, wall_ms: i64, monotonic: Instant) -> Instant {
    let remaining_ms = effect
        .effective_valid_until_ms()
        .saturating_sub(wall_ms)
        .clamp(0, crate::pomodoro::native_runtime::EFFECT_LEASE_MS);
    monotonic + Duration::from_millis(remaining_ms as u64)
}

#[cfg(test)]
mod tests;
