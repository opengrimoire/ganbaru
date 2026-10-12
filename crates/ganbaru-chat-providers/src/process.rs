//! Shell-free provider process spawning and bounded diagnostics.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

const MAX_ARGUMENTS: usize = 256;
const MAX_ARGUMENT_BYTES: usize = 32 * 1024;
const MAX_ENVIRONMENT_ENTRIES: usize = 128;
const MAX_ENVIRONMENT_VALUE_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug)]
pub struct ProviderProcessConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<String, String>,
    pub stderr_limit_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedDiagnostic {
    pub text: String,
    pub truncated: bool,
}

pub struct ProviderProcessHandle {
    child: Child,
    #[cfg(unix)]
    process_group_id: libc::pid_t,
    #[cfg(windows)]
    process_job: std::os::windows::io::OwnedHandle,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Arc<Mutex<VecDeque<u8>>>,
    stderr_truncated: Arc<AtomicBool>,
    stderr_task: Option<JoinHandle<()>>,
    stopped: bool,
}

impl ProviderProcessHandle {
    pub fn process_id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn take_stdout(&mut self) -> ChatResult<ChildStdout> {
        self.stdout.take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "Provider process stdout was already claimed",
                false,
            )
        })
    }

    pub fn take_stdin(&mut self) -> ChatResult<ChildStdin> {
        self.stdin.take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "Provider process stdin was already claimed",
                false,
            )
        })
    }

    pub async fn write_stdin(&mut self, bytes: &[u8]) -> ChatResult<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Conflict,
                "Provider process stdin is closed",
                true,
            )
        })?;
        stdin.write_all(bytes).await.map_err(process_io_error)?;
        stdin.flush().await.map_err(process_io_error)
    }

    pub fn diagnostic(&self) -> ChatResult<BoundedDiagnostic> {
        let mut bytes = self.stderr.lock().map_err(|_| process_state_error())?;
        Ok(BoundedDiagnostic {
            text: String::from_utf8_lossy(bytes.make_contiguous()).into_owned(),
            truncated: self.stderr_truncated.load(Ordering::Relaxed),
        })
    }

    pub async fn stop(
        &mut self,
        graceful_deadline: Duration,
        force_deadline: Duration,
    ) -> ChatResult<()> {
        if self.stopped {
            return Ok(());
        }
        self.stdin.take();
        match tokio::time::timeout(graceful_deadline, self.child.wait()).await {
            Ok(Ok(_)) => {
                self.signal_process_tree(true).await?;
                return self.finish_stop().await;
            }
            Ok(Err(error)) => return Err(process_io_error(error)),
            Err(_) => {}
        }
        self.signal_process_tree(false).await?;
        match tokio::time::timeout(force_deadline, self.child.wait()).await {
            Ok(Ok(_)) => {
                #[cfg(unix)]
                self.signal_process_tree(true).await?;
            }
            Ok(Err(error)) => return Err(process_io_error(error)),
            Err(_) => {
                self.signal_process_tree(true).await?;
                tokio::time::timeout(force_deadline, self.child.wait())
                    .await
                    .map_err(|_| process_timeout_error())?
                    .map_err(process_io_error)?;
            }
        }
        self.finish_stop().await
    }

    async fn finish_stop(&mut self) -> ChatResult<()> {
        self.stopped = true;
        if let Some(task) = self.stderr_task.take() {
            task.await.map_err(|_| process_state_error())?;
        }
        Ok(())
    }

    #[cfg(unix)]
    async fn signal_process_tree(&mut self, force: bool) -> ChatResult<()> {
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        match signal_process_group(self.process_group_id, signal) {
            Ok(()) => Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            Err(error) => Err(process_io_error(error)),
        }
    }

    #[cfg(windows)]
    async fn signal_process_tree(&mut self, _force: bool) -> ChatResult<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::JobObjects::TerminateJobObject;

        let job = HANDLE(self.process_job.as_raw_handle());
        // SAFETY: `process_job` owns a live job handle for the duration of the
        // call. Termination does not transfer or close that handle.
        unsafe { TerminateJobObject(job, 1) }.map_err(process_io_error)
    }

    #[cfg(not(any(unix, windows)))]
    async fn signal_process_tree(&mut self, _force: bool) -> ChatResult<()> {
        self.child.kill().await.map_err(process_io_error)
    }
}

impl Drop for ProviderProcessHandle {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        #[cfg(unix)]
        let _ = signal_process_group(self.process_group_id, libc::SIGKILL);
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::JobObjects::TerminateJobObject;

            // SAFETY: `process_job` remains a live owned job handle during Drop.
            // This best-effort call neither transfers nor closes the handle.
            unsafe {
                let _ = TerminateJobObject(HANDLE(self.process_job.as_raw_handle()), 1);
            }
        }
        let _ = self.child.start_kill();
    }
}

