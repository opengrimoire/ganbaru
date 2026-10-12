use ganbaru_pomodoro::{CommittedFocusEffect, FocusExecutionError};
use std::time::{Duration, Instant};

use super::FocusProjection;

/// Derive all consumer facts from one accepted snapshot and shared ownership fence.
pub(super) fn committed_effect(
    projection: &FocusProjection,
    now_ms: i64,
    ownership_generation: Option<u64>,
) -> Option<CommittedFocusEffect> {
    let snapshot = projection.snapshot.as_ref()?;
    let vault_id = projection.vault_id.as_ref()?;
    Some(CommittedFocusEffect {
        vault_id: vault_id.clone(),
        vault_generation: projection.vault_generation,
        ownership_generation: ownership_generation?,
        execution_revision: snapshot.revision,
        run_id: snapshot.run.as_ref().map(|run| run.id.clone()),
        segment_id: snapshot.segment.as_ref().map(|segment| segment.id.clone()),
        event_id: snapshot.run.as_ref().and_then(|run| run.event_id.clone()),
        occurrence_id: snapshot.run.as_ref().map(|run| run.occurrence_id.clone()),
        event_title: snapshot.run.as_ref().and_then(|run| run.title.clone()),
        phase: snapshot.segment.as_ref().map(|segment| segment.phase),
        mode: snapshot.mode,
        phase_deadline_ms: snapshot.phase_deadline_ms,
        event_deadline_ms: snapshot.run.as_ref().map(|run| run.planned_end_ms),
        remaining_ms: snapshot.remaining_ms,
        valid_until_ms: now_ms.saturating_add(super::EFFECT_LEASE_MS).min(
            snapshot
                .run
                .as_ref()
                .map(|run| run.planned_end_ms)
                .unwrap_or(i64::MAX),
        ),
    })
}

/// Delivery backoff belongs to one accepted revision, independently of execution.
#[derive(Default)]
pub(super) struct DeliveryRetry {
    failure: Option<DeliveryFailure>,
}

struct DeliveryFailure {
    generation: u64,
    revision: i64,
    retry_at: Instant,
    message: String,
}

impl DeliveryRetry {
    pub fn ready(&self, generation: u64, revision: i64, now: Instant) -> bool {
        self.failure.as_ref().is_none_or(|failure| {
            failure.generation != generation
                || failure.revision != revision
                || now >= failure.retry_at
        })
    }

    pub fn failed(&mut self, generation: u64, revision: i64, now: Instant, message: String) {
        self.failure = Some(DeliveryFailure {
            generation,
            revision,
            retry_at: now + Duration::from_millis(super::ERROR_RETRY_INTERVAL_MS as u64),
            message,
        });
    }

    pub fn clear(&mut self) {
        self.failure = None;
    }

    pub fn message(&self) -> Option<&str> {
        self.failure
            .as_ref()
            .map(|failure| failure.message.as_str())
    }

