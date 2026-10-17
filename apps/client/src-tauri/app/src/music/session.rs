//! One application-owned music session above desktop, Android, and browser adapters. Session
//! models, policy, persistence, and queue loading come from `ganbaru-music-library`.

mod backend;
pub(crate) mod runtime;
mod subscriptions;

use ganbaru_music_library::session::{models, persistence, policy, queue};

pub(crate) use runtime::{
    reconcile_committed_focus, resume_after_vault_handoff, setup, stop_for_vault_handoff,
};

#[cfg(desktop)]
pub(crate) use runtime::dispatch_control;

#[cfg(desktop)]
pub(crate) use models::{PlaybackOrder, SessionIntent, SessionProjection, SourceKind};
