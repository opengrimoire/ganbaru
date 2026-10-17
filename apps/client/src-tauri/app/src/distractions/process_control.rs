use super::*;

#[cfg(target_os = "linux")]
fn signal_desktop_process(process_id: u32, signal: &str) -> Result<(), String> {
    let status = std::process::Command::new("kill")
        .arg(signal)
        .arg(process_id.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("close app: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("close app failed with status {status}"))
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DesktopProcessSignal {
    Term,
    Kill,
}

#[cfg(target_os = "linux")]
pub(super) trait DesktopProcessController {
    fn observe(&mut self, process_id: u32) -> Result<Option<ObservedDesktopProcess>, String>;
    fn signal(&mut self, process_id: u32, signal: DesktopProcessSignal) -> Result<(), String>;
    fn wait(&mut self, duration: std::time::Duration);
}

#[cfg(target_os = "linux")]
struct SystemDesktopProcessController;

#[cfg(target_os = "linux")]
impl DesktopProcessController for SystemDesktopProcessController {
    fn observe(&mut self, process_id: u32) -> Result<Option<ObservedDesktopProcess>, String> {
        let process_path = PathBuf::from("/proc").join(process_id.to_string());
        if !process_path.exists() {
            return Ok(None);
        }
        Ok(observe_linux_process(&process_path, process_id))
    }

    fn signal(&mut self, process_id: u32, signal: DesktopProcessSignal) -> Result<(), String> {
        signal_desktop_process(
            process_id,
            match signal {
                DesktopProcessSignal::Term => "-TERM",
                DesktopProcessSignal::Kill => "-KILL",
            },
        )
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
        .any(|name| is_protected_desktop_app_name(name))
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
    let Some(observed) = observe_exact_close_process(request, controller)? else {
        return Ok(false);
    };
    authorize(&observed)?;
    if let Err(error) = controller.signal(request.process_id, DesktopProcessSignal::Term) {
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
    controller.signal(request.process_id, DesktopProcessSignal::Kill)?;
    Ok(true)
}

/// Recheck runtime authority immediately before each process signal.
#[cfg(target_os = "linux")]
pub(super) fn close_desktop_process_checked<R: Runtime>(
    app: &tauri::AppHandle<R>,
    request: DistractionsCloseDesktopAppRequest,
    mut before_close: impl FnMut() -> Result<(), String>,
) -> Result<bool, String> {
    let mut controller = SystemDesktopProcessController;
    close_desktop_process_with(&request, &mut controller, |observed| {
        before_close()?;
        let authorization = load_close_authorization(app, &request.rule_identity)?;
        validate_names_authorized(observed.match_names.clone(), &authorization)
    })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn close_desktop_process_checked<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    _request: DistractionsCloseDesktopAppRequest,
    _before_close: impl FnMut() -> Result<(), String>,
) -> Result<bool, String> {
    Err("desktop app closing is only available on Linux for now".to_string())
}
