//! Shell-free provider process spawning and bounded diagnostics.

#![allow(
    unsafe_code,
    reason = "Retained unsafe boundary; see docs/data/security/unsafe-rust.md."
)]

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
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

/// Process tree of one short-lived helper command, such as Git.
///
/// The tree is stopped only through [`ProcessTree::terminate`], so descendants
/// that a successful command leaves behind (credential caches, signing agents)
/// keep running after the handle is dropped.
pub struct ProcessTree {
    #[cfg(unix)]
    process_group_id: libc::pid_t,
    #[cfg(windows)]
    process_job: std::os::windows::io::OwnedHandle,
}

impl ProcessTree {
    /// Claims the tree of a child spawned from a command prepared with
    /// [`configure_std_process_tree`]. On failure the child is killed.
    pub fn claim_std(child: &mut std::process::Child) -> ChatResult<Self> {
        #[cfg(unix)]
        {
            let process_group_id = libc::pid_t::try_from(child.id())
                .ok()
                .filter(|id| *id > 1)
                .ok_or_else(|| {
                    let _ = child.kill();
                    process_state_error()
                })?;
            Ok(Self { process_group_id })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let raw_handle = child.as_raw_handle();
            match claim_windows_job(raw_handle, child.id(), false) {
                Ok(process_job) => Ok(Self { process_job }),
                Err(error) => {
                    let _ = child.kill();
                    Err(spawn_error(error))
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    /// Claims the tree of a child spawned from a command prepared with
    /// `configure_process_tree`. On failure the child is killed.
    fn claim_tokio(child: &mut Child) -> ChatResult<Self> {
        #[cfg(unix)]
        {
            let process_group_id = child
                .id()
                .and_then(|id| libc::pid_t::try_from(id).ok())
                .filter(|id| *id > 1)
                .ok_or_else(|| {
                    let _ = child.start_kill();
                    process_state_error()
                })?;
            Ok(Self { process_group_id })
        }
        #[cfg(windows)]
        {
            let (Some(raw_handle), Some(process_id)) = (child.raw_handle(), child.id()) else {
                let _ = child.start_kill();
                return Err(process_state_error());
            };
            match claim_windows_job(raw_handle, process_id, false) {
                Ok(process_job) => Ok(Self { process_job }),
                Err(error) => {
                    let _ = child.start_kill();
                    Err(spawn_error(error))
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    /// Forcefully stops every process in the tree. Already stopped trees are
    /// not an error.
    pub fn terminate(&self) {
        #[cfg(unix)]
        let _ = signal_process_group(self.process_group_id, libc::SIGKILL);
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::HANDLE;
            use windows::Win32::System::JobObjects::TerminateJobObject;

            // SAFETY: `process_job` owns a live job handle for the duration of
            // the call. Termination does not transfer or close that handle.
            unsafe {
                let _ = TerminateJobObject(HANDLE(self.process_job.as_raw_handle()), 1);
            }
        }
    }
}

/// One short-lived provider probe, such as a version or account query.
pub struct BoundedCommand<'a> {
    pub executable: &'a Path,
    pub arguments: &'a [&'a str],
    pub working_directory: &'a Path,
    pub environment: &'a BTreeMap<String, String>,
    /// Largest stdout or stderr the probe may write; more stops the process tree.
    pub output_limit_bytes: usize,
    pub timeout: Duration,
}

/// Complete output of a probe that finished within its limits.
#[derive(Debug)]
pub struct BoundedCommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub successful: bool,
}

/// Why a probe produced no usable output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundedCommandFailure {
    /// The executable could not be started or its output could not be read.
    Unavailable,
    TimedOut,
    Oversized,
}

/// Runs a probe with a null stdin and a clean environment, reading each output
/// stream only up to its limit. A probe that writes more or runs past its
/// timeout has its whole process tree stopped, so a hostile or broken
/// executable cannot make the app buffer unbounded output.
pub async fn run_bounded_command(
    command: BoundedCommand<'_>,
) -> Result<BoundedCommandOutput, BoundedCommandFailure> {
    let mut process = Command::new(command.executable);
    process
        .args(command.arguments)
        .current_dir(command.working_directory)
        .env_clear()
        .envs(command.environment)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    configure_process_tree(&mut process);
    let mut child = process
        .spawn()
        .map_err(|_| BoundedCommandFailure::Unavailable)?;
    let tree =
        ProcessTree::claim_tokio(&mut child).map_err(|_| BoundedCommandFailure::Unavailable)?;
    let limit = command.output_limit_bytes;
    let result = tokio::time::timeout(command.timeout, async {
        let stdout = child
            .stdout
            .take()
            .ok_or(BoundedCommandFailure::Unavailable)?;
        let stderr = child
            .stderr
            .take()
            .ok_or(BoundedCommandFailure::Unavailable)?;
        let (stdout, stderr) =
            tokio::try_join!(read_capped(stdout, limit), read_capped(stderr, limit))?;
        let status = child
            .wait()
            .await
            .map_err(|_| BoundedCommandFailure::Unavailable)?;
        Ok(BoundedCommandOutput {
            stdout,
            stderr,
            successful: status.success(),
        })
    })
    .await
    .unwrap_or(Err(BoundedCommandFailure::TimedOut));
    if result.is_err() {
        tree.terminate();
    }
    result
}

async fn read_capped(
    reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, BoundedCommandFailure> {
    let mut output = Vec::new();
    reader
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut output)
        .await
        .map_err(|_| BoundedCommandFailure::Unavailable)?;
    if output.len() > limit {
        return Err(BoundedCommandFailure::Oversized);
    }
    Ok(output)
}

/// Prepares a standard-library command so its descendants form one process
/// tree that [`ProcessTree::claim_std`] can claim after spawning.
pub fn configure_std_process_tree(command: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED};

        command.creation_flags(CREATE_NEW_PROCESS_GROUP.0 | CREATE_SUSPENDED.0);
    }
    #[cfg(not(any(unix, windows)))]
    let _ = command;
}

fn configure_process_tree(command: &mut Command) {
    configure_std_process_tree(command.as_std_mut());
}

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
    let Some(raw_handle) = child.raw_handle() else {
        let _ = child.start_kill();
        return Err(process_state_error());
    };
    let Some(process_id) = child.id() else {
        let _ = child.start_kill();
        return Err(process_state_error());
    };
    claim_windows_job(raw_handle, process_id, true).map_err(|error| {
        let _ = child.start_kill();
        spawn_error(error)
    })
}

