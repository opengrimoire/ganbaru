use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaControlsUpdate {
    status: String,
    title: Option<String>,
    source_kind_label: Option<String>,
    artwork_url: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    can_seek: bool,
    position_ms: u64,
    duration_ms: Option<u64>,
    volume: f64,
    muted: bool,
    rate: f64,
    shuffle_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MusicHardwareControlPayload {
    action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    position_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shuffle_enabled: Option<bool>,
}

pub fn update_media_controls(update: MediaControlsUpdate) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    linux::update_mpris_state(update);
    #[cfg(target_os = "windows")]
    windows::update_smtc_state(update);
    Ok(())
}

pub fn setup_media_controls(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    linux::setup_mpris(app);
    #[cfg(target_os = "windows")]
    windows::setup_smtc(app);
    Ok(())
}

const MIN_PLAYBACK_RATE: f64 = 0.25;
const MAX_PLAYBACK_RATE: f64 = 2.0;

#[cfg(target_os = "linux")]
fn clamp_unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn clamp_playback_rate(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(MIN_PLAYBACK_RATE, MAX_PLAYBACK_RATE)
    } else {
        1.0
    }
}

fn emit_control(app: &tauri::AppHandle, payload: MusicHardwareControlPayload) {
    use crate::music::session::{PlaybackOrder, SessionIntent, dispatch_control};
    let intent = match payload.action {
        "play" => SessionIntent::Play,
        "pause" => SessionIntent::Pause,
        "playPause" => SessionIntent::Toggle,
        "stop" => SessionIntent::Stop,
        "previousTrack" => SessionIntent::Previous,
        "nextTrack" => SessionIntent::Next,
        "seekBy" => SessionIntent::SeekBy {
            delta_ms: payload.delta_ms.unwrap_or(0),
        },
        "seekTo" => SessionIntent::Seek {
            position_ms: payload.position_ms.unwrap_or(0),
        },
        "setVolume" => SessionIntent::Volume {
            volume: payload.volume.unwrap_or(1.0),
        },
        "setRate" => SessionIntent::Rate {
            rate: payload.rate.unwrap_or(1.0),
        },
        "setShuffle" => SessionIntent::Order {
            order: if payload.shuffle_enabled == Some(true) {
                PlaybackOrder::Shuffle
            } else {
                PlaybackOrder::InOrder
            },
        },
        _ => {
            eprintln!("unknown native media action: {}", payload.action);
            return;
        }
    };
    if let Err(error) = dispatch_control(app, intent) {
        eprintln!("native media control failed: {error}");
    }
}

/// Projects accepted native state to operating-system transport controls.
pub(crate) fn publish_session(
    session: &crate::music::session::SessionProjection,
) -> Result<(), String> {
    use crate::music::session::{PlaybackOrder, SourceKind};
    update_media_controls(MediaControlsUpdate {
        status: session.status.as_str().into(),
        title: session
            .current_source
            .as_ref()
            .map(|source| source.title.clone()),
        source_kind_label: session.current_source.as_ref().map(|source| {
            match source.kind {
                SourceKind::LocalFile => "Local file",
                SourceKind::YoutubeVideo | SourceKind::YoutubePlaylist => "YouTube",
            }
            .into()
        }),
        artwork_url: None,
        can_play_pause: session.current_source.is_some(),
        can_previous: session.can_previous,
        can_next: session.can_next,
        can_seek: session.current_source.is_some(),
        position_ms: session.position_ms,
        duration_ms: session.duration_ms,
        volume: session.volume,
        muted: session.muted,
        rate: session.rate,
        shuffle_enabled: session.order != PlaybackOrder::InOrder,
    })
}

#[cfg(any(target_os = "windows", test))]
fn run_windows_media_callback(callback: impl FnOnce()) -> bool {
    // windows-rs dispatches these closures through a non-unwind COM ABI. Keep
    // every application and listener call inside this barrier. A panic payload
    // may itself panic when dropped, so leak that payload on this exceptional
    // path instead of risking a second unwind through the COM thunk.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(callback)) {
        Ok(()) => true,
        Err(payload) => {
            std::mem::forget(payload);
            false
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn control(action: &'static str) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        delta_ms: None,
        position_ms: None,
        volume: None,
        rate: None,
        shuffle_enabled: None,
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn control_delta(action: &'static str, delta_ms: i64) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        delta_ms: Some(delta_ms),
        ..control(action)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn control_position(action: &'static str, position_ms: u64) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        position_ms: Some(position_ms),
        ..control(action)
    }
}

#[cfg(target_os = "linux")]
fn control_volume(action: &'static str, volume: f64) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        volume: Some(volume),
        ..control(action)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn control_rate(action: &'static str, rate: f64) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        rate: Some(rate),
        ..control(action)
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn control_shuffle(action: &'static str, shuffle_enabled: bool) -> MusicHardwareControlPayload {
    MusicHardwareControlPayload {
        action,
        shuffle_enabled: Some(shuffle_enabled),
        ..control(action)
    }
}

#[cfg(target_os = "linux")]
fn ms_to_us_i64(ms: u64) -> i64 {
    ms.saturating_mul(1_000).min(i64::MAX as u64) as i64
}

#[cfg(target_os = "linux")]
fn us_to_ms_i64(us: i64) -> i64 {
    us / 1_000
}

#[cfg(target_os = "linux")]
fn us_to_ms_u64(us: i64) -> u64 {
    if us <= 0 { 0 } else { (us as u64) / 1_000 }
}

#[cfg(target_os = "windows")]
fn ms_to_windows_ticks(ms: u64) -> i64 {
    ms.saturating_mul(10_000).min(i64::MAX as u64) as i64
}

#[cfg(target_os = "windows")]
fn windows_ticks_to_ms_u64(ticks: i64) -> u64 {
    if ticks <= 0 {
        0
    } else {
        (ticks as u64) / 10_000
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_unit_rejects_invalid_values() {
        assert_eq!(clamp_unit(f64::NAN), 0.0);
        assert_eq!(clamp_unit(-0.5), 0.0);
        assert_eq!(clamp_unit(1.5), 1.0);
    }

    #[test]
    fn playback_rate_stays_inside_supported_bounds() {
        assert_eq!(clamp_playback_rate(f64::NAN), 1.0);
        assert_eq!(clamp_playback_rate(0.1), MIN_PLAYBACK_RATE);
        assert_eq!(clamp_playback_rate(3.0), MAX_PLAYBACK_RATE);
    }

    #[test]
    fn mpris_time_conversion_saturates() {
        assert_eq!(ms_to_us_i64(123), 123_000);
        assert_eq!(ms_to_us_i64(u64::MAX), i64::MAX);
        assert_eq!(us_to_ms_i64(-1_500), -1);
        assert_eq!(us_to_ms_u64(-1), 0);
    }

    #[test]
    fn windows_media_callback_barrier_contains_panics() {
        struct PanicOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);

        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
                panic!("panic payload drop fixture");
            }
        }

        assert!(!run_windows_media_callback(|| panic!("callback fixture")));
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let payload = PanicOnDrop(std::sync::Arc::clone(&dropped));
        assert!(!run_windows_media_callback(|| std::panic::panic_any(
            payload
        )));
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));
        assert!(run_windows_media_callback(|| {}));
    }
}
