use std::sync::mpsc::{self, SyncSender};
use std::thread::JoinHandle;

#[cfg(any(target_os = "windows", test))]
const VK_TAB_CODE: u32 = 0x09;
#[cfg(any(target_os = "windows", test))]
const VK_ESCAPE_CODE: u32 = 0x1B;
#[cfg(any(target_os = "windows", test))]
const VK_SPACE_CODE: u32 = 0x20;
#[cfg(any(target_os = "windows", test))]
const VK_DELETE_CODE: u32 = 0x2E;
#[cfg(any(target_os = "windows", test))]
const VK_0_CODE: u32 = 0x30;
#[cfg(any(target_os = "windows", test))]
const VK_9_CODE: u32 = 0x39;
#[cfg(any(target_os = "windows", test))]
const VK_D_CODE: u32 = 0x44;
#[cfg(any(target_os = "windows", test))]
const VK_E_CODE: u32 = 0x45;
#[cfg(any(target_os = "windows", test))]
const VK_F4_CODE: u32 = 0x73;
#[cfg(any(target_os = "windows", test))]
const VK_L_CODE: u32 = 0x4C;
#[cfg(any(target_os = "windows", test))]
const VK_M_CODE: u32 = 0x4D;
#[cfg(any(target_os = "windows", test))]
const VK_R_CODE: u32 = 0x52;
#[cfg(any(target_os = "windows", test))]
const VK_LWIN_CODE: u32 = 0x5B;
#[cfg(any(target_os = "windows", test))]
const VK_RWIN_CODE: u32 = 0x5C;
#[cfg(any(target_os = "windows", test))]
const VK_SHIFT_CODE: u32 = 0x10;
#[cfg(any(target_os = "windows", test))]
const VK_CONTROL_CODE: u32 = 0x11;
#[cfg(any(target_os = "windows", test))]
const VK_MENU_CODE: u32 = 0x12;
#[cfg(any(target_os = "windows", test))]
const VK_LSHIFT_CODE: u32 = 0xA0;
#[cfg(any(target_os = "windows", test))]
const VK_RSHIFT_CODE: u32 = 0xA1;
#[cfg(any(target_os = "windows", test))]
const VK_LCONTROL_CODE: u32 = 0xA2;
#[cfg(any(target_os = "windows", test))]
const VK_RCONTROL_CODE: u32 = 0xA3;
#[cfg(any(target_os = "windows", test))]
const VK_LMENU_CODE: u32 = 0xA4;
#[cfg(any(target_os = "windows", test))]
const VK_RMENU_CODE: u32 = 0xA5;

#[cfg(any(target_os = "windows", test))]
const MODIFIER_SHIFT: u16 = 1 << 0;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_CONTROL: u16 = 1 << 1;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_ALT: u16 = 1 << 2;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_LEFT_SHIFT: u16 = 1 << 3;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_RIGHT_SHIFT: u16 = 1 << 4;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_LEFT_CONTROL: u16 = 1 << 5;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_RIGHT_CONTROL: u16 = 1 << 6;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_LEFT_ALT: u16 = 1 << 7;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_RIGHT_ALT: u16 = 1 << 8;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_LEFT_WIN: u16 = 1 << 9;
#[cfg(any(target_os = "windows", test))]
const MODIFIER_RIGHT_WIN: u16 = 1 << 10;

#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_HIDE_DOCK: u64 = 1 << 1;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_HIDE_MENU_BAR: u64 = 1 << 3;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_DISABLE_PROCESS_SWITCHING: u64 = 1 << 5;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_DISABLE_FORCE_QUIT: u64 = 1 << 6;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_DISABLE_SESSION_TERMINATION: u64 = 1 << 7;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_DISABLE_HIDE_APPLICATION: u64 = 1 << 8;
#[cfg(any(target_os = "macos", test))]
const MAC_PRESENTATION_FULL_SCREEN: u64 = 1 << 10;
#[cfg(any(target_os = "macos", test))]
const MAC_APPROVED_PRESENTATION_BITS: u64 = MAC_PRESENTATION_HIDE_DOCK
    | MAC_PRESENTATION_HIDE_MENU_BAR
    | MAC_PRESENTATION_DISABLE_PROCESS_SWITCHING
    | MAC_PRESENTATION_DISABLE_FORCE_QUIT
    | MAC_PRESENTATION_DISABLE_SESSION_TERMINATION
    | MAC_PRESENTATION_DISABLE_HIDE_APPLICATION
    | MAC_PRESENTATION_FULL_SCREEN;

trait EnforcementCleanup: Send {
    fn stop(&mut self) -> Result<(), String>;
}

impl<F> EnforcementCleanup for F
where
    F: FnMut() -> Result<(), String> + Send,
{
    fn stop(&mut self) -> Result<(), String> {
        self()
    }
}

pub(crate) struct OverlayEnforcementGuard {
    labels: Vec<String>,
    primary_label: String,
    cleanups: Vec<Box<dyn EnforcementCleanup>>,
    stopped: bool,
}

impl OverlayEnforcementGuard {
    fn new(labels: &[String], primary_label: &str) -> Self {
        Self {
            labels: labels.to_vec(),
            primary_label: primary_label.to_string(),
            cleanups: Vec::new(),
            stopped: false,
        }
    }

