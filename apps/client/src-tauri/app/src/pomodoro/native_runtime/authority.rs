//! Current post-commit authority, independent of asynchronous consumer delivery.

use std::time::{Duration, Instant};

use ganbaru_pomodoro::{CommittedFocusEffect, FocusMode};
use tauri::{Manager, Runtime};

use super::FocusRuntimeState;

#[derive(Clone)]
pub(super) struct EffectAuthority {
    effect: CommittedFocusEffect,
    expires_at: Instant,
    delivery_expires_at: Instant,
}

impl EffectAuthority {
    pub fn new(effect: CommittedFocusEffect, wall_ms: i64, monotonic: Instant) -> Self {
        let remaining_ms = effect
            .effective_valid_until_ms()
            .saturating_sub(wall_ms)
            .clamp(0, super::EFFECT_LEASE_MS);
        Self {
            effect,
            expires_at: monotonic + Duration::from_millis(remaining_ms as u64),
            delivery_expires_at: monotonic + Duration::from_millis(super::EFFECT_LEASE_MS as u64),
        }
    }

    fn current(&self, wall_ms: i64, monotonic: Instant) -> Option<&CommittedFocusEffect> {
        (monotonic < self.expires_at && wall_ms < self.effect.effective_valid_until_ms())
            .then_some(&self.effect)
    }

    fn presentable(&self, wall_ms: i64, monotonic: Instant) -> bool {
        if matches!(
            self.effect.mode,
            FocusMode::Running
                | FocusMode::ManualPause
                | FocusMode::IdlePause
                | FocusMode::Suspended
        ) {
            self.current(wall_ms, monotonic).is_some()
        } else {
            monotonic < self.delivery_expires_at
        }
    }
}

/// Read fresh canonical authority rather than the last effect a consumer received.
/// A civil-clock rollback cannot extend this owner's monotonic publication lease.
pub(crate) fn current_effect<R: Runtime>(
    app: &tauri::AppHandle<R>,
    wall_ms: i64,
) -> Result<Option<CommittedFocusEffect>, String> {
    canonical_effect(app, wall_ms, true)
}

fn canonical_effect<R: Runtime>(
    app: &tauri::AppHandle<R>,
    wall_ms: i64,
    require_phase_lease: bool,
) -> Result<Option<CommittedFocusEffect>, String> {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return Ok(None);
    };
    if state.lifecycle.is_revoked() {
        return Ok(None);
    }
    let effect = state
        .authority
        .borrow()
        .as_ref()
        .and_then(|authority| {
            if require_phase_lease {
                authority.current(wall_ms, Instant::now())
            } else {
                authority
                    .presentable(wall_ms, Instant::now())
                    .then_some(&authority.effect)
            }
        })
        .cloned();
    let Some(effect) = effect else {
        return Ok(None);
    };
    if crate::vault::active_vault_id(app)? != effect.vault_id {
        return Ok(None);
    }
    let status = app
        .state::<crate::vault::ownership::VaultOwnershipManager>()
        .status(&effect.vault_id)?;
    if !status.can_write
        || status.generation != effect.ownership_generation
        || state.lifecycle.is_revoked()
    {
        return Ok(None);
    }
    // Ownership reads can wait for IO. Do not return the authority that preceded
    // a commit or revocation which happened while that read was in progress.
    let current = state.authority.borrow().as_ref().is_some_and(|authority| {
        same_revision(&authority.effect, &effect)
            && if require_phase_lease {
                authority.current(wall_ms, Instant::now()).is_some()
            } else {
                authority.presentable(wall_ms, Instant::now())
            }
    });
    Ok(current.then_some(effect))
}

/// Require the exact committed revision when admitting an asynchronous effect.
pub(crate) fn effect_is_current<R: Runtime>(
    app: &tauri::AppHandle<R>,
    effect: &CommittedFocusEffect,
    wall_ms: i64,
) -> Result<bool, String> {
    Ok(canonical_effect(app, wall_ms, false)?
        .is_some_and(|current| same_revision(&current, effect)))
}

/// Check the native callback immediately before changing a platform surface.
/// Only cached committed authority is read here; platform callbacks do no vault IO.
pub(crate) fn presentation_is_current<R: Runtime>(
    app: &tauri::AppHandle<R>,
    generation: u64,
    revision: i64,
) -> bool {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return false;
    };
    if state.lifecycle.is_revoked() {
        return false;
    }
    let Ok(wall_ms) = super::now_ms() else {
        return false;
    };
    let candidate = state
        .authority
        .borrow()
        .as_ref()
        .filter(|authority| {
            authority.effect.vault_generation == generation
                && authority.effect.execution_revision == revision
                && authority.presentable(wall_ms, Instant::now())
        })
        .map(|authority| authority.effect.clone());
    let Some(candidate) = candidate else {
        return false;
    };
    if !callback_owner_is_current(app, &candidate) {
        return false;
    }
    // Do not hold the publication borrow while checking another owner's lock.
    let Ok(wall_ms) = super::now_ms() else {
        return false;
    };
    state.authority.borrow().as_ref().is_some_and(|authority| {
        !state.lifecycle.is_revoked()
            && same_revision(&authority.effect, &candidate)
            && authority.presentable(wall_ms, Instant::now())
    })
}

fn same_revision(current: &CommittedFocusEffect, candidate: &CommittedFocusEffect) -> bool {
    current.matches_vault_authority(&candidate.vault_id, candidate.ownership_generation)
        && current.vault_generation == candidate.vault_generation
        && current.execution_revision == candidate.execution_revision
        && current.run_id == candidate.run_id
        && current.segment_id == candidate.segment_id
        && current.mode == candidate.mode
        && current.phase == candidate.phase
        && current.phase_deadline_ms == candidate.phase_deadline_ms
        && current.event_deadline_ms == candidate.event_deadline_ms
}

