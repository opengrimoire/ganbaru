//! Soundtrack activation uses canonical Calendar geometry or a committed Focus effect.

use super::*;
use ganbaru_music::assignments::{MusicActivityPhase, MusicAssignmentBehavior};
use ganbaru_pomodoro::CommittedFocusEffect;
use sqlx::{Sqlite, Transaction};

const MAX_CONTEXT_REFERENCE_BYTES: i64 = 200;

#[derive(Debug, sqlx::FromRow)]
struct AssignmentRow {
    source: String,
    behavior: String,
    playlist_id: Option<String>,
    soundscape_behavior: String,
    soundscape_id: Option<String>,
}

async fn resolve_assignment(
    connection: &mut sqlx::SqliteConnection,
    event_id: &str,
    phase: MusicActivityPhase,
) -> MusicLibraryResult<Option<AssignmentRow>> {
    let oversized: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM music_context_assignments
         WHERE phase = ? AND (
           (owner_id = ? AND owner_kind IN ('event-override', 'event-snapshot'))
           OR (owner_kind = 'work-environment' AND owner_id =
             (SELECT environment_id FROM calendar_events WHERE id = ?)))
         AND (length(CAST(playlist_id AS BLOB)) > ? OR length(CAST(soundscape_id AS BLOB)) > ?))",
    )
    .bind(phase.as_ref())
    .bind(event_id)
    .bind(event_id)
    .bind(MAX_CONTEXT_REFERENCE_BYTES)
    .bind(MAX_CONTEXT_REFERENCE_BYTES)
    .fetch_one(&mut *connection)
    .await
    .map_err(|error| MusicLibraryError::database("admit soundtrack references", error))?;
    if oversized {
        return Err(MusicLibraryError::validation(
            "assignment",
            "a soundtrack reference exceeds its size limit",
        ));
    }
    sqlx::query_as(
        "SELECT owner_kind AS source, behavior, playlist_id, soundscape_behavior, soundscape_id
         FROM music_context_assignments
         WHERE phase = ? AND (behavior != 'inherit' OR soundscape_behavior != 'inherit') AND (
           (owner_id = ? AND owner_kind IN ('event-override', 'event-snapshot'))
           OR (owner_kind = 'work-environment' AND owner_id =
             (SELECT environment_id FROM calendar_events WHERE id = ?)))
         ORDER BY CASE owner_kind WHEN 'event-override' THEN 0
           WHEN 'work-environment' THEN 1 ELSE 2 END LIMIT 1",
    )
    .bind(phase.as_ref())
    .bind(event_id)
    .bind(event_id)
    .fetch_optional(connection)
    .await
    .map_err(|error| MusicLibraryError::database("resolve accepted soundtrack", error))
}

#[cfg(test)]
pub(super) async fn test_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    pool
}

#[derive(Clone)]
pub(super) enum AssignmentAuthority {
    Focus(Box<CommittedFocusEffect>),
    #[cfg(desktop)]
    Calendar {
        start_ms: i64,
        end_ms: i64,
    },
}

#[derive(Clone)]
pub(super) struct ContextActivation {
    pub event_id: String,
    pub event_title: String,
    pub key: String,
    pub phase: MusicActivityPhase,
    pub authority: AssignmentAuthority,
}

impl ContextActivation {
    pub(super) fn owner(&self) -> SessionOwner {
        match self.authority {
            AssignmentAuthority::Focus(_) => SessionOwner::Pomodoro,
            #[cfg(desktop)]
            AssignmentAuthority::Calendar { .. } => SessionOwner::CalendarEvent,
        }
    }

    /// Check cached committed authority without querying or locking a writer.
    pub(super) fn current(&self, app: &tauri::AppHandle) -> MusicLibraryResult<bool> {
        match &self.authority {
            AssignmentAuthority::Focus(effect) => {
                if effect.effective_valid_until_ms() <= now_ms() {
                    return Ok(false);
                }
                crate::pomodoro::native_runtime::effect_is_current(app, effect, now_ms()).map_err(
                    |error| MusicLibraryError::runtime("check soundtrack Focus authority", error),
                )
            }
            #[cfg(desktop)]
            AssignmentAuthority::Calendar { start_ms, end_ms } => {
                let now = now_ms();
                let focus =
                    crate::pomodoro::native_runtime::current_effect(app, now).map_err(|error| {
                        MusicLibraryError::runtime("check soundtrack Focus priority", error)
                    })?;
                Ok(now >= *start_ms
                    && now < *end_ms
                    && focus.is_none_or(|effect| {
                        effect.run_id.is_none()
                            || matches!(
                                effect.mode,
                                ganbaru_pomodoro::FocusMode::Stopped
                                    | ganbaru_pomodoro::FocusMode::Expired
                            )
                    }))
            }
        }
    }
}

