//! Windows system media transport integration.

use super::{
    MediaControlsUpdate, MusicHardwareControlPayload, clamp_playback_rate, control, control_delta,
    control_position, control_rate, control_shuffle, emit_control, ms_to_windows_ticks,
    run_windows_media_callback, windows_ticks_to_ms_u64,
};
use std::sync::{LazyLock, Mutex};
use tauri::Manager;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    PlaybackRateChangeRequestedEventArgs, ShuffleEnabledChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::core::{HSTRING, factory};

const SEEK_STEP_MS: i64 = 10_000;

#[derive(Debug, Clone, PartialEq)]
struct SmtcState {
    status: String,
    title: Option<String>,
    source_kind_label: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    can_seek: bool,
    position_ticks: i64,
    duration_ticks: Option<i64>,
    rate: f64,
    shuffle_enabled: bool,
}

struct SmtcSession {
    controls: SystemMediaTransportControls,
    _button_token: i64,
    _position_token: Option<i64>,
    _rate_token: Option<i64>,
    _shuffle_token: Option<i64>,
}

static SMTC_STATE: LazyLock<Mutex<SmtcState>> = LazyLock::new(|| Mutex::new(SmtcState::default()));
static SMTC_SESSION: LazyLock<Mutex<Option<SmtcSession>>> = LazyLock::new(|| Mutex::new(None));

impl Default for SmtcState {
    fn default() -> Self {
        Self {
            status: "idle".to_string(),
            title: None,
            source_kind_label: None,
            can_play_pause: false,
            can_previous: false,
            can_next: false,
            can_seek: false,
            position_ticks: 0,
            duration_ticks: None,
            rate: 1.0,
            shuffle_enabled: true,
        }
    }
}

impl SmtcState {
    fn from_update(update: MediaControlsUpdate) -> Self {
        let MediaControlsUpdate {
            status,
            title,
            source_kind_label,
            artwork_url,
            can_play_pause,
            can_previous,
            can_next,
            can_seek,
            position_ms,
            duration_ms,
            volume,
            muted,
            rate,
            shuffle_enabled,
        } = update;
        let _ = (artwork_url, volume, muted);

        Self {
            status,
            title,
            source_kind_label,
            can_play_pause,
            can_previous,
            can_next,
            can_seek,
            position_ticks: ms_to_windows_ticks(position_ms),
            duration_ticks: duration_ms.map(ms_to_windows_ticks),
            rate: clamp_playback_rate(rate),
            shuffle_enabled,
        }
    }

    fn has_track(&self) -> bool {
        self.title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty())
    }
}

pub(super) fn setup_smtc(app: &tauri::AppHandle) {
    if SMTC_SESSION
        .lock()
        .map(|session| session.is_some())
        .unwrap_or(true)
    {
        return;
    }

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let hwnd = match window.hwnd() {
        Ok(hwnd) => hwnd,
        Err(err) => {
            eprintln!("failed to read Windows window handle for media controls: {err}");
            return;
        }
    };
    let controls = match system_media_transport_controls_for_window(hwnd) {
        Ok(controls) => controls,
        Err(err) => {
            eprintln!("failed to initialize Windows media controls: {err}");
            return;
        }
    };
    if let Err(err) = configure_supported_buttons(&controls) {
        eprintln!("failed to configure Windows media controls: {err}");
        return;
    }

    let button_token = match register_button_handler(&controls, app.clone()) {
        Ok(token) => token,
        Err(err) => {
            eprintln!("failed to listen for Windows media button events: {err}");
            return;
        }
    };
    let position_token = register_position_handler(&controls, app.clone())
        .map_err(|err| {
            eprintln!("failed to listen for Windows media seek events: {err}");
            err
        })
        .ok();
    let rate_token = register_rate_handler(&controls, app.clone())
        .map_err(|err| {
            eprintln!("failed to listen for Windows media rate events: {err}");
            err
        })
        .ok();
    let shuffle_token = register_shuffle_handler(&controls, app.clone())
        .map_err(|err| {
            eprintln!("failed to listen for Windows media shuffle events: {err}");
            err
        })
        .ok();

    if let Ok(mut session) = SMTC_SESSION.lock() {
        *session = Some(SmtcSession {
            controls: controls.clone(),
            _button_token: button_token,
            _position_token: position_token,
            _rate_token: rate_token,
            _shuffle_token: shuffle_token,
        });
    }

    if let Err(err) = apply_smtc_state(&controls, &current_state()) {
        eprintln!("failed to publish Windows media controls state: {err}");
    }
}

