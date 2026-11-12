//! Inventory of the long-lived processes Chat starts, with read-only descendant listing and an
//! identity-checked stop.
//!
//! A record lives for as long as its [`ProcessRegistration`] guard. Stopping signals the process
//! group of a registered root only after a fresh read confirms that the process ID still names
//! the process that was registered, so a reused process ID is never signaled.

use super::systemd_scope::ResourceScope;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
#[cfg(target_os = "linux")]
use std::time::Duration;

/// Largest number of descendant processes one list reports; the rest are counted as omitted.
pub const MAX_LISTED_PROCESSES: usize = 256;
/// Longest executable name a record carries, in bytes.
const MAX_EXECUTABLE_NAME_BYTES: usize = 255;
/// Longest provider instance label a record carries, in bytes.
const MAX_LABEL_BYTES: usize = 256;
#[cfg(target_os = "linux")]
const STOP_GRACE: Duration = Duration::from_secs(3);
#[cfg(target_os = "linux")]
const STOP_POLL: Duration = Duration::from_millis(100);

/// Why a provider process runs, set on its launch configuration.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProviderProcessPurpose {
    /// A provider session or connection.
    #[default]
    Session,
    /// A short provider query, such as a version check.
    Probe,
    /// A local server a provider session talks to.
    LocalServer,
}

/// Why a listed Chat process runs.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProcessPurpose {
    Session,
    Probe,
    LocalServer,
    /// The user's shell in a Chat terminal.
    Terminal,
}

impl From<ProviderProcessPurpose> for ChatProcessPurpose {
    fn from(purpose: ProviderProcessPurpose) -> Self {
        match purpose {
            ProviderProcessPurpose::Session => Self::Session,
            ProviderProcessPurpose::Probe => Self::Probe,
            ProviderProcessPurpose::LocalServer => Self::LocalServer,
        }
    }
}

/// Kernel start time of a process, in clock ticks since boot, which tells a process apart from a
/// later one that reuses its process ID.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProcessStartTime(pub u64);

/// One process Chat started.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProcessRecord {
    pub pid: u32,
    /// Absent where the platform reports no start time; such a process can be listed, not stopped.
    pub start_time: Option<ProcessStartTime>,
    pub executable_name: String,
    pub provider_instance_label: Option<String>,
    pub purpose: ChatProcessPurpose,
    /// RFC 3339 time the process was registered.
    pub started_at: String,
    /// Whether [`stop_process`] can stop this process on this platform.
    pub stoppable: bool,
    /// Whether the process runs in a resource scope; absent for processes that are never scoped.
    pub resource_scope: Option<ResourceScope>,
}

/// A process started by a listed root or by one of its descendants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProcessDescendantRead {
    pub pid: u32,
    pub parent_pid: u32,
    /// Process ID of the listed root this process descends from.
    pub root_pid: u32,
    pub executable_name: String,
}

/// Running Chat processes and their descendants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProcessListRead {
    pub roots: Vec<ChatProcessRecord>,
    pub descendants: Vec<ChatProcessDescendantRead>,
    /// Descendants left out once [`MAX_LISTED_PROCESSES`] were listed.
    pub omitted: u32,
}

static INVENTORY: LazyLock<Mutex<BTreeMap<u64, ChatProcessRecord>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
static NEXT_REGISTRATION: AtomicU64 = AtomicU64::new(1);

/// Keeps a process in the inventory; dropping it removes the record.
#[derive(Debug)]
pub struct ProcessRegistration {
    id: u64,
}

impl Drop for ProcessRegistration {
    fn drop(&mut self) {
        inventory().remove(&self.id);
    }
}

/// Registers a process Chat just started, while it is still unreaped so its process ID cannot
/// have been reused.
///
/// # Parameters
/// - `pid`: Process ID of the started process, also its process group ID.
/// - `executable`: Executable path; only its file name is kept.
/// - `provider_instance_label`: Label of the provider instance that started it, if any.
/// - `purpose`: Why the process runs.
/// - `resource_scope`: Whether the process runs in a resource scope, or `None` when its purpose is
///   never scoped.
pub fn register_process(
    pid: u32,
    executable: &Path,
    provider_instance_label: Option<&str>,
    purpose: ChatProcessPurpose,
    resource_scope: Option<ResourceScope>,
) -> ProcessRegistration {
    let start_time = current_start_time(pid);
    let record = ChatProcessRecord {
        pid,
        start_time,
        executable_name: bounded(
            &executable
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| executable.to_string_lossy().into_owned()),
            MAX_EXECUTABLE_NAME_BYTES,
        ),
        provider_instance_label: provider_instance_label
            .map(|label| bounded(label, MAX_LABEL_BYTES)),
        purpose,
        started_at: chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        stoppable: cfg!(target_os = "linux") && start_time.is_some(),
        resource_scope,
    };
    let id = NEXT_REGISTRATION.fetch_add(1, Ordering::Relaxed);
    inventory().insert(id, record);
    ProcessRegistration { id }
}

