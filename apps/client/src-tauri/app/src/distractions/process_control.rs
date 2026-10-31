use super::*;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DesktopProcessSignal {
    Term,
    Kill,
}

/// Desktop process access used by the close flow. A process is pinned before its identity is
/// checked, and signals go to the pinned process, so a PID reused after the check never
/// receives them.
#[cfg(target_os = "linux")]
pub(super) trait DesktopProcessController {
    type Pinned;

    /// Pins the process currently using `process_id`, or returns `None` when none does.
    fn pin(&mut self, process_id: u32) -> Result<Option<Self::Pinned>, String>;
    fn observe(&mut self, process_id: u32) -> Result<Option<ObservedDesktopProcess>, String>;
    fn signal(&mut self, pinned: &Self::Pinned, signal: DesktopProcessSignal)
    -> Result<(), String>;
    fn wait(&mut self, duration: std::time::Duration);
}

/// A Linux process reference that cannot drift to another process.
#[cfg(target_os = "linux")]
pub(super) enum PinnedLinuxProcess {
    /// A pidfd keeps naming the opened process after it exits, even if its PID is reused.
    Pidfd(rustix::fd::OwnedFd),
    /// Kernels before 5.3 have no `pidfd_open`; the signal goes by PID right after the same
    /// identity check.
    Pid(rustix::process::Pid),
}

#[cfg(target_os = "linux")]
pub(super) fn pin_linux_process(process_id: u32) -> Result<Option<PinnedLinuxProcess>, String> {
    let Some(pid) = i32::try_from(process_id)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return Ok(None);
    };
    match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
        Ok(pidfd) => Ok(Some(PinnedLinuxProcess::Pidfd(pidfd))),
        Err(rustix::io::Errno::SRCH) => Ok(None),
        Err(rustix::io::Errno::NOSYS) => Ok(Some(PinnedLinuxProcess::Pid(pid))),
        Err(error) => Err(format!("close app: open process handle: {error}")),
    }
}

#[cfg(target_os = "linux")]
pub(super) fn signal_linux_process(
    pinned: &PinnedLinuxProcess,
    signal: DesktopProcessSignal,
) -> Result<(), String> {
    let signal = match signal {
        DesktopProcessSignal::Term => rustix::process::Signal::TERM,
        DesktopProcessSignal::Kill => rustix::process::Signal::KILL,
    };
    match pinned {
        PinnedLinuxProcess::Pidfd(pidfd) => rustix::process::pidfd_send_signal(pidfd, signal),
        PinnedLinuxProcess::Pid(pid) => rustix::process::kill_process(*pid, signal),
    }
    .map_err(|error| format!("close app: {error}"))
}

#[cfg(target_os = "linux")]
struct SystemDesktopProcessController;

#[cfg(target_os = "linux")]
impl DesktopProcessController for SystemDesktopProcessController {
    type Pinned = PinnedLinuxProcess;

    fn pin(&mut self, process_id: u32) -> Result<Option<PinnedLinuxProcess>, String> {
        pin_linux_process(process_id)
    }

    fn observe(&mut self, process_id: u32) -> Result<Option<ObservedDesktopProcess>, String> {
        let process_path = PathBuf::from("/proc").join(process_id.to_string());
        if !process_path.exists() {
            return Ok(None);
        }
        Ok(observe_linux_process(&process_path, process_id))
    }

    fn signal(
        &mut self,
        pinned: &PinnedLinuxProcess,
        signal: DesktopProcessSignal,
    ) -> Result<(), String> {
        signal_linux_process(pinned, signal)
    }

    fn wait(&mut self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(target_os = "linux")]
fn validate_close_process_id(process_id: u32) -> Result<(), String> {
    if process_id <= 1 || process_id == std::process::id() {
        return Err("refusing to close protected process".to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn validate_observed_close_process(
    request: &DistractionsCloseDesktopAppRequest,
    observed: &ObservedDesktopProcess,
) -> Result<(), String> {
    validate_close_process_id(request.process_id)?;
    let process_name = normalize_process_match_name(&request.process_name)
        .ok_or_else(|| "observed process name is invalid".to_string())?;
    if request.process_identity.trim().is_empty()
        || request.process_identity.len() > 128
        || request.process_identity != observed.process_identity
    {
        return Err("process identity changed before it could be closed".to_string());
    }
    let observed_names = std::iter::once(observed.process_name.clone())
        .chain(observed.match_names.iter().cloned())
        .collect::<Vec<_>>();
    if observed_names
        .iter()
        .chain(observed.executable_name.iter())
        .any(|name| is_protected_observed_process_name(name))
    {
        return Err("refusing to close protected process".to_string());
    }
    if !observed_names
        .iter()
        .any(|name| app_name_key(name) == app_name_key(&process_name))
    {
        return Err("process name changed before it could be closed".to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn observe_exact_close_process<C: DesktopProcessController>(
    request: &DistractionsCloseDesktopAppRequest,
    controller: &mut C,
) -> Result<Option<ObservedDesktopProcess>, String> {
    let Some(observed) = controller.observe(request.process_id)? else {
        return Ok(None);
    };
    validate_observed_close_process(request, &observed)?;
    Ok(Some(observed))
}

#[cfg(target_os = "linux")]
pub(super) fn close_desktop_process_with<C, A>(
    request: &DistractionsCloseDesktopAppRequest,
    controller: &mut C,
    mut authorize: A,
) -> Result<bool, String>
where
    C: DesktopProcessController,
    A: FnMut(&ObservedDesktopProcess) -> Result<(), String>,
{
    let Some(pinned) = controller.pin(request.process_id)? else {
        return Ok(false);
    };
    let Some(observed) = observe_exact_close_process(request, controller)? else {
        return Ok(false);
    };
    authorize(&observed)?;
    if let Err(error) = controller.signal(&pinned, DesktopProcessSignal::Term) {
        if observe_exact_close_process(request, controller)?.is_none() {
            return Ok(false);
        }
        return Err(error);
    }
    for _ in 0..8 {
        controller.wait(std::time::Duration::from_millis(100));
        if observe_exact_close_process(request, controller)?.is_none() {
            return Ok(true);
        }
    }
    let Some(observed) = observe_exact_close_process(request, controller)? else {
        return Ok(true);
    };
    authorize(&observed)?;
    if let Err(error) = controller.signal(&pinned, DesktopProcessSignal::Kill) {
        if observe_exact_close_process(request, controller)?.is_none() {
            return Ok(true);
        }
        return Err(error);
    }
    Ok(true)
}

/// Recheck runtime authority and the `config` rules immediately before each process signal.
#[cfg(target_os = "linux")]
pub(super) fn close_desktop_process_checked<R: Runtime>(
    app: &tauri::AppHandle<R>,
    config: &Value,
    request: DistractionsCloseDesktopAppRequest,
    mut before_close: impl FnMut() -> Result<(), String>,
) -> Result<bool, String> {
    let mut controller = SystemDesktopProcessController;
    close_desktop_process_with(&request, &mut controller, |observed| {
        before_close()?;
        let authorization = load_close_authorization(app, config, &request.rule_identity)?;
        validate_names_authorized(observed.match_names.clone(), &authorization)
    })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn close_desktop_process_checked<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    _config: &Value,
    _request: DistractionsCloseDesktopAppRequest,
    _before_close: impl FnMut() -> Result<(), String>,
) -> Result<bool, String> {
    Err("desktop app closing is only available on Linux for now".to_string())
}