pub(super) fn update_smtc_state(update: MediaControlsUpdate) {
    let state = SmtcState::from_update(update);
    {
        let Ok(mut stored) = SMTC_STATE.lock() else {
            return;
        };
        *stored = state.clone();
    }

    let controls = {
        let Ok(session) = SMTC_SESSION.lock() else {
            return;
        };
        session.as_ref().map(|session| session.controls.clone())
    };
    let Some(controls) = controls else {
        return;
    };

    if let Err(err) = apply_smtc_state(&controls, &state) {
        eprintln!("failed to publish Windows media controls state: {err}");
    }
}

fn current_state() -> SmtcState {
    SMTC_STATE
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|_| SmtcState::default())
}

fn system_media_transport_controls_for_window(
    hwnd: HWND,
) -> windows::core::Result<SystemMediaTransportControls> {
    let interop: ISystemMediaTransportControlsInterop =
        factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>()?;
    // SAFETY: The caller obtains `hwnd` from the live Tauri main window in
    // this process. `interop` is the matching WinRT activation factory, so
    // its typed wrapper supplies the required interface ID and output slot.
    unsafe { interop.GetForWindow(hwnd) }
}

fn configure_supported_buttons(
    controls: &SystemMediaTransportControls,
) -> windows::core::Result<()> {
    controls.SetIsEnabled(false)?;
    controls.SetIsPlayEnabled(false)?;
    controls.SetIsPauseEnabled(false)?;
    controls.SetIsStopEnabled(false)?;
    controls.SetIsPreviousEnabled(false)?;
    controls.SetIsNextEnabled(false)?;
    controls.SetIsFastForwardEnabled(false)?;
    controls.SetIsRewindEnabled(false)?;
    Ok(())
}

fn register_button_handler(
    controls: &SystemMediaTransportControls,
    app: tauri::AppHandle,
) -> windows::core::Result<i64> {
    controls.ButtonPressed(&TypedEventHandler::<
        SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
    >::new(move |_sender, args| {
        run_windows_media_callback(|| {
            if let Some(args) = args.as_ref() {
                if let Ok(button) = args.Button() {
                    if let Some(payload) = payload_for_button(button) {
                        emit_control(&app, payload);
                    }
                }
            }
        });
        Ok(())
    }))
}

fn register_position_handler(
    controls: &SystemMediaTransportControls,
    app: tauri::AppHandle,
) -> windows::core::Result<i64> {
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::<
        SystemMediaTransportControls,
        PlaybackPositionChangeRequestedEventArgs,
    >::new(move |_sender, args| {
        run_windows_media_callback(|| {
            if let Some(args) = args.as_ref() {
                if let Ok(position) = args.RequestedPlaybackPosition() {
                    emit_control(
                        &app,
                        control_position("seekTo", windows_ticks_to_ms_u64(position.Duration)),
                    );
                }
            }
        });
        Ok(())
    }))
}

fn register_rate_handler(
    controls: &SystemMediaTransportControls,
    app: tauri::AppHandle,
) -> windows::core::Result<i64> {
    controls.PlaybackRateChangeRequested(&TypedEventHandler::<
        SystemMediaTransportControls,
        PlaybackRateChangeRequestedEventArgs,
    >::new(move |_sender, args| {
        run_windows_media_callback(|| {
            if let Some(args) = args.as_ref() {
                if let Ok(rate) = args.RequestedPlaybackRate() {
                    emit_control(&app, control_rate("setRate", clamp_playback_rate(rate)));
                }
            }
        });
        Ok(())
    }))
}

fn register_shuffle_handler(
    controls: &SystemMediaTransportControls,
    app: tauri::AppHandle,
) -> windows::core::Result<i64> {
    controls.ShuffleEnabledChangeRequested(&TypedEventHandler::<
        SystemMediaTransportControls,
        ShuffleEnabledChangeRequestedEventArgs,
    >::new(move |_sender, args| {
        run_windows_media_callback(|| {
            if let Some(args) = args.as_ref() {
                if let Ok(shuffle_enabled) = args.RequestedShuffleEnabled() {
                    emit_control(&app, control_shuffle("setShuffle", shuffle_enabled));
                }
            }
        });
        Ok(())
    }))
}