    /// A failed presentation suppresses its overdue alert wake, never a semantic wake.
    pub fn next_delay(
        &self,
        semantic_delay: Duration,
        presentation_delay: Option<Duration>,
        now: Instant,
    ) -> Duration {
        let delivery_delay = self
            .failure
            .as_ref()
            .map(|failure| failure.retry_at.saturating_duration_since(now))
            .or(presentation_delay);
        delivery_delay.map_or(semantic_delay, |delay| semantic_delay.min(delay))
    }
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct FocusEffects {
    last: Option<CommittedFocusEffect>,
    pending: bool,
    #[cfg(desktop)]
    desktop: super::presentation::DesktopPresentation,
    #[cfg(target_os = "android")]
    mobile: super::mobile::AndroidPresentation,
}

impl FocusEffects {
    pub async fn apply(
        &mut self,
        app: &tauri::AppHandle,
        projection: &FocusProjection,
        now_ms: i64,
        lease_ms: i64,
        ownership_generation: Option<u64>,
        pool: Option<&sqlx::SqlitePool>,
    ) -> Result<(), FocusExecutionError> {
        let Some(snapshot) = &projection.snapshot else {
            return Ok(());
        };
        let Some(vault_id) = &projection.vault_id else {
            return Ok(());
        };
        let ownership_generation = ownership_generation.ok_or_else(|| {
            FocusExecutionError::from("Focus effects have no vault ownership fence".to_owned())
        })?;
        if !super::presentation_is_current(app, projection.vault_generation, snapshot.revision) {
            return Err("Native Focus delivery was superseded or expired"
                .to_owned()
                .into());
        }
        // Renew at one third of the lease, rather than fsyncing a device snapshot
        // or enqueueing the same Music state on every visual second.
        let fresh = !self.pending
            && self.last.as_ref().is_some_and(|last| {
                last.matches_vault_authority(vault_id, ownership_generation)
                    && last.vault_generation == projection.vault_generation
                    && last.execution_revision == snapshot.revision
                    && last.valid_until_ms > now_ms.saturating_add(lease_ms.saturating_mul(2) / 3)
            });
        let consumers_result: Result<(), FocusExecutionError> = if fresh {
            Ok(())
        } else {
            let effect = committed_effect(projection, now_ms, Some(ownership_generation))
                .ok_or_else(|| "Focus effects have no canonical execution snapshot".to_owned())?;
            let music_result =
                crate::music::session::reconcile_committed_focus(app, effect.clone());
            let rules_result = publish_rules(app, effect.clone(), now_ms).await;
            self.last = Some(effect);
            self.pending = music_result.is_err() || rules_result.is_err();
            if music_result.is_err()
                && let Err(error) = &rules_result
            {
                eprintln!("Publish accepted Focus rules while Music delivery failed: {error}");
            }
            music_result
                .map_err(FocusExecutionError::from)
                .and(rules_result)
        };
        #[cfg(desktop)]
        let presentation_result = crate::tray::reconcile_committed_focus(
            app,
            projection.vault_generation,
            snapshot,
            now_ms,
        )
        .await
        .map_err(FocusExecutionError::from)
        .and(
            self.desktop
                .apply(app, projection.vault_generation, snapshot, now_ms, pool)
                .await,
        );
        #[cfg(mobile)]
        let _ = pool;
        #[cfg(target_os = "android")]
        let presentation_result = self
            .mobile
            .apply(app, projection, now_ms, ownership_generation)
            .await;
        #[cfg(target_os = "ios")]
        let presentation_result: Result<(), FocusExecutionError> = Ok(());
        consumers_result?;
        presentation_result?;
        Ok(())
    }

    pub async fn revoke(&mut self, app: &tauri::AppHandle) -> Result<(), FocusExecutionError> {
        #[cfg(desktop)]
        let presentation_result = self.desktop.revoke(app).await;
        #[cfg(target_os = "android")]
        let presentation_result = self.mobile.revoke(app).await;
        #[cfg(target_os = "ios")]
        let presentation_result: Result<(), FocusExecutionError> = Ok(());
        if let Some(effect) = &self.last {
            let mut revoked = effect.clone();
            revoked.valid_until_ms = 0;
            crate::music::session::reconcile_committed_focus(app, revoked)?;
            self.last = None;
            self.pending = false;
        }
        presentation_result
    }

    #[cfg(not(target_os = "ios"))]
    pub async fn configure_notification_copy(
        &mut self,
        app: &tauri::AppHandle,
        copy: super::NotificationCopy,
    ) -> Result<(), FocusExecutionError> {
        #[cfg(target_os = "android")]
        {
            self.mobile.configure_copy(app, copy).await
        }
        #[cfg(desktop)]
        {
            let _ = app;
            self.desktop.configure_copy(copy)
        }
    }

    pub fn next_presentation_deadline(&self) -> Option<i64> {
        #[cfg(desktop)]
        {
            self.desktop.next_deadline()
        }
        #[cfg(mobile)]
        {
            None
        }
    }

    pub fn invalidate_preferences(&mut self) {
        #[cfg(desktop)]
        self.desktop.invalidate_preferences();
    }
}

#[cfg(desktop)]
async fn publish_rules(
    app: &tauri::AppHandle,
    effect: CommittedFocusEffect,
    now_ms: i64,
) -> Result<(), FocusExecutionError> {
    let generation = crate::distractions::runtime::capture_publication_generation(app);
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::distractions::state_files::publish_committed_focus(&app, &effect, generation, now_ms)
    })
    .await
    .map_err(|error| format!("Publish accepted Focus rules: {error}"))??;
    Ok(())
}

#[cfg(mobile)]
async fn publish_rules(
    _app: &tauri::AppHandle,
    _effect: CommittedFocusEffect,
    _now_ms: i64,
) -> Result<(), FocusExecutionError> {
    Ok(())
}