impl Owner {
    pub(super) async fn retry_context(
        &mut self,
        action: Option<(&str, &str)>,
    ) -> MusicLibraryResult<()> {
        let context = self
            .state
            .context
            .as_ref()
            .filter(|context| context.state == "unavailable")
            .ok_or_else(|| MusicLibraryError::conflict("The soundtrack no longer needs a retry"))?;
        match self.state.owner {
            SessionOwner::Pomodoro => {
                let app = self.app.clone();
                let effect = tauri::async_runtime::spawn_blocking(move || {
                    crate::pomodoro::native_runtime::current_effect(&app, now_ms())
                })
                .await
                .map_err(|error| {
                    MusicLibraryError::runtime("read soundtrack retry authority", error)
                })?
                .map_err(|error| {
                    MusicLibraryError::runtime("resolve soundtrack retry authority", error)
                })?
                .filter(|effect| effect.mode == ganbaru_pomodoro::FocusMode::Running)
                .ok_or_else(|| MusicLibraryError::conflict("Focus is no longer running"))?;
                let key = format!(
                    "{}:{}:{}",
                    effect.run_id.as_deref().unwrap_or_default(),
                    effect.segment_id.as_deref().unwrap_or_default(),
                    effect.event_id.as_deref().unwrap_or_default()
                );
                if key != context.activation_key {
                    return Err(MusicLibraryError::conflict(
                        "The soundtrack phase changed before retry",
                    ));
                }
                self.apply_focus_assignment(&effect, &key, action).await?;
            }
            #[cfg(desktop)]
            SessionOwner::CalendarEvent => {
                self.calendar.retry();
                self.reconcile_calendar_assignment(action).await?;
            }
            _ => {
                return Err(MusicLibraryError::conflict(
                    "The soundtrack was superseded by another playback owner",
                ));
            }
        }
        Ok(())
    }