/// Assigns a suspended child to a new job object and resumes its main thread.
///
/// `child_handle` must be the live process handle of the suspended child whose
/// ID is `process_id`. With `kill_on_close`, closing the returned handle stops
/// every process in the job.
#[cfg(windows)]
fn claim_windows_job(
    child_handle: std::os::windows::io::RawHandle,
    process_id: u32,
    kill_on_close: bool,
) -> Result<std::os::windows::io::OwnedHandle, windows::core::Error> {
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

    {
        // SAFETY: Null security attributes and name request a private job object.
        // The returned noninvalid handle is immediately transferred to OwnedHandle.
        let raw_job = unsafe { CreateJobObjectW(None, None) }?;
        // SAFETY: CreateJobObjectW returned a new handle owned by this call, and no
        // other Rust owner exists. OwnedHandle will close it exactly once.
        let job = unsafe { OwnedHandle::from_raw_handle(raw_job.0) };
        let job_handle = HANDLE(job.as_raw_handle());
        if kill_on_close {
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `job_handle` is live for the call. `limits` has the exact
            // type and byte size required by Windows, and the call neither
            // retains the supplied pointer nor transfers handle ownership.
            unsafe {
                SetInformationJobObject(
                    job_handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )?;
            }
        }
        // SAFETY: The caller guarantees `child_handle` is the live process
        // handle of the suspended child, and `job_handle` is live. The call does
        // not transfer ownership of either handle.
        unsafe { AssignProcessToJobObject(job_handle, HANDLE(child_handle)) }?;

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
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{BoundedCommand, BoundedCommandFailure, run_bounded_command, signal_process_group};
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Duration;

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

    fn shell_probe<'a>(
        script: &'a [&'a str],
        environment: &'a BTreeMap<String, String>,
        output_limit_bytes: usize,
        timeout: Duration,
    ) -> BoundedCommand<'a> {
        BoundedCommand {
            executable: Path::new("/bin/sh"),
            arguments: script,
            working_directory: Path::new("/"),
            environment,
            output_limit_bytes,
            timeout,
        }
    }

    #[tokio::test]
    async fn bounded_probes_return_complete_output_and_exit_status() {
        let environment = BTreeMap::from([("PROBE_VALUE".to_string(), "visible".to_string())]);
        let output = run_bounded_command(shell_probe(
            &["-c", "printf \"$PROBE_VALUE\"; printf warning >&2; exit 3"],
            &environment,
            64,
            Duration::from_secs(10),
        ))
        .await
        .expect("probe within limits");
        assert_eq!(output.stdout, b"visible");
        assert_eq!(output.stderr, b"warning");
        assert!(!output.successful);

        let exact = run_bounded_command(shell_probe(
            &["-c", "printf 12345678"],
            &environment,
            8,
            Duration::from_secs(10),
        ))
        .await
        .expect("output exactly at the limit");
        assert_eq!(exact.stdout, b"12345678");
    }

    #[tokio::test]
    async fn bounded_probes_stop_endless_output_on_either_stream() {
        let environment = BTreeMap::new();
        for script in ["yes", "yes >&2"] {
            let started = std::time::Instant::now();
            let failure = run_bounded_command(shell_probe(
                &["-c", script],
                &environment,
                16 * 1024,
                Duration::from_secs(30),
            ))
            .await
            .expect_err("endless output must be rejected");
            assert_eq!(failure, BoundedCommandFailure::Oversized);
            assert!(started.elapsed() < Duration::from_secs(10));
        }
    }

    #[tokio::test]
    async fn bounded_probes_stop_the_whole_tree_after_the_timeout() {
        let environment = BTreeMap::new();
        let marker =
            std::env::temp_dir().join(format!("ganbaru-bounded-probe-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let script = format!("(sleep 2; touch '{}') & sleep 30", marker.display());
        let failure = run_bounded_command(shell_probe(
            &["-c", &script],
            &environment,
            64,
            Duration::from_millis(300),
        ))
        .await
        .expect_err("a hung probe must time out");
        assert_eq!(failure, BoundedCommandFailure::TimedOut);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(!marker.exists(), "a descendant outlived the stopped probe");
    }

    #[tokio::test]
    async fn bounded_probes_report_missing_executables_as_unavailable() {
        let environment = BTreeMap::new();
        let failure = run_bounded_command(BoundedCommand {
            executable: Path::new("/nonexistent/ganbaru-probe"),
            arguments: &[],
            working_directory: Path::new("/"),
            environment: &environment,
            output_limit_bytes: 64,
            timeout: Duration::from_secs(5),
        })
        .await
        .expect_err("missing executable");
        assert_eq!(failure, BoundedCommandFailure::Unavailable);
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
