use serde::Deserialize;
use std::sync::{
    LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    AppHandle, Emitter,
    image::Image,
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

const ICON_SIZE: u32 = 32;
const RING_WIDTH: f64 = 3.5;
const RING_REMAINING_COLOR: (u8, u8, u8) = (240, 240, 240);
const RING_EMPTY_COLOR: (u8, u8, u8) = (90, 90, 90);
const PAUSED_PULSE_FRAME_COUNT: u8 = 22;
const PAUSED_PULSE_AMOUNTS: [f64; 22] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.067, 0.25, 0.5, 0.75, 0.933, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.933,
    0.75, 0.5, 0.25, 0.067, 0.0,
];
const MUSIC_TRAY_STATUS_MAX_CHARS: usize = 25;

#[derive(Debug, Clone)]
struct PomodoroTrayState {
    phase: String,
    remaining_seconds: u32,
    total_seconds: u32,
    is_running: bool,
    is_active: bool,
    can_pause_resume: bool,
    can_add_focus_time: bool,
    paused_pulse_frame: Option<u8>,
    native_context: Option<crate::pomodoro::FocusNativeContext>,
}

#[derive(Debug, Clone)]
struct MusicTrayState {
    status: String,
    title: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    context_label: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroTrayUpdate {
    phase: String,
    remaining_seconds: u32,
    total_seconds: u32,
    is_running: bool,
    is_active: bool,
    can_pause_resume: bool,
    can_add_focus_time: bool,
    paused_pulse_frame: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicTrayUpdate {
    status: String,
    title: Option<String>,
    can_play_pause: bool,
    can_previous: bool,
    can_next: bool,
    context_label: Option<String>,
}

#[derive(Debug, Clone)]
struct TrayState {
    pomodoro: PomodoroTrayState,
    music: MusicTrayState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuShape {
    pomodoro_active: bool,
    pomodoro_phase_id: u8,
    pomodoro_running: bool,
    pomodoro_can_pause_resume: bool,
    pomodoro_can_add_focus_time: bool,
    music_status: String,
    music_title: Option<String>,
    music_can_play_pause: bool,
    music_can_previous: bool,
    music_can_next: bool,
    music_context_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayIconKey {
    Empty,
    Progress(u8),
    PausedProgress { step: u8, frame: u8 },
}

static TRAY_STATE: LazyLock<Mutex<TrayState>> = LazyLock::new(|| {
    Mutex::new(TrayState {
        pomodoro: PomodoroTrayState {
            phase: "idle".to_string(),
            remaining_seconds: 0,
            total_seconds: 0,
            is_running: false,
            is_active: false,
            can_pause_resume: false,
            can_add_focus_time: false,
            paused_pulse_frame: None,
            native_context: None,
        },
        music: MusicTrayState {
            status: "idle".to_string(),
            title: None,
            can_play_pause: false,
            can_previous: false,
            can_next: false,
            context_label: None,
        },
    })
});

/// Track last icon state to avoid unnecessary set_icon calls.
static LAST_ICON_KEY: Mutex<TrayIconKey> = Mutex::new(TrayIconKey::Empty);
static LAST_MENU_SHAPE: LazyLock<Mutex<Option<MenuShape>>> = LazyLock::new(|| Mutex::new(None));
static POMODORO_STATUS_ITEM: Mutex<Option<MenuItem<tauri::Wry>>> = Mutex::new(None);
static MUSIC_STATUS_ITEM: Mutex<Option<MenuItem<tauri::Wry>>> = Mutex::new(None);
static MUSIC_TRAY_REFRESH_QUEUED: AtomicBool = AtomicBool::new(false);

fn ring_progress_color(paused_pulse_frame: Option<u8>) -> (u8, u8, u8) {
    let Some(frame) = paused_pulse_frame else {
        return RING_REMAINING_COLOR;
    };
    let amount = PAUSED_PULSE_AMOUNTS[(frame % PAUSED_PULSE_FRAME_COUNT) as usize];
    let mix = |from: u8, to: u8| -> u8 {
        (from as f64 + (to as f64 - from as f64) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        mix(RING_REMAINING_COLOR.0, RING_EMPTY_COLOR.0),
        mix(RING_REMAINING_COLOR.1, RING_EMPTY_COLOR.1),
        mix(RING_REMAINING_COLOR.2, RING_EMPTY_COLOR.2),
    )
}

fn normalized_paused_pulse_frame(frame: Option<u8>) -> Option<u8> {
    frame.map(|value| value % PAUSED_PULSE_FRAME_COUNT)
}

/// Render a progress ring icon as raw RGBA pixels.
fn render_progress_icon(progress: f64, active: bool) -> Vec<u8> {
    render_progress_icon_with_pause(progress, active, None)
}

fn render_progress_icon_with_pause(
    progress: f64,
    active: bool,
    paused_pulse_frame: Option<u8>,
) -> Vec<u8> {
    let size = ICON_SIZE as usize;
    let mut pixels = vec![0u8; size * size * 4];
    let center = size as f64 / 2.0;
    let outer_r = center - 0.5;
    let inner_r = outer_r - RING_WIDTH;

    let progress_color = ring_progress_color(paused_pulse_frame);
    let progress = normalize_progress(progress);

    for y in 0..size {
        for x in 0..size {
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist >= inner_r - 0.5 && dist <= outer_r + 0.5 {
                let aa_outer = (outer_r + 0.5 - dist).clamp(0.0, 1.0);
                let aa_inner = (dist - (inner_r - 0.5)).clamp(0.0, 1.0);
                let alpha = (aa_outer * aa_inner * 255.0) as u8;

                let (r, g, b) = if active {
                    let angle = (-dx).atan2(-dy);
                    let normalized = (angle + std::f64::consts::PI) / (2.0 * std::f64::consts::PI);
                    let normalized = (normalized + 0.5) % 1.0;

                    if normalized <= progress {
                        RING_EMPTY_COLOR
                    } else {
                        progress_color
                    }
                } else {
                    RING_EMPTY_COLOR
                };

                let idx = (y * size + x) * 4;
                pixels[idx] = r;
                pixels[idx + 1] = g;
                pixels[idx + 2] = b;
                pixels[idx + 3] = alpha;
            }
        }
    }
    pixels
}

fn normalize_progress(progress: f64) -> f64 {
    if progress.is_finite() {
        progress.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn format_tray_time(total_seconds: u32) -> String {
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

fn phase_id(phase: &str) -> u8 {
    match phase {
        "focus" => 0,
        "short_break" => 1,
        "long_break" => 2,
        _ => 3,
    }
}

fn phase_label(phase: &str) -> &'static str {
    match phase {
        "focus" => "Focus",
        "short_break" => "Short break",
        "long_break" => "Long break",
        _ => "Idle",
    }
}

fn phase_advance_label(phase: &str) -> &'static str {
    match phase {
        "focus" => "Go to break now",
        "short_break" | "long_break" => "Start focus now",
        _ => "Go to break now",
    }
}

fn music_status_label(status: &str) -> &'static str {
    match status {
        "playing" => "Playing",
        "paused" => "Paused",
        "loading" => "Loading",
        "ready" => "Ready",
        "ended" => "Ended",
        "error" => "Error",
        _ => "Idle",
    }
}

fn is_pomodoro_active(state: &PomodoroTrayState) -> bool {
    state.is_active
}

fn pomodoro_progress(state: &PomodoroTrayState) -> f64 {
    if state.total_seconds > 0 {
        1.0 - (state.remaining_seconds as f64 / state.total_seconds as f64)
    } else {
        0.0
    }
}

fn tray_icon_key(progress: f64, active: bool, paused_pulse_frame: Option<u8>) -> TrayIconKey {
    let progress = normalize_progress(progress);
    if !active || progress >= 1.0 {
        return TrayIconKey::Empty;
    }

    let step = (progress * 100.0) as u8;
    if let Some(frame) = normalized_paused_pulse_frame(paused_pulse_frame) {
        TrayIconKey::PausedProgress { step, frame }
    } else {
        TrayIconKey::Progress(step)
    }
}

fn pomodoro_status_text(state: &PomodoroTrayState) -> String {
    if is_pomodoro_active(state) {
        format!("{} left", format_tray_time(state.remaining_seconds))
    } else {
        "No active session".to_string()
    }
}

fn music_status_text(state: &MusicTrayState) -> String {
    let title = state
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    if let Some(title) = title {
        return title.to_string();
    }
    if state.can_play_pause || state.status != "idle" {
        music_status_label(&state.status).to_string()
    } else {
        "No music loaded".to_string()
    }
}

fn truncate_tray_menu_label(value: &str, max_chars: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_chars {
        return value.to_string();
    }

    let prefix_chars = max_chars.saturating_sub(3);
    let mut label = value.chars().take(prefix_chars).collect::<String>();
    label = label.trim_end().to_string();
    label.push_str("...");
    label
}

fn music_menu_status_text(state: &MusicTrayState) -> String {
    truncate_tray_menu_label(&music_status_text(state), MUSIC_TRAY_STATUS_MAX_CHARS)
}

fn tray_tooltip(state: &TrayState) -> String {
    let pomodoro = if is_pomodoro_active(&state.pomodoro) {
        format!(
            "{} {}",
            phase_label(&state.pomodoro.phase),
            format_tray_time(state.pomodoro.remaining_seconds)
        )
    } else {
        "No active session".to_string()
    };
    let music = music_status_text(&state.music);
    match state.music.context_label.as_deref() {
        Some(context) => format!("Ganbaru AI\n{pomodoro}\n{music}\n{context}"),
        None => format!("Ganbaru AI\n{pomodoro}\n{music}"),
    }
}

fn menu_shape(state: &TrayState) -> MenuShape {
    MenuShape {
        pomodoro_active: is_pomodoro_active(&state.pomodoro),
        pomodoro_phase_id: phase_id(&state.pomodoro.phase),
        pomodoro_running: state.pomodoro.is_running,
        pomodoro_can_pause_resume: state.pomodoro.can_pause_resume,
        pomodoro_can_add_focus_time: state.pomodoro.can_add_focus_time,
        music_status: state.music.status.clone(),
        music_title: state.music.title.clone(),
        music_can_play_pause: state.music.can_play_pause,
        music_can_previous: state.music.can_previous,
        music_can_next: state.music.can_next,
        music_context_label: state.music.context_label.clone(),
    }
}

fn pomodoro_paused_pulse_frame(state: &PomodoroTrayState) -> Option<u8> {
    if state.phase != "focus" || !state.is_active || state.is_running {
        return None;
    }
    normalized_paused_pulse_frame(state.paused_pulse_frame)
}

fn build_menu(app: &AppHandle, state: &TrayState) -> Result<Menu<tauri::Wry>, String> {
    let pomodoro_status = MenuItemBuilder::with_id("status", pomodoro_status_text(&state.pomodoro))
        .enabled(false)
        .build(app)
        .map_err(|e| e.to_string())?;
    {
        let mut stored = POMODORO_STATUS_ITEM.lock().unwrap();
        *stored = Some(pomodoro_status.clone());
    }

    let music_status =
        MenuItemBuilder::with_id("music_status", music_menu_status_text(&state.music))
            .enabled(false)
            .build(app)
            .map_err(|e| e.to_string())?;
    {
        let mut stored = MUSIC_STATUS_ITEM.lock().unwrap();
        *stored = Some(music_status.clone());
    }

    let pomodoro_active = is_pomodoro_active(&state.pomodoro);
    let pause_resume_label = if state.pomodoro.is_running {
        "Pause focus"
    } else {
        "Resume focus"
    };
    let pause_resume_item = MenuItemBuilder::with_id("pause_resume", pause_resume_label)
        .enabled(state.pomodoro.can_pause_resume)
        .build(app)
        .map_err(|e| e.to_string())?;
    let add_focus_time_item = MenuItemBuilder::with_id("add_focus_time", "Extend focus 3 minutes")
        .enabled(state.pomodoro.can_add_focus_time)
        .build(app)
        .map_err(|e| e.to_string())?;
    let skip_item = MenuItemBuilder::with_id("skip", phase_advance_label(&state.pomodoro.phase))
        .enabled(pomodoro_active)
        .build(app)
        .map_err(|e| e.to_string())?;

    let separator = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let play_pause_label = if state.music.status == "playing" {
        "Pause music"
    } else {
        "Play music"
    };
    let play_pause_item = MenuItemBuilder::with_id("music_play_pause", play_pause_label)
        .enabled(state.music.can_play_pause)
        .build(app)
        .map_err(|e| e.to_string())?;
    let previous_item = MenuItemBuilder::with_id("music_previous", "Previous music")
        .enabled(state.music.can_previous)
        .build(app)
        .map_err(|e| e.to_string())?;
    let next_item = MenuItemBuilder::with_id("music_next", "Next music")
        .enabled(state.music.can_next)
        .build(app)
        .map_err(|e| e.to_string())?;
    let open_music_item = MenuItemBuilder::with_id("music_open", "Open music")
        .build(app)
        .map_err(|e| e.to_string())?;

    let mut builder = MenuBuilder::new(app)
        .item(&pomodoro_status)
        .item(&pause_resume_item)
        .item(&add_focus_time_item)
        .item(&skip_item)
        .item(&separator)
        .item(&music_status)
        .item(&play_pause_item);
    let context_item;
    let inspect_item;
    if let Some(context_label) = state.music.context_label.as_deref() {
        context_item = MenuItemBuilder::with_id(
            "music_context_status",
            truncate_tray_menu_label(context_label, MUSIC_TRAY_STATUS_MAX_CHARS),
        )
        .enabled(false)
        .build(app)
        .map_err(|e| e.to_string())?;
        inspect_item =
            MenuItemBuilder::with_id("music_inspect_assignment", "Inspect soundtrack assignment")
                .build(app)
                .map_err(|e| e.to_string())?;
        builder = builder.item(&context_item).item(&inspect_item);
    }
    builder
        .item(&previous_item)
        .item(&next_item)
        .item(&open_music_item)
        .build()
        .map_err(|e| e.to_string())
}

fn set_tray_icon(tray: &tauri::tray::TrayIcon<tauri::Wry>, pixels: Vec<u8>) -> Result<(), String> {
    let icon = Image::new_owned(pixels, ICON_SIZE, ICON_SIZE);
    tray.set_icon(Some(icon)).map_err(|e| e.to_string())
}

fn apply_tray_state(app: &AppHandle, state: &TrayState) -> Result<(), String> {
    let tray = app.tray_by_id("main").ok_or("No tray icon found")?;

    let progress = pomodoro_progress(&state.pomodoro);
    let active = is_pomodoro_active(&state.pomodoro);
    let paused_pulse_frame = pomodoro_paused_pulse_frame(&state.pomodoro);
    let icon_key = tray_icon_key(progress, active, paused_pulse_frame);
    {
        let mut last = LAST_ICON_KEY.lock().unwrap();
        if *last != icon_key {
            let pixels = render_progress_icon_with_pause(progress, active, paused_pulse_frame);
            set_tray_icon(&tray, pixels)?;
            *last = icon_key;
        }
    }

    tray.set_tooltip(Some(&tray_tooltip(state)))
        .map_err(|e| e.to_string())?;

    {
        let item = POMODORO_STATUS_ITEM.lock().unwrap();
        if let Some(ref status_item) = *item {
            status_item
                .set_text(pomodoro_status_text(&state.pomodoro))
                .map_err(|error| error.to_string())?;
        }
    }
    {
        let item = MUSIC_STATUS_ITEM.lock().unwrap();
        if let Some(ref status_item) = *item {
            status_item
                .set_text(music_menu_status_text(&state.music))
                .map_err(|error| error.to_string())?;
        }
    }

    let shape = menu_shape(state);
    let should_rebuild = LAST_MENU_SHAPE.lock().unwrap().as_ref() != Some(&shape);
    if should_rebuild {
        let menu = build_menu(app, state)?;
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
        *LAST_MENU_SHAPE.lock().unwrap() = Some(shape);
    }

    Ok(())
}

/// Initialize the system tray with a combined Pomodoro and Music menu.
pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let idle_icon = render_progress_icon(0.0, false);
    let icon = Image::new_owned(idle_icon, ICON_SIZE, ICON_SIZE);
    let state = TRAY_STATE.lock().unwrap().clone();
    let menu = build_menu(app, &state).map_err(std::io::Error::other)?;
    {
        let mut last = LAST_ICON_KEY.lock().unwrap();
        *last = TrayIconKey::Empty;
    }

    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Ganbaru AI")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "pause_resume" => {
                let state = TRAY_STATE.lock().unwrap().pomodoro.clone();
                if state.can_pause_resume {
                    dispatch_focus_control(
                        app,
                        if state.is_running {
                            ganbaru_pomodoro::FocusIntent::Pause
                        } else {
                            ganbaru_pomodoro::FocusIntent::Resume
                        },
                        state.native_context,
                    );
                }
            }
            "skip" => {
                dispatch_focus_control(
                    app,
                    ganbaru_pomodoro::FocusIntent::Advance,
                    TRAY_STATE.lock().unwrap().pomodoro.native_context.clone(),
                );
            }
            "add_focus_time" => {
                dispatch_focus_control(
                    app,
                    ganbaru_pomodoro::FocusIntent::ExtendFocus { seconds: 180 },
                    TRAY_STATE.lock().unwrap().pomodoro.native_context.clone(),
                );
            }
            "music_play_pause" => {
                dispatch_music_control(app, crate::music::session::SessionIntent::Toggle);
            }
            "music_previous" => {
                dispatch_music_control(app, crate::music::session::SessionIntent::Previous);
            }
            "music_next" => {
                dispatch_music_control(app, crate::music::session::SessionIntent::Next);
            }
            "music_open" => {
                let _ = app.emit("tray-music-open", ());
            }
            "music_inspect_assignment" => {
                let _ = app.emit("tray-music-inspect-assignment", ());
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Update tray icon progress ring, tooltip, and Pomodoro menu state.
async fn update_tray(
    app: &AppHandle,
    update: PomodoroTrayUpdate,
    context: crate::pomodoro::FocusNativeContext,
) -> Result<(), String> {
    let (response, receiver) = tokio::sync::oneshot::channel();
    let callback_app = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            if !crate::pomodoro::native_runtime::presentation_is_current(
                &callback_app,
                context.vault_generation,
                context.revision,
            ) {
                return Err("Native Focus tray delivery was superseded or expired".to_owned());
            }
            let previous = TRAY_STATE
                .lock()
                .map_err(|error| format!("Read native tray state: {error}"))?
                .clone();
            let mut next = previous;
            next.pomodoro = PomodoroTrayState {
                phase: update.phase,
                remaining_seconds: update.remaining_seconds,
                total_seconds: update.total_seconds,
                is_running: update.is_running,
                is_active: update.is_active,
                can_pause_resume: update.can_pause_resume,
                can_add_focus_time: update.can_add_focus_time,
                paused_pulse_frame: update.paused_pulse_frame,
                native_context: Some(context),
            };
            apply_tray_state(&callback_app, &next)?;
            // Main-thread menu events cannot run until its displayed context is retained.
            TRAY_STATE
                .lock()
                .map_err(|error| format!("Publish native tray state: {error}"))?
                .pomodoro = next.pomodoro;
            Ok(())
        })();
        let _ = response.send(result);
    })
    .map_err(|error| format!("Queue native Focus tray delivery: {error}"))?;
    receiver
        .await
        .map_err(|error| format!("Native Focus tray delivery stopped: {error}"))?
}

fn dispatch_focus_control(
    app: &AppHandle,
    intent: ganbaru_pomodoro::FocusIntent,
    context: Option<crate::pomodoro::FocusNativeContext>,
) {
    let Some(context) = context else {
        return;
    };
    if let Err(error) = crate::pomodoro::native_control_in_context(app, intent, context) {
        eprintln!("Dispatch native Focus control: {error}");
    }
}

/// Update native presentation from committed state; interpolation cannot advance a phase.
pub(crate) async fn reconcile_committed_focus(
    app: &AppHandle,
    generation: u64,
    snapshot: &ganbaru_pomodoro::FocusExecutionSnapshot,
    now_ms: i64,
) -> Result<(), String> {
    use ganbaru_pomodoro::{FocusMode, FocusPhase};
    let context = crate::pomodoro::capture_native_context(app)?;
    if !context.matches_snapshot(generation, snapshot) {
        return Err("Native Focus tray snapshot was superseded".to_owned());
    }
    let running = snapshot.mode == FocusMode::Running;
    let active = snapshot
        .run
        .as_ref()
        .is_some_and(|run| run.ended_at_ms.is_none())
        && !matches!(snapshot.mode, FocusMode::Stopped | FocusMode::Expired);
    let focus = snapshot
        .segment
        .as_ref()
        .is_some_and(|segment| segment.phase == FocusPhase::Focus);
    let actionable =
        active && focus && matches!(snapshot.mode, FocusMode::Running | FocusMode::ManualPause);
    let remaining = snapshot
        .remaining_ms
        .saturating_sub(if running {
            now_ms.saturating_sub(snapshot.observed_at_ms).max(0)
        } else {
            0
        })
        .max(0);
    let seconds = |milliseconds: i64| {
        u32::try_from(milliseconds.saturating_add(999) / 1000).unwrap_or(u32::MAX)
    };
    update_tray(
        app,
        PomodoroTrayUpdate {
            phase: match snapshot.segment.as_ref().map(|segment| segment.phase) {
                Some(FocusPhase::Focus) => "focus",
                Some(FocusPhase::ShortBreak) => "short_break",
                Some(FocusPhase::LongBreak) => "long_break",
                None => "idle",
            }
            .into(),
            remaining_seconds: seconds(remaining),
            total_seconds: seconds(
                snapshot
                    .segment
                    .as_ref()
                    .map(|segment| segment.chosen_duration_ms)
                    .unwrap_or(0),
            ),
            is_running: running,
            is_active: active,
            can_pause_resume: actionable,
            can_add_focus_time: actionable && !snapshot.focus_extension_used,
            paused_pulse_frame: (snapshot.mode == FocusMode::ManualPause)
                .then(|| ((now_ms.max(0) / 180) % i64::from(PAUSED_PULSE_FRAME_COUNT)) as u8),
        },
        context,
    )
    .await
}

/// Update the Music section of the shared tray menu.
#[tauri::command]
pub fn update_music_tray(app: AppHandle, update: MusicTrayUpdate) -> Result<(), String> {
    {
        let mut state = TRAY_STATE.lock().unwrap();
        state.music = MusicTrayState {
            status: update.status,
            title: update.title,
            can_play_pause: update.can_play_pause,
            can_previous: update.can_previous,
            can_next: update.can_next,
            context_label: update.context_label,
        };
    }
    // Retain one queued refresh and read the displayed Focus context on the main
    // thread. A delayed Music update cannot redraw an earlier Focus phase while
    // retaining a newer run's controls, or accumulate refreshes behind a stalled UI.
    if MUSIC_TRAY_REFRESH_QUEUED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let callback_app = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        MUSIC_TRAY_REFRESH_QUEUED.store(false, Ordering::Release);
        let result = TRAY_STATE
            .lock()
            .map_err(|error| format!("Read shared tray state: {error}"))
            .map(|state| state.clone())
            .and_then(|state| apply_tray_state(&callback_app, &state));
        if let Err(error) = result {
            eprintln!("Publish shared native tray presentation: {error}");
        }
    }) {
        MUSIC_TRAY_REFRESH_QUEUED.store(false, Ordering::Release);
        return Err(format!("Queue native Music tray delivery: {error}"));
    }
    Ok(())
}