pub fn spawn_provider_process(config: ProviderProcessConfig) -> ChatResult<ProviderProcessHandle> {
    validate_config(&config)?;
    let mut command = Command::new(&config.executable);
    command
        .args(&config.arguments)
        .current_dir(&config.working_directory)
        .env_clear()
        .envs(&config.environment)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    configure_process_tree(&mut command);
    let mut child = command.spawn().map_err(spawn_error)?;
    #[cfg(unix)]
    let process_group_id = child
        .id()
        .and_then(|id| libc::pid_t::try_from(id).ok())
        .filter(|id| *id > 1)
        .ok_or_else(process_state_error)?;
    #[cfg(windows)]
    let process_job = claim_windows_process_tree(&mut child)?;
    let stdin = child.stdin.take().ok_or_else(process_state_error)?;
    let stdout = child.stdout.take().ok_or_else(process_state_error)?;
    let mut stderr_reader = child.stderr.take().ok_or_else(process_state_error)?;
    let stderr = Arc::new(Mutex::new(VecDeque::with_capacity(
        config.stderr_limit_bytes,
    )));
    let stderr_buffer = Arc::clone(&stderr);
    let stderr_truncated = Arc::new(AtomicBool::new(false));
    let truncated = Arc::clone(&stderr_truncated);
    let limit = config.stderr_limit_bytes;
    let stderr_task = tokio::spawn(async move {
        let mut chunk = [0_u8; 4096];
        while let Ok(read) = stderr_reader.read(&mut chunk).await {
            if read == 0 {
                break;
            }
            let Ok(mut buffer) = stderr_buffer.lock() else {
                break;
            };
            for byte in &chunk[..read] {
                if buffer.len() == limit {
                    buffer.pop_front();
                    truncated.store(true, Ordering::Relaxed);
                }
                buffer.push_back(*byte);
            }
        }
    });
    Ok(ProviderProcessHandle {
        child,
        #[cfg(unix)]
        process_group_id,
        #[cfg(windows)]
        process_job,
        stdin: Some(stdin),
        stdout: Some(stdout),
        stderr,
        stderr_truncated,
        stderr_task: Some(stderr_task),
        stopped: false,
    })
}