/// Lists registered processes that are still running, then their descendants up to
/// [`MAX_LISTED_PROCESSES`]. Descendants are listed only on Linux.
pub fn list_processes() -> ChatProcessListRead {
    let mut roots = inventory()
        .values()
        .filter(|record| still_running(record))
        .cloned()
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| {
        left.started_at
            .cmp(&right.started_at)
            .then(left.pid.cmp(&right.pid))
    });
    let (descendants, omitted) = list_descendants(&roots);
    ChatProcessListRead {
        roots,
        descendants,
        omitted,
    }
}

/// Stops one registered process and its process group: a terminate signal, then a kill signal
/// when it outlives a short grace period.
///
/// # Errors
/// - `Validation` for the app's own process ID or a process ID at or below one.
/// - `ProcessNotFound` when no running registered process has this ID and start time.
/// - `ProcessIdentityChanged` when the process ID now names another process, or the process no
///   longer leads its own process group.
/// - `CapabilityUnsupported` on platforms without a start time.
pub async fn stop_process(pid: u32, start_time: ProcessStartTime) -> ChatResult<()> {
    if pid <= 1 || pid == std::process::id() {
        return Err(ChatError::validation(
            "pid",
            "This process cannot be stopped from Chat",
        ));
    }
    stop_registered(pid, start_time).await
}

#[cfg(target_os = "linux")]
async fn stop_registered(pid: u32, start_time: ProcessStartTime) -> ChatResult<()> {
    signal_registered(pid, start_time, libc::SIGTERM)?;
    let deadline = tokio::time::Instant::now() + STOP_GRACE;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(STOP_POLL).await;
        if !alive_as(pid, start_time) {
            return Ok(());
        }
    }
    match signal_registered(pid, start_time, libc::SIGKILL) {
        Err(error) if error.code == ChatErrorCode::ProcessNotFound => Ok(()),
        result => result,
    }
}

#[cfg(not(target_os = "linux"))]
async fn stop_registered(_pid: u32, _start_time: ProcessStartTime) -> ChatResult<()> {
    Err(ChatError::new(
        ChatErrorCode::CapabilityUnsupported,
        "Stopping one Chat process is not supported on this platform",
        false,
    ))
}

/// Signals the group of a registered process after confirming its identity. The inventory lock
/// is held across the check and the signal, so a record cannot change in between.
#[cfg(target_os = "linux")]
fn signal_registered(
    pid: u32,
    start_time: ProcessStartTime,
    signal: libc::c_int,
) -> ChatResult<()> {
    let records = inventory();
    if !records
        .values()
        .any(|record| record.pid == pid && record.start_time == Some(start_time))
    {
        return Err(process_not_found());
    }
    let stat = linux::read_stat(pid).ok_or_else(process_not_found)?;
    if stat.start_time != start_time {
        return Err(process_identity_changed());
    }
    if stat.zombie {
        return Err(process_not_found());
    }
    if stat.process_group != pid {
        return Err(process_identity_changed());
    }
    let group = libc::pid_t::try_from(pid).map_err(|_| process_identity_changed())?;
    match super::signal_process_group(group, signal) {
        Ok(()) => Ok(()),
        Err(error) if error.raw_os_error() == Some(libc::ESRCH) => Err(process_not_found()),
        Err(error) => Err(ChatError::new(
            ChatErrorCode::Permission,
            format!("Chat process could not be signaled: {error}"),
            true,
        )),
    }
}

#[cfg(target_os = "linux")]
fn alive_as(pid: u32, start_time: ProcessStartTime) -> bool {
    linux::read_stat(pid).is_some_and(|stat| stat.start_time == start_time && !stat.zombie)
}