fn payload_for_button(
    button: SystemMediaTransportControlsButton,
) -> Option<MusicHardwareControlPayload> {
    if button == SystemMediaTransportControlsButton::Play {
        Some(control("play"))
    } else if button == SystemMediaTransportControlsButton::Pause {
        Some(control("pause"))
    } else if button == SystemMediaTransportControlsButton::Stop {
        Some(control("stop"))
    } else if button == SystemMediaTransportControlsButton::Next {
        Some(control("nextTrack"))
    } else if button == SystemMediaTransportControlsButton::Previous {
        Some(control("previousTrack"))
    } else if button == SystemMediaTransportControlsButton::FastForward {
        Some(control_delta("seekBy", SEEK_STEP_MS))
    } else if button == SystemMediaTransportControlsButton::Rewind {
        Some(control_delta("seekBy", -SEEK_STEP_MS))
    } else {
        None
    }
}

fn apply_smtc_state(
    controls: &SystemMediaTransportControls,
    state: &SmtcState,
) -> windows::core::Result<()> {
    let has_track = state.has_track();
    controls.SetPlaybackStatus(smtc_playback_status(state))?;
    controls.SetIsEnabled(has_track)?;
    controls.SetIsPlayEnabled(has_track && state.can_play_pause)?;
    controls.SetIsPauseEnabled(has_track && state.can_play_pause)?;
    controls.SetIsStopEnabled(has_track)?;
    controls.SetIsPreviousEnabled(has_track && state.can_previous)?;
    controls.SetIsNextEnabled(has_track && state.can_next)?;
    controls.SetIsFastForwardEnabled(has_track && state.can_seek)?;
    controls.SetIsRewindEnabled(has_track && state.can_seek)?;
    controls.SetShuffleEnabled(state.shuffle_enabled)?;
    controls.SetPlaybackRate(state.rate)?;
    update_display(controls, state)?;
    update_timeline(controls, state)?;
    Ok(())
}

fn update_display(
    controls: &SystemMediaTransportControls,
    state: &SmtcState,
) -> windows::core::Result<()> {
    let display = controls.DisplayUpdater()?;
    if !state.has_track() {
        display.ClearAll()?;
        display.Update()?;
        return Ok(());
    }

    display.SetType(MediaPlaybackType::Music)?;
    display.SetAppMediaId(&HSTRING::from("ganbaru-ai"))?;
    let properties = display.MusicProperties()?;
    properties.SetTitle(&HSTRING::from(
        state.title.as_deref().unwrap_or("Ganbaru AI").trim(),
    ))?;
    properties.SetArtist(&HSTRING::from(
        state
            .source_kind_label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .unwrap_or("Ganbaru AI"),
    ))?;
    display.Update()?;
    Ok(())
}

fn update_timeline(
    controls: &SystemMediaTransportControls,
    state: &SmtcState,
) -> windows::core::Result<()> {
    let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
    let position = state.position_ticks.max(0);
    let duration = state
        .duration_ticks
        .unwrap_or(position)
        .max(position)
        .max(0);
    timeline.SetStartTime(time_span(0))?;
    timeline.SetEndTime(time_span(duration))?;
    timeline.SetMinSeekTime(time_span(0))?;
    timeline.SetMaxSeekTime(time_span(duration))?;
    timeline.SetPosition(time_span(position.min(duration)))?;
    controls.UpdateTimelineProperties(&timeline)?;
    Ok(())
}

fn smtc_playback_status(state: &SmtcState) -> MediaPlaybackStatus {
    if !state.has_track() {
        return MediaPlaybackStatus::Closed;
    }

    match state.status.as_str() {
        "playing" => MediaPlaybackStatus::Playing,
        "loading" => MediaPlaybackStatus::Changing,
        "paused" | "ready" => MediaPlaybackStatus::Paused,
        _ => MediaPlaybackStatus::Stopped,
    }
}

fn time_span(duration: i64) -> TimeSpan {
    TimeSpan { Duration: duration }
}