fn validate_config(config: &ProviderProcessConfig) -> ChatResult<()> {
    if !config.executable.is_absolute()
        || !config.executable.is_file()
        || !config.working_directory.is_absolute()
        || !config.working_directory.is_dir()
        || config.arguments.len() > MAX_ARGUMENTS
        || config
            .arguments
            .iter()
            .any(|value| value.len() > MAX_ARGUMENT_BYTES || value.contains('\0'))
        || config.environment.len() > MAX_ENVIRONMENT_ENTRIES
        || config.environment.iter().any(|(key, value)| {
            key.is_empty()
                || key.contains(['=', '\0'])
                || value.len() > MAX_ENVIRONMENT_VALUE_BYTES
                || value.contains('\0')
        })
        || config.stderr_limit_bytes == 0
        || config.stderr_limit_bytes > 1024 * 1024
    {
        return Err(ChatError::validation(
            "process",
            "Provider process configuration is invalid",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn configure_process_tree(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(windows)]
fn configure_process_tree(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

    command
        .as_std_mut()
        .creation_flags(CREATE_NEW_PROCESS_GROUP.0 | CREATE_SUSPENDED.0);
}

#[cfg(not(any(unix, windows)))]
fn configure_process_tree(_command: &mut Command) {}

#[cfg(unix)]
fn signal_process_group(process_group_id: libc::pid_t, signal: libc::c_int) -> std::io::Result<()> {
    if !matches!(signal, libc::SIGTERM | libc::SIGKILL) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provider process signal is invalid",
        ));
    }
    if process_group_id <= 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provider process group ID is invalid",
        ));
    }
    let target = process_group_id.checked_neg().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provider process group ID is invalid",
        )
    })?;
    // SAFETY: `target` is the checked negative form of a group ID greater than
    // one, so it is less than -1 and cannot select all permitted processes. The
    // group was established for the child by `process_group(0)`. The signal is
    // one of the two validated constants above, and this call borrows no Rust memory.
    if unsafe { libc::kill(target, signal) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn claim_windows_process_tree(child: &mut Child) -> ChatResult<std::os::windows::io::OwnedHandle> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows::Win32::Foundation::{ERROR_INVALID_DATA, ERROR_INVALID_STATE, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::Win32::System::Threading::{
        GetProcessIdOfThread, OpenThread, ResumeThread, THREAD_QUERY_LIMITED_INFORMATION,
        THREAD_SUSPEND_RESUME,
    };
    use windows::core::HRESULT;

    let setup = || -> Result<OwnedHandle, windows::core::Error> {
        // SAFETY: Null security attributes and name request a private job object.
        // The returned noninvalid handle is immediately transferred to OwnedHandle.
        let raw_job = unsafe { CreateJobObjectW(None, None) }?;
        // SAFETY: CreateJobObjectW returned a new handle owned by this call, and no
        // other Rust owner exists. OwnedHandle will close it exactly once.
        let job = unsafe { OwnedHandle::from_raw_handle(raw_job.0) };
        let job_handle = HANDLE(job.as_raw_handle());
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `job_handle` and the child process handle are live for the
        // calls. `limits` has the exact type and byte size required by Windows;
        // neither call retains the supplied pointer or transfers handle ownership.
        unsafe {
            SetInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )?;
            let child_handle = child
                .raw_handle()
                .ok_or_else(windows::core::Error::from_win32)?;
            AssignProcessToJobObject(job_handle, HANDLE(child_handle))?;
        }

        let process_id = child.id().ok_or_else(windows::core::Error::from_win32)?;
        // SAFETY: This requests a read-only system thread snapshot and returns a
        // new handle on success.
        let raw_snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }?;
        // SAFETY: The successful snapshot call returned a new owned handle. It is
        // transferred immediately and will be closed exactly once.
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw_snapshot.0) };
        let entry_size = size_of::<THREADENTRY32>() as u32;
        let required_entry_size =
            (std::mem::offset_of!(THREADENTRY32, th32OwnerProcessID) + size_of::<u32>()) as u32;
        let new_thread_entry = || THREADENTRY32 {
            dwSize: entry_size,
            ..THREADENTRY32::default()
        };
        let mut entry = new_thread_entry();
        // SAFETY: `snapshot` stays open and `entry` is a writable output buffer
        // with dwSize initialized to the exact structure size.
        unsafe { Thread32First(HANDLE(snapshot.as_raw_handle()), &mut entry) }?;
        loop {
            if entry.dwSize < required_entry_size {
                return Err(windows::core::Error::new(
                    HRESULT::from_win32(ERROR_INVALID_DATA.0),
                    "ToolHelp returned a truncated thread entry",
                ));
            }
            if entry.th32OwnerProcessID == process_id {
                // SAFETY: The thread ID came from the live snapshot. The requested
                // access is limited to identity validation and resuming the newly
                // created child thread.
                let raw_thread = unsafe {
                    OpenThread(
                        THREAD_SUSPEND_RESUME | THREAD_QUERY_LIMITED_INFORMATION,
                        false,
                        entry.th32ThreadID,
                    )
                }?;
                // SAFETY: OpenThread returned a new handle owned by this call. No
                // other Rust value owns it, so OwnedHandle closes it exactly once.
                let thread = unsafe { OwnedHandle::from_raw_handle(raw_thread.0) };
                // SAFETY: `thread` is a live handle with query access. The return
                // value is an integer process ID and no pointer or ownership is
                // transferred. Revalidating after OpenThread closes the TID reuse
                // window between the Tool Help snapshot and handle acquisition.
                let opened_process_id =
                    unsafe { GetProcessIdOfThread(HANDLE(thread.as_raw_handle())) };
                if opened_process_id == 0 {
                    return Err(windows::core::Error::from_win32());
                }
                if opened_process_id != process_id {
                    return Err(windows::core::Error::new(
                        HRESULT::from_win32(ERROR_INVALID_DATA.0),
                        "ToolHelp thread identity changed before it was opened",
                    ));
                }
                // SAFETY: `thread` is a live handle with THREAD_SUSPEND_RESUME
                // access. CREATE_SUSPENDED establishes an initial count of one.
                let previous_suspend_count =
                    unsafe { ResumeThread(HANDLE(thread.as_raw_handle())) };
                if previous_suspend_count == u32::MAX {
                    return Err(windows::core::Error::from_win32());
                }
                if previous_suspend_count != 1 {
                    return Err(windows::core::Error::new(
                        HRESULT::from_win32(ERROR_INVALID_STATE.0),
                        "Suspended provider thread had an unexpected suspend count",
                    ));
                }
                return Ok(job);
            }
            entry = new_thread_entry();
            // SAFETY: The snapshot remains open and `entry` remains a writable
            // buffer whose dwSize field was reset to the full structure size.
            unsafe { Thread32Next(HANDLE(snapshot.as_raw_handle()), &mut entry) }?;
        }
    };

    setup().map_err(|error| {
        let _ = child.start_kill();
        spawn_error(error)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::signal_process_group;

    #[test]
    fn process_group_signaling_rejects_broad_targets() {
        for process_group_id in [1, 0, -1, libc::pid_t::MIN] {
            let error = signal_process_group(process_group_id, libc::SIGTERM)
                .expect_err("broad process targets must be rejected");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn process_group_signaling_rejects_unapproved_signals() {
        for signal in [0, libc::SIGSTOP] {
            let error =
                signal_process_group(1, signal).expect_err("unapproved signals must be rejected");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        }
    }
}

fn spawn_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Provider process could not be started",
        true,
    )
}
fn process_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Provider process communication failed",
        true,
    )
}
fn process_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Provider process state is unavailable",
        false,
    )
}
fn process_timeout_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::DriverUnavailable,
        "Provider process did not stop before the force deadline",
        true,
    )
}
