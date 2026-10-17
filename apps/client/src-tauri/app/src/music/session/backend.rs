//! Platform effects contain playback mechanisms, never queue decisions.

use super::models::*;
#[cfg(desktop)]
use tauri::Manager;

#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
mod delivery;

#[cfg(target_os = "android")]
pub(super) use android::attach;
#[cfg(target_os = "android")]
pub(super) use android::{is_active, restart_for_play};

/// Failed media and uncertain transport have different queue recovery semantics.
#[derive(Debug)]
pub(super) enum Failure {
    #[cfg(any(desktop, test))]
    Source(String),
    Interrupted(String),
}

impl Failure {
    pub fn is_source_failure(&self) -> bool {
        match self {
            #[cfg(any(desktop, test))]
            Self::Source(_) => true,
            Self::Interrupted(_) => false,
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(any(desktop, test))]
            Self::Source(message) => formatter.write_str(message),
            Self::Interrupted(message) => formatter.write_str(message),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self::Interrupted(message)
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Self::Interrupted(message.into())
    }
}

#[cfg(desktop)]
impl From<crate::music::player::MediaPlayerError> for Failure {
    fn from(error: crate::music::player::MediaPlayerError) -> Self {
        if error.is_source_failure() {
            Self::Source(error.message)
        } else {
            Self::Interrupted(error.message)
        }
    }
}

/// Applies effects only after durable transition success.
pub(super) async fn apply(
    app: &tauri::AppHandle,
    backend: Option<SessionBackend>,
    effect: SessionEffect,
    #[cfg(not(target_os = "ios"))] authority: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), Failure> {
    if backend == Some(SessionBackend::Browser) {
        if matches!(effect, SessionEffect::Load { .. }) {
            stop_native(
                app,
                effect_generation(&effect),
                #[cfg(not(target_os = "ios"))]
                authority.clone(),
            )
            .await?;
        }
        return Ok(());
    }
    #[cfg(desktop)]
    {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::music::player::apply_session_effect(
                app.state::<crate::music::player::MediaPlayerState>()
                    .inner(),
                &effect,
                authority,
            )
            .map(|_| ())
            .map_err(Failure::from)
        })
        .await
        .map_err(|error| Failure::Interrupted(error.to_string()))?
    }
    #[cfg(target_os = "android")]
    {
        android::apply(effect, authority)
            .await
            .map_err(Failure::from)
    }
    #[cfg(target_os = "ios")]
    {
        let _ = effect;
        Err("Native Music sessions are not supported on iOS".into())
    }
}

pub(super) fn effect_generation(effect: &SessionEffect) -> u64 {
    match effect {
        SessionEffect::Load { generation, .. }
        | SessionEffect::Play { generation }
        | SessionEffect::Pause { generation }
        | SessionEffect::Stop { generation }
        | SessionEffect::Seek { generation, .. }
        | SessionEffect::Settings { generation, .. } => *generation,
    }
}

async fn stop_native(
    app: &tauri::AppHandle,
    generation: u64,
    #[cfg(not(target_os = "ios"))] authority: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), Failure> {
    #[cfg(desktop)]
    {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::music::player::apply_session_effect(
                app.state::<crate::music::player::MediaPlayerState>()
                    .inner(),
                &SessionEffect::Stop { generation },
                authority,
            )
            .map(|_| ())
            .map_err(Failure::from)
        })
        .await
        .map_err(|error| Failure::Interrupted(error.to_string()))?
    }
    #[cfg(target_os = "android")]
    {
        let _ = app;
        android::apply(SessionEffect::Stop { generation }, authority)
            .await
            .map_err(Failure::from)
    }
    #[cfg(target_os = "ios")]
    {
        let _ = (app, generation);
        Ok(())
    }
}

pub(super) async fn observe(
    app: &tauri::AppHandle,
    session_id: &str,
    generation: u64,
    sequence: u64,
) -> Result<Option<SessionObservation>, String> {
    #[cfg(desktop)]
    {
        let app = app.clone();
        let snapshot = tauri::async_runtime::spawn_blocking(move || {
            crate::music::player::session_snapshot(
                app.state::<crate::music::player::MediaPlayerState>()
                    .inner(),
            )
            .map_err(|error| error.message)
        })
        .await
        .map_err(|error| error.to_string())??;
        let Some(source_identity) = snapshot.source_identity else {
            return Ok(None);
        };
        use crate::music::player::PlayerStatus;
        let status = match snapshot.status {
            PlayerStatus::Idle => SessionStatus::Idle,
            PlayerStatus::Ready => SessionStatus::Ready,
            PlayerStatus::Playing => SessionStatus::Playing,
            PlayerStatus::Paused => SessionStatus::Paused,
            PlayerStatus::Ended => SessionStatus::Ended,
            PlayerStatus::Error => SessionStatus::Error,
        };
        Ok(Some(SessionObservation {
            session_id: session_id.into(),
            generation,
            sequence,
            source_identity,
            status,
            position_ms: snapshot.position_ms,
            duration_ms: snapshot.duration_ms,
            error: snapshot.error,
        }))
    }
    #[cfg(mobile)]
    {
        let _ = (app, session_id, generation, sequence);
        Ok(None)
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    #[test]
    fn only_proven_source_failures_allow_failed_track_advancement() {
        for code in ["invalidSource", "decodeFailed"] {
            let failure = Failure::from(crate::music::player::MediaPlayerError {
                code: code.into(),
                message: format!("context for {code}"),
            });
            assert!(failure.is_source_failure());
            assert_eq!(failure.to_string(), format!("context for {code}"));
        }
        for code in [
            "backendUnavailable",
            "audioDevice",
            "seekFailed",
            "backendThread",
            "backendBusy",
            "backendTimeout",
            "deliveryRevoked",
            "noPreparedSource",
            "unknown",
        ] {
            let failure = Failure::from(crate::music::player::MediaPlayerError {
                code: code.into(),
                message: format!("context for {code}"),
            });
            assert!(!failure.is_source_failure(), "{code}");
            assert_eq!(failure.to_string(), format!("context for {code}"));
        }
    }
}