    pub(super) async fn apply_context_assignment(
        &mut self,
        activation: ContextActivation,
        mut transaction: Transaction<'_, Sqlite>,
        permit: crate::vault::ownership::ManagedVaultWritePermit,
        action: Option<(&str, &str)>,
    ) -> MusicLibraryResult<bool> {
        if self.state.owner == SessionOwner::Review {
            return Ok(false);
        }
        let Some(assignment) =
            resolve_assignment(&mut transaction, &activation.event_id, activation.phase).await?
        else {
            let app = self.app.clone();
            let check = activation.clone();
            if !tauri::async_runtime::spawn_blocking(move || check.current(&app))
                .await
                .map_err(|error| {
                    MusicLibraryError::runtime("check absent soundtrack authority", error)
                })??
                || self.lifecycle.is_revoked()
            {
                return Ok(false);
            }
            let clear_context = self.state.context.is_some()
                && matches!(
                    self.state.owner,
                    SessionOwner::CalendarEvent | SessionOwner::Pomodoro
                );
            if action.is_some() || clear_context {
                let mut next = self.state.clone();
                let transition = if clear_context {
                    next.context = None;
                    if activation.owner() == SessionOwner::CalendarEvent
                        || self.state.owner == SessionOwner::CalendarEvent
                    {
                        next.owner = SessionOwner::Manual;
                    } else {
                        next.context = Some(SessionContext {
                            activation_key: activation.key.clone(),
                            event_id: activation.event_id.clone(),
                            event_title: activation.event_title.clone(),
                            phase: activation.phase,
                            behavior: MusicAssignmentBehavior::Inherit,
                            assignment_source: "none".into(),
                            playlist_id: next.playlist_id.clone(),
                            state: "kept".into(),
                            issue: None,
                        });
                    }
                    next.apply(SessionIntent::Refresh, now_ms())
                } else {
                    Transition::default()
                };
                persistence::commit_in_transaction(
                    &mut transaction,
                    &self.device_id,
                    &next,
                    &transition,
                    action,
                    now_ms(),
                )
                .await?;
                transaction.commit().await.map_err(|error| {
                    MusicLibraryError::database("accept absent soundtrack retry", error)
                })?;
                self.install(next, transition).await?;
            }
            return Ok(true);
        };
        let AssignmentRow {
            source,
            behavior,
            playlist_id,
            soundscape_behavior,
            soundscape_id,
        } = assignment;
        let behavior = MusicAssignmentBehavior::try_from(behavior.as_str())
            .map_err(|error| MusicLibraryError::validation("assignment.behavior", error))?;
        let mut next = self.state.clone();
        #[cfg(desktop)]
        {
            next.context_error = None;
        }
        next.owner = activation.owner();
        next.suspended = None;
        next.review_checkpoint_id = None;
        next.context = Some(SessionContext {
            activation_key: activation.key.clone(),
            event_id: activation.event_id.clone(),
            event_title: activation.event_title.clone(),
            phase: activation.phase,
            behavior,
            assignment_source: if source == "event-snapshot" {
                "project-snapshot".into()
            } else {
                source
            },
            playlist_id: playlist_id.clone(),
            state: "prepared".into(),
            issue: None,
        });
        let replaces_queue = matches!(
            behavior,
            MusicAssignmentBehavior::PlayAutomatically | MusicAssignmentBehavior::PrepareSilently
        );
        let mut queue_replaced = false;
        let mut transition = if replaces_queue {
            let loaded = if let Some(playlist_id) = playlist_id {
                let definition = SessionQueueIntent::SavedPlaylist {
                    playlist_id,
                    explicit_item_id: None,
                    avoid_item_id: self
                        .state
                        .current_entry()
                        .and_then(|entry| entry.item_id.clone()),
                };
                queue::load_queue(
                    &mut transaction,
                    &self.roots,
                    &definition,
                    &mut next,
                    now_ms(),
                )
                .await
            } else {
                Err(MusicLibraryError::not_found("playlist", "unassigned"))
            };
            match loaded {
                Ok(_) => {
                    queue_replaced = true;
                    next.current = None;
                    next.history.clear();
                    next.failed.clear();
                    next.shuffle.clear();
                    let avoid = self
                        .state
                        .current_entry()
                        .and_then(|entry| entry.item_id.as_deref());
                    let autoplay = behavior == MusicAssignmentBehavior::PlayAutomatically;
                    if let Some(context) = &mut next.context {
                        context.state = if autoplay { "playing" } else { "prepared" }.into();
                    }
                    let transition = next.initial(None, avoid, autoplay, now_ms());
                    if next.issue == Some(SessionIssue::NoEligibleItems)
                        && let Some(context) = &mut next.context
                    {
                        context.state = "unavailable".into();
                        context.issue = Some("no-eligible-items".into());
                    }
                    transition
                }
                Err(error)
                    if matches!(
                        error.code,
                        ganbaru_music::error::MusicLibraryErrorCode::NotFound
                            | ganbaru_music::error::MusicLibraryErrorCode::Validation
                    ) =>
                {
                    // Queue reads can fail after changing the draft. Retain the actual prior queue.
                    let context = next.context.take();
                    next = self.state.clone();
                    next.owner = activation.owner();
                    next.context = context;
                    next.issue = Some(SessionIssue::NoEligibleItems);
                    if let Some(context) = &mut next.context {
                        context.state = "unavailable".into();
                        context.issue = Some("missing-playlist".into());
                    }
                    next.apply(SessionIntent::Pause, now_ms())
                }
                Err(error) => return Err(error),
            }
        } else {
            if let Some(context) = &mut next.context {
                context.state = if behavior == MusicAssignmentBehavior::PauseMusic {
                    "paused"
                } else {
                    "kept"
                }
                .into();
            }
            next.apply(
                if behavior == MusicAssignmentBehavior::PauseMusic {
                    SessionIntent::Pause
                } else {
                    SessionIntent::Refresh
                },
                now_ms(),
            )
        };
        if behavior == MusicAssignmentBehavior::Inherit {
            // Background-only assignments do not take ownership of the main music stream.
            next.owner = self.state.owner;
            next.context = self.state.context.clone();
            next.suspended = self.state.suspended.clone();
            next.review_checkpoint_id = self.state.review_checkpoint_id.clone();
        }
        if queue_replaced {
            let mut prior = self.state.clone();
            transition
                .listening
                .extend(prior.apply(SessionIntent::Stop, now_ms()).listening);
        }
        #[cfg(desktop)]
        let soundscape = super::context_soundscape::prepare(
            &mut transaction,
            &self.device_id,
            &soundscape_behavior,
            soundscape_id.as_deref(),
        )
        .await?;
        #[cfg(mobile)]
        let _ = (soundscape_behavior, soundscape_id);
        #[cfg(desktop)]
        if soundscape.missing
            && let Some(context) = &mut next.context
            && context.issue.is_none()
        {
            context.issue = Some("deleted-soundscape".into());
        }
        let app = self.app.clone();
        let check = activation.clone();
        let current = tauri::async_runtime::spawn_blocking(move || check.current(&app))
            .await
            .map_err(|error| MusicLibraryError::runtime("check accepted soundtrack", error))??;
        if !current || self.lifecycle.is_revoked() {
            return Ok(false);
        }
        persistence::commit_in_transaction(
            &mut transaction,
            &self.device_id,
            &next,
            &transition,
            action,
            now_ms(),
        )
        .await?;
        transaction
            .commit()
            .await
            .map_err(|error| MusicLibraryError::database("commit accepted soundtrack", error))?;
        self.install(next, transition).await?;
        #[cfg(desktop)]
        if let Err(error) = self
            .deliver_context_soundscape(soundscape, activation, permit)
            .await
        {
            self.state.context_error = Some(super::super::policy::ContextFailure::Soundscape(
                error.to_string(),
            ));
            self.publish(false)?;
        }
        #[cfg(desktop)]
        {
            let version: i64 = sqlx::query_scalar(
                "SELECT version FROM music_soundscape_state WHERE singleton = 1",
            )
            .fetch_one(self.pool.as_ref().expect("initialized music pool"))
            .await
            .map_err(|error| {
                MusicLibraryError::database("project accepted background sound", error)
            })?;
            self.state.soundscape_version = Some(version);
            self.publish(false)?;
        }
        #[cfg(mobile)]
        drop(permit);
        Ok(true)
    }
}

#[cfg(desktop)]
mod calendar;
#[cfg(desktop)]
pub(super) use calendar::CalendarActivationState;

#[cfg(test)]
mod tests;