    pub(crate) fn set_window_labels(&mut self, labels: Vec<String>, primary_label: &str) {
        self.labels = labels;
        self.primary_label = primary_label.to_string();
    }

    pub(crate) fn push_cleanup<F>(&mut self, cleanup: F)
    where
        F: FnMut() -> Result<(), String> + Send + 'static,
    {
        self.cleanups.push(Box::new(cleanup));
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    fn push_guard<G>(&mut self, guard: G)
    where
        G: EnforcementCleanup + 'static,
    {
        self.cleanups.push(Box::new(guard));
    }

    pub(crate) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let _ = self.labels.len();
        let _ = self.primary_label.as_str();
        while let Some(mut cleanup) = self.cleanups.pop() {
            if let Err(err) = cleanup.stop() {
                eprintln!("failed to stop Pomodoro overlay enforcement: {err}");
            }
        }
    }

    #[cfg(test)]
    fn cleanup_len(&self) -> usize {
        self.cleanups.len()
    }
}

impl Drop for OverlayEnforcementGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg(any(target_os = "windows", test))]
pub(crate) struct WindowsOverlayShortcutModifiers {
    pub(crate) alt: bool,
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    pub(crate) win: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(any(target_os = "windows", test))]
pub(crate) struct WindowsOverlayShortcutEvent {
    pub(crate) key_code: u32,
    pub(crate) modifiers: WindowsOverlayShortcutModifiers,
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn should_block_windows_overlay_shortcut(event: WindowsOverlayShortcutEvent) -> bool {
    let key_code = event.key_code;
    let modifiers = event.modifiers;

    if key_code == VK_LWIN_CODE || key_code == VK_RWIN_CODE {
        return true;
    }

    if modifiers.ctrl && modifiers.alt && key_code == VK_DELETE_CODE {
        return false;
    }

    if modifiers.win {
        return is_shell_chord_key(key_code) || !is_modifier_only_key(key_code);
    }

    if modifiers.alt
        && matches!(
            key_code,
            VK_TAB_CODE | VK_ESCAPE_CODE | VK_F4_CODE | VK_SPACE_CODE
        )
    {
        return true;
    }

    if modifiers.ctrl && key_code == VK_ESCAPE_CODE {
        return true;
    }

    modifiers.ctrl && modifiers.shift && key_code == VK_ESCAPE_CODE
}

#[cfg(any(target_os = "windows", test))]
fn windows_modifier_bit(key_code: u32) -> Option<u16> {
    match key_code {
        VK_SHIFT_CODE => Some(MODIFIER_SHIFT),
        VK_CONTROL_CODE => Some(MODIFIER_CONTROL),
        VK_MENU_CODE => Some(MODIFIER_ALT),
        VK_LSHIFT_CODE => Some(MODIFIER_LEFT_SHIFT),
        VK_RSHIFT_CODE => Some(MODIFIER_RIGHT_SHIFT),
        VK_LCONTROL_CODE => Some(MODIFIER_LEFT_CONTROL),
        VK_RCONTROL_CODE => Some(MODIFIER_RIGHT_CONTROL),
        VK_LMENU_CODE => Some(MODIFIER_LEFT_ALT),
        VK_RMENU_CODE => Some(MODIFIER_RIGHT_ALT),
        VK_LWIN_CODE => Some(MODIFIER_LEFT_WIN),
        VK_RWIN_CODE => Some(MODIFIER_RIGHT_WIN),
        _ => None,
    }
}

#[cfg(any(target_os = "windows", test))]
fn update_windows_modifier_bits(bits: u16, key_code: u32, key_down: bool) -> u16 {
    let Some(bit) = windows_modifier_bit(key_code) else {
        return bits;
    };
    if key_down { bits | bit } else { bits & !bit }
}

#[cfg(any(target_os = "windows", test))]
fn windows_modifiers_from_bits(
    bits: u16,
    low_level_alt_context: bool,
) -> WindowsOverlayShortcutModifiers {
    WindowsOverlayShortcutModifiers {
        alt: low_level_alt_context
            || bits & (MODIFIER_ALT | MODIFIER_LEFT_ALT | MODIFIER_RIGHT_ALT) != 0,
        ctrl: bits & (MODIFIER_CONTROL | MODIFIER_LEFT_CONTROL | MODIFIER_RIGHT_CONTROL) != 0,
        shift: bits & (MODIFIER_SHIFT | MODIFIER_LEFT_SHIFT | MODIFIER_RIGHT_SHIFT) != 0,
        win: bits & (MODIFIER_LEFT_WIN | MODIFIER_RIGHT_WIN) != 0,
    }
}

#[cfg(any(target_os = "windows", test))]
fn is_shell_chord_key(key_code: u32) -> bool {
    matches!(
        key_code,
        VK_TAB_CODE | VK_D_CODE | VK_E_CODE | VK_L_CODE | VK_M_CODE | VK_R_CODE
    ) || (VK_0_CODE..=VK_9_CODE).contains(&key_code)
}

#[cfg(any(target_os = "windows", test))]
fn is_modifier_only_key(key_code: u32) -> bool {
    matches!(key_code, VK_LWIN_CODE | VK_RWIN_CODE)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn mac_overlay_presentation_options_bits() -> u64 {
    MAC_APPROVED_PRESENTATION_BITS
}

#[derive(Debug, Default)]
#[cfg(any(target_os = "macos", test))]
struct MacPresentationLeaseState {
    active_guards: usize,
    restore_options_bits: Option<u64>,
}

#[cfg(any(target_os = "macos", test))]
impl MacPresentationLeaseState {
    fn acquire(&mut self, previous_options_bits: u64) -> Result<(), String> {
        let next_active_guards = self
            .active_guards
            .checked_add(1)
            .ok_or_else(|| "macOS presentation guard count overflowed".to_string())?;
        if self.active_guards == 0 {
            self.restore_options_bits = Some(previous_options_bits);
        }
        self.active_guards = next_active_guards;
        Ok(())
    }

    fn release(&mut self) -> Result<Option<u64>, String> {
        if self.active_guards == 0 {
            return Err("macOS presentation guard release was unbalanced".to_string());
        }
        self.active_guards -= 1;
        if self.active_guards == 0 {
            self.restore_options_bits
                .take()
                .map(Some)
                .ok_or_else(|| "macOS presentation restore state was unavailable".to_string())
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn start_overlay_enforcement(
    app: &tauri::AppHandle,
    labels: &[String],
    primary_label: &str,
) -> OverlayEnforcementGuard {
    let guard = OverlayEnforcementGuard::new(labels, primary_label);

    #[cfg(target_os = "windows")]
    {
        let _ = app;
        let mut guard = guard;
        match windows::WindowsPowerGuard::start() {
            Ok(power_guard) => guard.push_guard(power_guard),
            Err(err) => eprintln!("failed to start Windows overlay power guard: {err}"),
        }
        match windows::WindowsShortcutHookGuard::start() {
            Ok(shortcut_guard) => guard.push_guard(shortcut_guard),
            Err(err) => eprintln!("failed to start Windows overlay shortcut guard: {err}"),
        }
        return guard;
    }

    #[cfg(target_os = "macos")]
    {
        let mut guard = guard;
        match macos::MacPresentationGuard::start(app) {
            Ok(presentation_guard) => guard.push_guard(presentation_guard),
            Err(err) => eprintln!("failed to start macOS overlay presentation guard: {err}"),
        }
        match macos::MacPowerAssertionGuard::start() {
            Ok(power_guard) => guard.push_guard(power_guard),
            Err(err) => eprintln!("failed to start macOS overlay power guard: {err}"),
        }
        return guard;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        guard
    }
}

pub(crate) fn reinforce_overlay_windows(
    app: &tauri::AppHandle,
    labels: &[String],
    primary_label: &str,
) {
    #[cfg(target_os = "windows")]
    let _ = primary_label;

    #[cfg(target_os = "windows")]
    if let Err(err) = windows::reinforce_overlay_windows(app, labels) {
        eprintln!("failed to reinforce Windows Pomodoro overlay windows: {err}");
    }

    #[cfg(target_os = "macos")]
    if let Err(err) = macos::reinforce_overlay_windows(app, labels, primary_label) {
        eprintln!("failed to reinforce macOS Pomodoro overlay windows: {err}");
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = app;
        let _ = labels;
        let _ = primary_label;
    }
}

pub(crate) struct OverlayReconcileGuard {
    stop_tx: Option<SyncSender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl OverlayReconcileGuard {
    pub(crate) fn start<F>(reconcile: F) -> Result<Self, String>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        Self::start_with_interval(reconcile, std::time::Duration::from_millis(2_500))
    }

    fn start_with_interval<F>(
        mut reconcile: F,
        interval: std::time::Duration,
    ) -> Result<Self, String>
    where
        F: FnMut() -> bool + Send + 'static,
    {
        let (stop_tx, stop_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("ganbaru-ai-pomodoro-overlay-reconcile".to_string())
            .spawn(move || {
                loop {
                    match stop_rx.recv_timeout(interval) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if !reconcile() {
                                break;
                            }
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
        })
    }

    fn request_stop(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), String> {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| "Pomodoro overlay monitor reconciler panicked".to_string())?;
        }
        Ok(())
    }
}

impl Drop for OverlayReconcileGuard {
    fn drop(&mut self) {
        self.request_stop();
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
use super::run_on_main_thread_blocking;

#[cfg(target_os = "windows")]
mod windows {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc::{self, SyncSender},
    };
    use std::thread::JoinHandle;

    use super::{
        VK_LCONTROL_CODE, VK_LMENU_CODE, VK_LSHIFT_CODE, VK_LWIN_CODE, VK_RCONTROL_CODE,
        VK_RMENU_CODE, VK_RSHIFT_CODE, VK_RWIN_CODE, WindowsOverlayShortcutEvent,
        run_on_main_thread_blocking, should_block_windows_overlay_shortcut,
        update_windows_modifier_bits, windows_modifiers_from_bits,
    };
    use tauri::Manager;
    use windows::Win32::Foundation::{
        HANDLE, HINSTANCE, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Power::{
        ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };
    use windows::Win32::System::Threading::{CreateEventW, SetEvent};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, HC_ACTION, HWND_TOPMOST, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN,
        MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SWP_NOMOVE,
        SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, SetWindowsHookExW, TranslateMessage,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };
    use windows::core::PCWSTR;

    static MODIFIER_BITS: AtomicU16 = AtomicU16::new(0);
    const SHORTCUT_HOOK_STOP_POLL_MS: u32 = 250;

    pub(super) struct WindowsPowerGuard {
        stop_tx: Option<SyncSender<()>>,
        worker: Option<JoinHandle<Result<(), String>>>,
    }

    impl WindowsPowerGuard {
        pub(super) fn start() -> Result<Self, String> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let (stop_tx, stop_rx) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("ganbaru-ai-overlay-windows-power".to_string())
                .spawn(move || {
                    let active_state = ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED;
                    // SAFETY: The flags are a documented execution-state
                    // combination and this dedicated thread also clears them.
                    let active = unsafe { SetThreadExecutionState(active_state) };
                    let _ = ready_tx.send(active.0 != 0);
                    let _ = stop_rx.recv();
                    // SAFETY: ES_CONTINUOUS alone clears the requirements set by
                    // this same thread and requires no pointer or owned handle.
                    if unsafe { SetThreadExecutionState(ES_CONTINUOUS) }.0 == 0 {
                        Err("Windows rejected execution-state cleanup".to_string())
                    } else {
                        Ok(())
                    }
                })
                .map_err(|e| e.to_string())?;

            let accepted = match ready_rx.recv() {
                Ok(accepted) => accepted,
                Err(error) => {
                    let _ = stop_tx.send(());
                    let _ = worker.join();
                    return Err(error.to_string());
                }
            };
            if !accepted {
                let _ = stop_tx.send(());
                worker
                    .join()
                    .map_err(|_| "Windows power guard thread panicked".to_string())??;
                return Err("Windows rejected the Pomodoro overlay execution state".to_string());
            }

            Ok(Self {
                stop_tx: Some(stop_tx),
                worker: Some(worker),
            })
        }
    }

    impl super::EnforcementCleanup for WindowsPowerGuard {
        fn stop(&mut self) -> Result<(), String> {
            if let Some(stop_tx) = self.stop_tx.take() {
                let _ = stop_tx.send(());
            }
            if let Some(worker) = self.worker.take() {
                worker
                    .join()
                    .map_err(|_| "Windows power guard thread panicked".to_string())??;
            }
            Ok(())
        }
    }

    pub(super) struct WindowsShortcutHookGuard {
        stop_requested: Arc<AtomicBool>,
        stop_event: OwnedHandle,
        worker: Option<JoinHandle<Result<(), String>>>,
    }

    impl WindowsShortcutHookGuard {
        pub(super) fn start() -> Result<Self, String> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            // SAFETY: Null security attributes and name create a private,
            // non-inheritable manual-reset event. The returned handle is new.
            let raw_stop_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
                .map_err(|e| e.to_string())?;
            // SAFETY: CreateEventW returned a new handle owned by this call. No
            // other Rust owner exists, so OwnedHandle closes it exactly once.
            let stop_event = unsafe { OwnedHandle::from_raw_handle(raw_stop_event.0) };
            let worker_stop_event = stop_event.try_clone().map_err(|e| e.to_string())?;
            let stop_requested = Arc::new(AtomicBool::new(false));
            let worker_stop_requested = Arc::clone(&stop_requested);
            let worker = std::thread::Builder::new()
                .name("ganbaru-ai-overlay-keyboard-hook".to_string())
                .spawn(move || {
                    run_shortcut_hook(worker_stop_event, worker_stop_requested, ready_tx)
                })
                .map_err(|e| e.to_string())?;

            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Self {
                    stop_requested,
                    stop_event,
                    worker: Some(worker),
                }),
                Ok(Err(err)) => {
                    let _ = worker.join();
                    Err(err)
                }
                Err(err) => {
                    let _ = worker.join();
                    Err(err.to_string())
                }
            }
        }
    }

    fn run_shortcut_hook(
        stop_event: OwnedHandle,
        stop_requested: Arc<AtomicBool>,
        ready_tx: SyncSender<Result<(), String>>,
    ) -> Result<(), String> {
        MODIFIER_BITS.store(initial_modifier_bits(), Ordering::Relaxed);
        // SAFETY: A null module name requests the current executable module. The
        // module remains loaded for the process lifetime and contains the callback.
        let module = match unsafe { GetModuleHandleW(PCWSTR::null()) } {
            Ok(module) => module,
            Err(error) => {
                let _ = ready_tx.send(Err(error.to_string()));
                return Ok(());
            }
        };
        // SAFETY: The callback has the required system ABI and process lifetime.
        // `module` identifies the loaded image containing it. WH_KEYBOARD_LL is
        // delivered back to this installing thread, and Windows validates whether
        // the image and desktop support this desktop-wide hook during installation.
        let hook = match unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                Some(HINSTANCE(module.0)),
                0,
            )
        } {
            Ok(hook) => hook,
            Err(error) => {
                let _ = ready_tx.send(Err(error.to_string()));
                return Ok(());
            }
        };
        if ready_tx.send(Ok(())).is_err() {
            // SAFETY: `hook` is the live handle returned above and has not been
            // unhooked. This call releases it before the worker exits.
            return unsafe { UnhookWindowsHookEx(hook) }.map_err(|error| error.to_string());
        }

        let handles = [HANDLE(stop_event.as_raw_handle())];
        let wait_result = 'wait: loop {
            if stop_requested.load(Ordering::SeqCst) {
                break Ok(());
            }
            // SAFETY: `handles` borrows one live event for this call. The worker
            // pumps all queued input messages whenever the message queue wakes.
            // The timeout observes the atomic fallback if event signaling fails.
            let result = unsafe {
                MsgWaitForMultipleObjects(
                    Some(&handles),
                    false,
                    SHORTCUT_HOOK_STOP_POLL_MS,
                    QS_ALLINPUT,
                )
            };
            if result == WAIT_OBJECT_0 {
                break Ok(());
            }
            if result == WAIT_TIMEOUT {
                continue;
            }
            if result == WAIT_FAILED {
                break Err(windows::core::Error::from_win32().to_string());
            }
            if result.0 != WAIT_OBJECT_0.0 + handles.len() as u32 {
                break Err(format!(
                    "Windows keyboard hook wait returned status {}",
                    result.0
                ));
            }

            let mut message = MSG::default();
            loop {
                // SAFETY: `message` is writable for one MSG and the remaining
                // arguments request removal of any message for this thread.
                if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                    break;
                }
                if message.message == WM_QUIT {
                    break 'wait Ok(());
                }
                // SAFETY: `message` was initialized by PeekMessageW and remains
                // live for the standard translation and dispatch calls.
                unsafe {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        };

        // SAFETY: `hook` is still the one live handle returned by installation.
        // The message loop has stopped, so no later callback depends on it.
        let unhook_result = unsafe { UnhookWindowsHookEx(hook) }
            .map_err(|error| format!("Windows keyboard hook cleanup failed: {error}"));
        match (wait_result, unhook_result) {
            (Err(wait_error), Err(unhook_error)) => {
                Err(format!("{wait_error}; additionally, {unhook_error}"))
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    impl super::EnforcementCleanup for WindowsShortcutHookGuard {
        fn stop(&mut self) -> Result<(), String> {
            if self.worker.is_none() {
                return Ok(());
            }
            self.stop_requested.store(true, Ordering::SeqCst);
            // SAFETY: `stop_event` owns a live event handle. Signaling does not
            // transfer or close it, and it wakes the worker's message wait.
            let signal_result = unsafe { SetEvent(HANDLE(self.stop_event.as_raw_handle())) };
            if let Some(worker) = self.worker.take() {
                worker
                    .join()
                    .map_err(|_| "Windows shortcut hook thread panicked".to_string())??;
            }
            signal_result.map_err(|e| e.to_string())
        }
    }

    // SAFETY: Windows calls this function on the hook worker thread with
    // callback-duration message pointers. Its body performs only nonblocking
    // atomic and pure work, contains no panicking operations, and forwards every
    // unconsumed event.
    unsafe extern "system" fn low_level_keyboard_proc(
        ncode: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let message = wparam.0 as u32;
        if ncode == HC_ACTION as i32 && is_keyboard_message(message) {
            // SAFETY: For HC_ACTION keyboard messages, Windows guarantees lParam
            // points to an aligned KBDLLHOOKSTRUCT for this callback invocation.
            // `as_ref` also rejects a defensive null value before dereferencing.
            if let Some(keyboard) = unsafe { (lparam.0 as *const KBDLLHOOKSTRUCT).as_ref() } {
                let key_down = is_key_down_message(message);
                let bits = update_windows_modifier_bits(
                    MODIFIER_BITS.load(Ordering::Relaxed),
                    keyboard.vkCode,
                    key_down,
                );
                MODIFIER_BITS.store(bits, Ordering::Relaxed);
                let event = WindowsOverlayShortcutEvent {
                    key_code: keyboard.vkCode,
                    modifiers: windows_modifiers_from_bits(
                        bits,
                        keyboard.flags.contains(LLKHF_ALTDOWN),
                    ),
                };
                if should_block_windows_overlay_shortcut(event) {
                    return LRESULT(1);
                }
            }
        }
        // SAFETY: These are the unchanged arguments supplied by Windows. Passing
        // them onward is required whenever Ganbaru does not consume the event.
        unsafe { CallNextHookEx(None, ncode, wparam, lparam) }
    }

    fn is_keyboard_message(message: u32) -> bool {
        matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP)
    }

    fn is_key_down_message(message: u32) -> bool {
        matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN)
    }

    fn initial_modifier_bits() -> u16 {
        [
            VK_LSHIFT_CODE,
            VK_RSHIFT_CODE,
            VK_LCONTROL_CODE,
            VK_RCONTROL_CODE,
            VK_LMENU_CODE,
            VK_RMENU_CODE,
            VK_LWIN_CODE,
            VK_RWIN_CODE,
        ]
        .into_iter()
        .fold(0, |bits, key_code| {
            update_windows_modifier_bits(bits, key_code, async_key_down(key_code))
        })
    }

    fn async_key_down(key_code: u32) -> bool {
        // SAFETY: This query runs only while initializing the worker, outside the
        // low-level hook callback where Windows documents async state as stale.
        unsafe { GetAsyncKeyState(key_code as i32) < 0 }
    }

    pub(super) fn reinforce_overlay_windows(
        app: &tauri::AppHandle,
        labels: &[String],
    ) -> Result<(), String> {
        let labels = labels.to_vec();
        let app_for_setup = app.clone();
        run_on_main_thread_blocking(app, move || {
            for label in labels {
                let Some(window) = app_for_setup.get_webview_window(&label) else {
                    continue;
                };
                let hwnd = window.hwnd().map_err(|e| e.to_string())?;
                // SAFETY: Tauri returned a live HWND for `window`, which remains
                // owned through this main-thread call. The flags ignore dimensions
                // and request only topmost placement without ownership transfer.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::sync::Mutex;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSApplication, NSApplicationPresentationOptions, NSScreenSaverWindowLevel, NSWindow,
    };
    use objc2_core_foundation::{CFRetained, CFString};
    use tauri::Manager;

    use super::{
        MacPresentationLeaseState, mac_overlay_presentation_options_bits,
        run_on_main_thread_blocking,
    };

    type IOPMAssertionId = u32;
    type IOPMAssertionLevel = u32;
    type IOReturn = i32;
    type CFStringRef = *const c_void;

    const K_IOPM_ASSERTION_LEVEL_ON: IOPMAssertionLevel = 255;

    const DISPLAY_SLEEP_ASSERTION_TYPE: &str = "PreventUserIdleDisplaySleep";
    const SYSTEM_SLEEP_ASSERTION_TYPE: &str = "PreventUserIdleSystemSleep";

    static PRESENTATION_LEASE_STATE: Mutex<MacPresentationLeaseState> =
        Mutex::new(MacPresentationLeaseState {
            active_guards: 0,
            restore_options_bits: None,
        });

    // SAFETY: These declarations match IOKit's public C signatures. Assertion
    // IDs returned successfully must be released once through the paired function.
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: IOPMAssertionLevel,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionId,
        ) -> IOReturn;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionId) -> IOReturn;
    }

    pub(super) struct MacPresentationGuard {
        app: tauri::AppHandle,
        active: bool,
    }

    impl MacPresentationGuard {
        pub(super) fn start(app: &tauri::AppHandle) -> Result<Self, String> {
            let app_for_setup = app.clone();
            run_on_main_thread_blocking(app, move || {
                let mtm = MainThreadMarker::new()
                    .ok_or_else(|| "macOS presentation options need the main thread".to_string())?;
                let ns_app = NSApplication::sharedApplication(mtm);
                let previous_options_bits = ns_app.presentationOptions().0 as u64;
                PRESENTATION_LEASE_STATE
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .acquire(previous_options_bits)?;
                ns_app.setPresentationOptions(mac_overlay_presentation_options());
                Ok(())
            })?;

            Ok(Self {
                app: app_for_setup,
                active: true,
            })
        }
    }

    impl super::EnforcementCleanup for MacPresentationGuard {
        fn stop(&mut self) -> Result<(), String> {
            if !self.active {
                return Ok(());
            }
            run_on_main_thread_blocking(&self.app, move || {
                let mtm = MainThreadMarker::new()
                    .ok_or_else(|| "macOS presentation options need the main thread".to_string())?;
                let restore_options_bits = PRESENTATION_LEASE_STATE
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .release()?;
                if let Some(restore_options_bits) = restore_options_bits {
                    let ns_app = NSApplication::sharedApplication(mtm);
                    ns_app.setPresentationOptions(mac_presentation_options_from_bits(
                        restore_options_bits,
                    ));
                }
                Ok(())
            })?;
            self.active = false;
            Ok(())
        }
    }

    pub(super) struct MacPowerAssertionGuard {
        assertion_ids: Vec<IOPMAssertionId>,
    }

    impl MacPowerAssertionGuard {
        pub(super) fn start() -> Result<Self, String> {
            let mut assertion_ids = Vec::new();
            let display_type = CFString::from_static_str(DISPLAY_SLEEP_ASSERTION_TYPE);
            let display_id = create_power_assertion(&display_type)?;
            assertion_ids.push(display_id);

            let system_type = CFString::from_static_str(SYSTEM_SLEEP_ASSERTION_TYPE);
            match create_power_assertion(&system_type) {
                Ok(system_id) => assertion_ids.push(system_id),
                Err(err) => eprintln!("failed to create macOS system sleep assertion: {err}"),
            }

            Ok(Self { assertion_ids })
        }
    }

    impl super::EnforcementCleanup for MacPowerAssertionGuard {
        fn stop(&mut self) -> Result<(), String> {
            let mut failure = None;
            for assertion_id in self.assertion_ids.drain(..) {
                // SAFETY: Every stored ID came from one successful create call and
                // drain ensures it is submitted for release at most once.
                let result = unsafe { IOPMAssertionRelease(assertion_id) };
                if result != 0 && failure.is_none() {
                    failure = Some(format!(
                        "macOS power assertion {assertion_id} release returned {result}"
                    ));
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }

    fn create_power_assertion(
        assertion_type: &CFRetained<CFString>,
    ) -> Result<IOPMAssertionId, String> {
        let assertion_type_ref = CFRetained::as_ptr(assertion_type).as_ptr().cast::<c_void>();
        let name = CFString::from_static_str("Ganbaru AI Pomodoro overlay");
        let name_ref = CFRetained::as_ptr(&name).as_ptr().cast::<c_void>();
        let mut assertion_id = 0;
        // SAFETY: Both CFString pointers are non-null and remain alive for this
        // synchronous call. `assertion_id` is valid writable output storage and
        // is used only when IOKit reports success.
        let result = unsafe {
            IOPMAssertionCreateWithName(
                assertion_type_ref,
                K_IOPM_ASSERTION_LEVEL_ON,
                name_ref,
                &mut assertion_id,
            )
        };
        if result == 0 {
            Ok(assertion_id)
        } else {
            Err(format!("IOPMAssertionCreateWithName returned {result}"))
        }
    }

    fn mac_overlay_presentation_options() -> NSApplicationPresentationOptions {
        mac_presentation_options_from_bits(mac_overlay_presentation_options_bits())
    }

    fn mac_presentation_options_from_bits(bits: u64) -> NSApplicationPresentationOptions {
        NSApplicationPresentationOptions::from_bits_retain(bits as _)
    }

    pub(super) fn reinforce_overlay_windows(
        app: &tauri::AppHandle,
        labels: &[String],
        _primary_label: &str,
    ) -> Result<(), String> {
        let labels = labels.to_vec();
        let app_for_setup = app.clone();
        run_on_main_thread_blocking(app, move || {
            for label in labels {
                let Some(window) = app_for_setup.get_webview_window(&label) else {
                    continue;
                };
                let ns_window = window.ns_window().map_err(|e| e.to_string())?;
                let Some(ns_window) = std::ptr::NonNull::new(ns_window.cast::<NSWindow>()) else {
                    continue;
                };
                // SAFETY: Tauri returned the live NSWindow belonging to `window`.
                // The main-thread closure keeps that owner alive for this borrow.
                let ns_window = unsafe { ns_window.as_ref() };
                ns_window.setLevel(NSScreenSaverWindowLevel);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn event(
        key_code: u32,
        modifiers: WindowsOverlayShortcutModifiers,
    ) -> WindowsOverlayShortcutEvent {
        WindowsOverlayShortcutEvent {
            key_code,
            modifiers,
        }
    }

    #[test]
    fn windows_shortcut_filter_blocks_escape_routes() {
        assert!(should_block_windows_overlay_shortcut(event(
            VK_TAB_CODE,
            WindowsOverlayShortcutModifiers {
                alt: true,
                ..Default::default()
            },
        )));
        assert!(should_block_windows_overlay_shortcut(event(
            VK_ESCAPE_CODE,
            WindowsOverlayShortcutModifiers {
                alt: true,
                ..Default::default()
            },
        )));
        assert!(should_block_windows_overlay_shortcut(event(
            VK_D_CODE,
            WindowsOverlayShortcutModifiers {
                win: true,
                ..Default::default()
            },
        )));
        assert!(should_block_windows_overlay_shortcut(event(
            VK_ESCAPE_CODE,
            WindowsOverlayShortcutModifiers {
                ctrl: true,
                ..Default::default()
            },
        )));
        assert!(should_block_windows_overlay_shortcut(event(
            VK_LWIN_CODE,
            WindowsOverlayShortcutModifiers::default(),
        )));
    }

    #[test]
    fn windows_modifier_tracking_preserves_independent_key_sides() {
        let mut bits = 0;
        bits = update_windows_modifier_bits(bits, VK_LCONTROL_CODE, true);
        bits = update_windows_modifier_bits(bits, VK_RCONTROL_CODE, true);
        bits = update_windows_modifier_bits(bits, VK_LCONTROL_CODE, false);
        assert!(windows_modifiers_from_bits(bits, false).ctrl);

        bits = update_windows_modifier_bits(bits, VK_RCONTROL_CODE, false);
        assert!(!windows_modifiers_from_bits(bits, false).ctrl);
        assert!(windows_modifiers_from_bits(bits, true).alt);
    }

    #[test]
    fn windows_modifier_event_sequence_drives_shortcut_filtering() {
        let mut bits = 0;
        bits = update_windows_modifier_bits(bits, VK_LCONTROL_CODE, true);
        bits = update_windows_modifier_bits(bits, VK_RSHIFT_CODE, true);
        assert!(should_block_windows_overlay_shortcut(event(
            VK_ESCAPE_CODE,
            windows_modifiers_from_bits(bits, false),
        )));

        bits = update_windows_modifier_bits(bits, VK_LCONTROL_CODE, false);
        bits = update_windows_modifier_bits(bits, VK_RSHIFT_CODE, false);
        bits = update_windows_modifier_bits(bits, VK_LWIN_CODE, true);
        assert!(should_block_windows_overlay_shortcut(event(
            VK_D_CODE,
            windows_modifiers_from_bits(bits, false),
        )));
        bits = update_windows_modifier_bits(bits, VK_LWIN_CODE, false);
        assert!(!windows_modifiers_from_bits(bits, false).win);
    }

    #[test]
    fn windows_shortcut_filter_passes_overlay_controls_and_text() {
        assert!(!should_block_windows_overlay_shortcut(event(
            u32::from(b'A'),
            WindowsOverlayShortcutModifiers::default(),
        )));
        assert!(!should_block_windows_overlay_shortcut(event(
            VK_SPACE_CODE,
            WindowsOverlayShortcutModifiers::default(),
        )));
        assert!(!should_block_windows_overlay_shortcut(event(
            VK_ESCAPE_CODE,
            WindowsOverlayShortcutModifiers::default(),
        )));
        assert!(!should_block_windows_overlay_shortcut(event(
            VK_SPACE_CODE,
            WindowsOverlayShortcutModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        )));
    }

    #[test]
    fn windows_shortcut_filter_leaves_secure_attention_unclaimed() {
        assert!(!should_block_windows_overlay_shortcut(event(
            VK_DELETE_CODE,
            WindowsOverlayShortcutModifiers {
                ctrl: true,
                alt: true,
                ..Default::default()
            },
        )));
    }

    #[test]
    fn overlay_enforcement_guard_stops_cleanup_once_in_reverse_order() {
        let calls = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut guard = OverlayEnforcementGuard::new(&[], "primary");

        for id in [1, 2] {
            let calls = Arc::clone(&calls);
            let order = Arc::clone(&order);
            guard.push_cleanup(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                order
                    .lock()
                    .expect("test order lock should not be poisoned")
                    .push(id);
                Ok(())
            });
        }

        guard.stop();
        guard.stop();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            *order
                .lock()
                .expect("test order lock should not be poisoned"),
            vec![2, 1]
        );
        assert_eq!(guard.cleanup_len(), 0);
    }

    #[test]
    fn overlay_enforcement_guard_survives_partial_cleanup_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut guard = OverlayEnforcementGuard::new(&[], "primary");

        guard.push_cleanup(|| Err("first cleanup failed".to_string()));
        let calls_for_cleanup = Arc::clone(&calls);
        guard.push_cleanup(move || {
            calls_for_cleanup.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });

        guard.stop();

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(guard.cleanup_len(), 0);
    }

    #[test]
    fn overlay_enforcement_guard_close_without_start_is_safe() {
        let mut guard = OverlayEnforcementGuard::new(&[], "primary");

        guard.stop();
        guard.stop();

        assert_eq!(guard.cleanup_len(), 0);
    }

    #[test]
    fn overlay_reconcile_stop_joins_an_in_flight_iteration() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runs_for_worker = Arc::clone(&runs);
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let mut guard = OverlayReconcileGuard::start_with_interval(
            move || {
                runs_for_worker.fetch_add(1, Ordering::SeqCst);
                entered_tx
                    .send(())
                    .expect("test should observe the reconcile iteration");
                release_rx
                    .recv()
                    .expect("test should release the reconcile iteration");
                true
            },
            std::time::Duration::from_millis(1),
        )
        .expect("reconcile worker should start");

        entered_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("reconcile worker should enter its callback");
        guard.request_stop();
        release_tx
            .send(())
            .expect("reconcile worker should still be waiting");
        guard.stop().expect("reconcile worker should join");

        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn macos_presentation_options_use_only_kiosk_lite_bits() {
        let bits = mac_overlay_presentation_options_bits();

        assert_eq!(bits, MAC_APPROVED_PRESENTATION_BITS);
        assert_eq!(bits & (1 << 4), 0);
        assert_ne!(bits & MAC_PRESENTATION_HIDE_DOCK, 0);
        assert_ne!(bits & MAC_PRESENTATION_HIDE_MENU_BAR, 0);
        assert_ne!(bits & MAC_PRESENTATION_DISABLE_PROCESS_SWITCHING, 0);
        assert_ne!(bits & MAC_PRESENTATION_DISABLE_FORCE_QUIT, 0);
        assert_ne!(bits & MAC_PRESENTATION_DISABLE_SESSION_TERMINATION, 0);
        assert_ne!(bits & MAC_PRESENTATION_DISABLE_HIDE_APPLICATION, 0);
        assert_ne!(bits & MAC_PRESENTATION_FULL_SCREEN, 0);
    }

    #[test]
    fn macos_presentation_leases_restore_only_after_the_final_guard() {
        let mut state = MacPresentationLeaseState::default();

        state.acquire(7).expect("first lease should be acquired");
        state
            .acquire(MAC_APPROVED_PRESENTATION_BITS)
            .expect("overlapping lease should be acquired");
        assert_eq!(state.release().expect("newer lease should release"), None);
        assert_eq!(
            state.release().expect("final lease should release"),
            Some(7)
        );

        state.acquire(11).expect("later lease should be acquired");
        assert_eq!(
            state.release().expect("later lease should release"),
            Some(11)
        );
        assert!(state.release().is_err());
    }
}