/// Monitor repair follows the same phase identity across heartbeat revisions.
#[cfg(desktop)]
pub(crate) fn overlay_context_is_current<R: Runtime>(
    app: &tauri::AppHandle<R>,
    context: &super::FocusNativeContext,
) -> bool {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return false;
    };
    if state.lifecycle.is_revoked() {
        return false;
    }
    let Ok(wall_ms) = super::now_ms() else {
        return false;
    };
    let candidate = state
        .authority
        .borrow()
        .as_ref()
        .filter(|authority| {
            authority.effect.vault_generation == context.vault_generation
                && authority.effect.run_id == context.run_id
                && authority.effect.segment_id == context.segment_id
                && authority.effect.mode == context.mode
                && authority.effect.phase == context.phase
                && (!matches!(
                    authority.effect.mode,
                    FocusMode::Running
                        | FocusMode::ManualPause
                        | FocusMode::IdlePause
                        | FocusMode::Suspended
                ) || authority.current(wall_ms, Instant::now()).is_some())
        })
        .map(|authority| authority.effect.clone());
    let Some(candidate) = candidate else {
        return false;
    };
    if !callback_owner_is_current(app, &candidate) {
        return false;
    }
    let Ok(wall_ms) = super::now_ms() else {
        return false;
    };
    state.authority.borrow().as_ref().is_some_and(|authority| {
        !state.lifecycle.is_revoked()
            && same_revision(&authority.effect, &candidate)
            && (!matches!(
                authority.effect.mode,
                FocusMode::Running
                    | FocusMode::ManualPause
                    | FocusMode::IdlePause
                    | FocusMode::Suspended
            ) || authority.current(wall_ms, Instant::now()).is_some())
    })
}

fn callback_owner_is_current<R: Runtime>(
    app: &tauri::AppHandle<R>,
    effect: &CommittedFocusEffect,
) -> bool {
    let Some(ownership) = app.try_state::<crate::vault::ownership::VaultOwnershipManager>() else {
        return false;
    };
    match ownership.cached_status(&effect.vault_id) {
        Ok(Some(status)) => status.can_write && status.generation == effect.ownership_generation,
        Ok(None) => false,
        Err(error) => {
            eprintln!("Native Focus callback ownership is unavailable: {error}");
            false
        }
    }
}

/// Check a queued Music effect against current cached phase authority without vault IO.
/// Heartbeats can renew the phase, but a different phase/mode or an expired original
/// lease cannot authorize an older queued start or asynchronous source resolution.
#[cfg(not(target_os = "ios"))]
pub(crate) fn music_delivery_is_current<R: Runtime>(
    app: &tauri::AppHandle<R>,
    candidate: &CommittedFocusEffect,
    expires_at: Instant,
) -> bool {
    let Some(state) = app.try_state::<FocusRuntimeState>() else {
        return false;
    };
    let Ok(now) = super::now_ms() else {
        return false;
    };
    if state.lifecycle.is_revoked()
        || Instant::now() >= expires_at
        || now >= candidate.effective_valid_until_ms()
    {
        return false;
    }
    let current = state
        .authority
        .borrow()
        .as_ref()
        .and_then(|authority| authority.current(now, Instant::now()))
        .cloned();
    let Some(current) = current else {
        return false;
    };
    let matches = allows_music_delivery(&current, candidate);
    matches
        && callback_owner_is_current(app, &current)
        && !state.lifecycle.is_revoked()
        && state.authority.borrow().as_ref().is_some_and(|authority| {
            same_revision(&authority.effect, &current)
                && authority.current(now, Instant::now()).is_some()
        })
}

/// Capture the owner's actual monotonic deadline, rather than deriving a new
/// lease from wall time after clock rollback or a later heartbeat.
#[cfg(not(target_os = "ios"))]
pub(crate) fn capture_music_delivery<R: Runtime>(
    app: &tauri::AppHandle<R>,
) -> Option<(CommittedFocusEffect, Instant)> {
    let state = app.try_state::<FocusRuntimeState>()?;
    let now = super::now_ms().ok()?;
    let authority = state.authority.borrow().as_ref()?.clone();
    let candidate = authority.current(now, Instant::now())?.clone();
    music_delivery_is_current(app, &candidate, authority.expires_at)
        .then_some((candidate, authority.expires_at))
}

#[cfg(any(not(target_os = "ios"), test))]
fn allows_music_delivery(current: &CommittedFocusEffect, candidate: &CommittedFocusEffect) -> bool {
    same_revision(current, candidate) && allows_existing_lease(current, candidate)
}

/// Active transitions retain the consumer's original bounded lease until reconciliation.
/// A stop, another run, or a changed vault revokes it immediately. Keeping the
/// original lease avoids imposing a pause before the new phase's Music policy runs.
pub(crate) fn allows_existing_lease(
    current: &CommittedFocusEffect,
    candidate: &CommittedFocusEffect,
) -> bool {
    current.matches_vault_authority(&candidate.vault_id, candidate.ownership_generation)
        && current.vault_generation == candidate.vault_generation
        && current.execution_revision >= candidate.execution_revision
        && current.run_id == candidate.run_id
        && current.run_id.is_some()
        && matches!(
            current.mode,
            FocusMode::Running
                | FocusMode::ManualPause
                | FocusMode::IdlePause
                | FocusMode::Suspended
        )
}

#[cfg(test)]
mod tests;