fn dispatch_music_control(app: &AppHandle, intent: crate::music::session::SessionIntent) {
    if let Err(error) = crate::music::session::dispatch_control(app, intent) {
        eprintln!("native tray Music control failed: {error}");
    }
}

/// Updates transport presentation from committed native Music state.
pub(crate) fn publish_music_session(
    app: &AppHandle,
    session: &crate::music::session::SessionProjection,
) -> Result<(), String> {
    update_music_tray(
        app.clone(),
        MusicTrayUpdate {
            status: session.status.as_str().into(),
            title: session
                .current_source
                .as_ref()
                .map(|source| source.title.clone()),
            can_play_pause: session.current_source.is_some(),
            can_previous: session.can_previous,
            can_next: session.can_next,
            context_label: session
                .context
                .as_ref()
                .map(|context| context.event_title.clone()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray_state_with_pause_resume(can_pause_resume: bool) -> TrayState {
        TrayState {
            pomodoro: PomodoroTrayState {
                phase: "focus".to_string(),
                remaining_seconds: 1200,
                total_seconds: 2400,
                is_running: true,
                is_active: true,
                can_pause_resume,
                can_add_focus_time: false,
                paused_pulse_frame: None,
                native_context: None,
            },
            music: MusicTrayState {
                status: "idle".to_string(),
                title: None,
                can_play_pause: false,
                can_previous: false,
                can_next: false,
                context_label: None,
            },
        }
    }

    fn has_ring_pixel_with_color(pixels: &[u8], color: (u8, u8, u8)) -> bool {
        pixels.as_chunks::<4>().0.iter().any(|pixel| {
            pixel[3] > 0 && pixel[0] == color.0 && pixel[1] == color.1 && pixel[2] == color.2
        })
    }

    #[test]
    fn active_zero_progress_renders_remaining_ring() {
        let pixels = render_progress_icon(0.0, true);

        assert!(has_ring_pixel_with_color(&pixels, RING_REMAINING_COLOR));
    }

    #[test]
    fn inactive_zero_progress_renders_empty_ring() {
        let pixels = render_progress_icon(0.0, false);

        assert!(has_ring_pixel_with_color(&pixels, RING_EMPTY_COLOR));
        assert!(!has_ring_pixel_with_color(&pixels, RING_REMAINING_COLOR));
    }

    #[test]
    fn active_negative_progress_renders_remaining_ring() {
        assert_eq!(
            render_progress_icon(-0.25, true),
            render_progress_icon(0.0, true)
        );
    }

    #[test]
    fn active_positive_progress_renders_progress_arc() {
        let pixels = render_progress_icon(0.5, true);

        assert!(has_ring_pixel_with_color(&pixels, RING_REMAINING_COLOR));
        assert!(has_ring_pixel_with_color(&pixels, RING_EMPTY_COLOR));
    }

    #[test]
    fn active_progress_reduces_remaining_arc_from_top() {
        let pixels = render_progress_icon(0.25, true);
        let top_idx = ((2 * ICON_SIZE as usize) + (ICON_SIZE as usize / 2)) * 4;
        let bottom_idx =
            (((ICON_SIZE as usize - 3) * ICON_SIZE as usize) + (ICON_SIZE as usize / 2)) * 4;

        assert_eq!(
            &pixels[top_idx..top_idx + 3],
            &[RING_EMPTY_COLOR.0, RING_EMPTY_COLOR.1, RING_EMPTY_COLOR.2,]
        );
        assert_eq!(
            &pixels[bottom_idx..bottom_idx + 3],
            &[
                RING_REMAINING_COLOR.0,
                RING_REMAINING_COLOR.1,
                RING_REMAINING_COLOR.2,
            ]
        );
    }

    #[test]
    fn active_zero_progress_uses_progress_icon_key() {
        assert_eq!(tray_icon_key(0.0, true, None), TrayIconKey::Progress(0));
    }

    #[test]
    fn inactive_zero_progress_uses_empty_icon_key() {
        assert_eq!(tray_icon_key(0.0, false, None), TrayIconKey::Empty);
    }

    #[test]
    fn active_negative_progress_uses_zero_progress_icon_key() {
        assert_eq!(tray_icon_key(-0.25, true, None), TrayIconKey::Progress(0));
    }

    #[test]
    fn complete_progress_uses_empty_icon_key() {
        assert_eq!(tray_icon_key(1.0, true, None), TrayIconKey::Empty);
    }

    #[test]
    fn positive_progress_uses_progress_icon_key() {
        assert_eq!(tray_icon_key(0.5, true, None), TrayIconKey::Progress(50));
    }

    #[test]
    fn paused_progress_uses_paused_icon_key() {
        assert_eq!(
            tray_icon_key(0.5, true, Some(23)),
            TrayIconKey::PausedProgress { step: 50, frame: 1 }
        );
    }

    #[test]
    fn paused_progress_peak_renders_empty_gray_remaining_ring() {
        let pixels = render_progress_icon_with_pause(0.0, true, Some(12));

        assert!(has_ring_pixel_with_color(&pixels, RING_EMPTY_COLOR));
    }

    #[test]
    fn menu_shape_tracks_pause_resume_availability() {
        let disabled = menu_shape(&tray_state_with_pause_resume(false));
        let enabled = menu_shape(&tray_state_with_pause_resume(true));

        assert!(!disabled.pomodoro_can_pause_resume);
        assert!(enabled.pomodoro_can_pause_resume);
        assert_ne!(disabled, enabled);
    }

    #[test]
    fn long_music_menu_status_is_truncated() {
        let full_title =
            "Christopher Larkin - Hollow Knight (Original Soundtrack) - 09 City of Tears";
        let state = MusicTrayState {
            status: "playing".to_string(),
            title: Some(full_title.to_string()),
            can_play_pause: true,
            can_previous: true,
            can_next: true,
            context_label: None,
        };

        let label = music_menu_status_text(&state);

        assert!(label.chars().count() <= MUSIC_TRAY_STATUS_MAX_CHARS);
        assert!(label.starts_with("Christopher Larkin"));
        assert!(label.ends_with("..."));
        assert_ne!(label, full_title);
    }

    #[test]
    fn full_music_status_stays_available_for_tooltip() {
        let state = MusicTrayState {
            status: "playing".to_string(),
            title: Some(
                "Christopher Larkin - Hollow Knight (Original Soundtrack) - 09 City of Tears"
                    .to_string(),
            ),
            can_play_pause: true,
            can_previous: true,
            can_next: true,
            context_label: None,
        };

        assert_eq!(
            music_status_text(&state),
            "Christopher Larkin - Hollow Knight (Original Soundtrack) - 09 City of Tears"
        );
    }

    #[test]
    fn contextual_music_is_visible_in_the_tooltip_and_menu_shape() {
        let mut state = tray_state_with_pause_resume(true);
        state.music.status = "playing".to_string();
        state.music.title = Some("City of Tears".to_string());
        state.music.context_label = Some("Focus soundtrack for Reading".to_string());

        assert!(tray_tooltip(&state).contains("Focus soundtrack for Reading"));
        assert_eq!(
            menu_shape(&state).music_context_label.as_deref(),
            Some("Focus soundtrack for Reading")
        );
    }
}