fn still_running(record: &ChatProcessRecord) -> bool {
    #[cfg(target_os = "linux")]
    {
        record
            .start_time
            .is_some_and(|start_time| alive_as(record.pid, start_time))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = record;
        true
    }
}

fn current_start_time(pid: u32) -> Option<ProcessStartTime> {
    #[cfg(target_os = "linux")]
    {
        linux::read_stat(pid).map(|stat| stat.start_time)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

fn list_descendants(roots: &[ChatProcessRecord]) -> (Vec<ChatProcessDescendantRead>, u32) {
    #[cfg(target_os = "linux")]
    {
        let root_pids = roots.iter().map(|record| record.pid).collect::<Vec<_>>();
        linux::descendants(&root_pids, &linux::scan_processes(), MAX_LISTED_PROCESSES)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = roots;
        (Vec::new(), 0)
    }
}

fn inventory() -> MutexGuard<'static, BTreeMap<u64, ChatProcessRecord>> {
    INVENTORY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn bounded(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_string();
    }
    let mut end = maximum_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_string()
}

fn process_not_found() -> ChatError {
    ChatError::new(
        ChatErrorCode::ProcessNotFound,
        "The Chat process is no longer running",
        true,
    )
}

fn process_identity_changed() -> ChatError {
    ChatError::new(
        ChatErrorCode::ProcessIdentityChanged,
        "The process ID now belongs to a different process",
        true,
    )
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{ChatProcessDescendantRead, MAX_EXECUTABLE_NAME_BYTES, ProcessStartTime, bounded};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    /// Fields of `/proc/<pid>/stat` the inventory reads.
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(super) struct ProcessStat {
        pub(super) name: String,
        pub(super) zombie: bool,
        pub(super) parent_pid: u32,
        pub(super) process_group: u32,
        pub(super) start_time: ProcessStartTime,
    }

    pub(super) fn read_stat(pid: u32) -> Option<ProcessStat> {
        parse_stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }

    /// Parses one stat line. The name sits in parentheses and may itself hold spaces and
    /// parentheses, so the fields after it are read from the last closing parenthesis.
    pub(super) fn parse_stat(text: &str) -> Option<ProcessStat> {
        let open = text.find('(')?;
        let close = text.rfind(')')?;
        let name = text.get(open + 1..close)?;
        let fields = text
            .get(close + 1..)?
            .split_whitespace()
            .collect::<Vec<_>>();
        Some(ProcessStat {
            name: bounded(name, MAX_EXECUTABLE_NAME_BYTES),
            zombie: matches!(*fields.first()?, "Z" | "X"),
            parent_pid: fields.get(1)?.parse().ok()?,
            process_group: fields.get(2)?.parse().ok()?,
            start_time: ProcessStartTime(fields.get(19)?.parse().ok()?),
        })
    }

    /// Every readable process, keyed by process ID.
    pub(super) fn scan_processes() -> BTreeMap<u32, ProcessStat> {
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return BTreeMap::new();
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| Some((pid, read_stat(pid)?)))
            .collect()
    }

    /// Walks the process tree below each root, breadth first, listing at most `limit` processes
    /// and counting the rest.
    pub(super) fn descendants(
        roots: &[u32],
        processes: &BTreeMap<u32, ProcessStat>,
        limit: usize,
    ) -> (Vec<ChatProcessDescendantRead>, u32) {
        let mut children = BTreeMap::<u32, Vec<u32>>::new();
        for (pid, stat) in processes {
            children.entry(stat.parent_pid).or_default().push(*pid);
        }
        let mut visited = roots.iter().copied().collect::<BTreeSet<_>>();
        let mut listed = Vec::new();
        let mut omitted = 0_u32;
        for root in roots {
            let mut queue = VecDeque::from([*root]);
            while let Some(parent) = queue.pop_front() {
                for child in children.get(&parent).into_iter().flatten() {
                    if !visited.insert(*child) {
                        continue;
                    }
                    queue.push_back(*child);
                    if listed.len() < limit {
                        listed.push(ChatProcessDescendantRead {
                            pid: *child,
                            parent_pid: parent,
                            root_pid: *root,
                            executable_name: processes[child].name.clone(),
                        });
                    } else {
                        omitted = omitted.saturating_add(1);
                    }
                }
            }
        }
        (listed, omitted)
    }
}

#[cfg(test)]
mod tests;
