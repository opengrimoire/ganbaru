//! Linux user systemd resource scope for provider processes.
//!
//! On Linux, provider processes start inside a transient `systemd-run --user --scope` unit with
//! low CPU and IO weight, a task ceiling, and memory ceilings, so a runaway provider cannot starve
//! the desktop. The scope is optional: when user systemd is missing, sandboxed, or fails the probe,
//! providers start directly. Scope mode replaces `systemd-run` with the provider in place, so the
//! process ID, start time, and process group stay the provider's own.
//!
//! `systemd-run` needs the user bus variables, and the scope would pass them on to the provider,
//! whose allowlisted environment leaves them out on purpose. The scope therefore starts the
//! provider through `env -u`, which removes every variable the wrapper added before the provider
//! runs.

use super::{BoundedCommand, run_bounded_command};
use crate::events::NotificationEvent;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Absolute `systemd-run` locations; the probe never searches `PATH`.
pub const SYSTEMD_RUN_CANDIDATES: [&str; 2] = ["/usr/bin/systemd-run", "/bin/systemd-run"];
/// Absolute `env` locations used to remove the wrapper variables before the provider starts.
const ENV_CANDIDATES: [&str; 2] = ["/usr/bin/env", "/bin/env"];
/// File that marks a Flatpak sandbox, where user systemd scopes are not reachable.
const FLATPAK_MARKER: &str = "/.flatpak-info";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const PROBE_OUTPUT_LIMIT_BYTES: usize = 4 * 1024;
/// Variables `systemd-run` needs to reach the user manager through a cleared environment.
const SCOPE_ENVIRONMENT: [&str; 2] = ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"];
/// Variable the scope sets for the process it starts.
const SCOPE_INVOCATION_VARIABLE: &str = "INVOCATION_ID";
const GIB: u64 = 1024 * 1024 * 1024;
const SCOPE_PROPERTIES: [&str; 3] = ["CPUWeight=10", "IOWeight=10", "TasksMax=25%"];

/// Whether a provider process runs inside a resource scope, as the Chat process list reports it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceScope {
    /// The process runs in a user systemd scope with resource ceilings.
    Scoped,
    /// The process started directly, because no scope was available on this device.
    #[default]
    Unavailable,
}

/// Memory ceilings of one scope, in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryCeilings {
    /// Reclaim pressure starts here.
    pub high_bytes: u64,
    /// The kernel stops a process in the scope above this.
    pub max_bytes: u64,
}

/// Ceilings that leave the desktop at least 6 GiB before reclaim and 4 GiB before a kill on large
/// machines, and never take more than half and three quarters of memory on small ones.
pub fn memory_ceilings(total_bytes: u64) -> MemoryCeilings {
    MemoryCeilings {
        high_bytes: total_bytes.saturating_sub(6 * GIB).max(total_bytes / 2),
        max_bytes: total_bytes
            .saturating_sub(4 * GIB)
            .max(total_bytes - total_bytes / 4),
    }
}

/// Reads `MemTotal` from `/proc/meminfo` content, in bytes.
pub fn parse_mem_total(meminfo: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?;
    let mut parts = line.split_whitespace();
    let value = parts.next()?.parse::<u64>().ok()?;
    match parts.next() {
        Some("kB") => value.checked_mul(1024),
        _ => None,
    }
}

/// The two programs a scoped launch goes through.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeLauncher {
    /// `systemd-run`, which creates the scope and replaces itself with `env`.
    pub runner: PathBuf,
    /// `env`, which removes the wrapper variables and replaces itself with the provider.
    pub env: PathBuf,
}

/// Variables the wrapper adds that the provider's configured environment does not set.
pub fn wrapper_only_variables(configured: &BTreeMap<String, String>) -> Vec<&'static str> {
    SCOPE_ENVIRONMENT
        .into_iter()
        .chain([SCOPE_INVOCATION_VARIABLE])
        .filter(|key| !configured.contains_key(*key))
        .collect()
}

/// `systemd-run` arguments that start `executable` with `arguments` in a resource scope, removing
/// `removed` variables first. Memory ceilings are left out when the total is unknown.
pub fn scope_arguments(
    ceilings: Option<MemoryCeilings>,
    env: &Path,
    removed: &[&str],
    executable: &Path,
    arguments: &[String],
) -> Vec<OsString> {
    let mut scoped = ["--user", "--scope", "--quiet", "--collect"]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let memory = ceilings.into_iter().flat_map(|ceilings| {
        [
            format!("MemoryHigh={}", ceilings.high_bytes),
            format!("MemoryMax={}", ceilings.max_bytes),
        ]
    });
    for property in SCOPE_PROPERTIES
        .map(str::to_string)
        .into_iter()
        .chain(memory)
    {
        scoped.push(OsString::from("-p"));
        scoped.push(OsString::from(property));
    }
    scoped.push(OsString::from("--"));
    scoped.push(env.as_os_str().to_owned());
    for key in removed {
        scoped.push(OsString::from("-u"));
        scoped.push(OsString::from(key));
    }
    scoped.push(OsString::from("--"));
    scoped.push(executable.as_os_str().to_owned());
    scoped.extend(arguments.iter().map(OsString::from));
    scoped
}

/// Whether `env` would read `executable` as a variable assignment instead of a program.
pub fn launchable_through_env(executable: &Path) -> bool {
    !executable.as_os_str().to_string_lossy().contains('=')
}

/// The configured environment plus the variables the wrapper needs from the app environment.
pub(crate) fn wrapper_environment(
    configured: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut environment = configured.clone();
    for key in SCOPE_ENVIRONMENT {
        if environment.contains_key(key) {
            continue;
        }
        if let Ok(value) = std::env::var(key) {
            environment.insert(key.to_string(), value);
        }
    }
    environment
}

/// Probes the first existing `systemd-run` and `env` candidates through the same chain and
/// properties a provider launch uses, with `env -i` standing in for the provider. `None` means
/// providers start directly: inside Flatpak, without a candidate, or when the probe fails or
/// times out.
pub(crate) async fn probe_scope_launcher(
    runners: &[&Path],
    envs: &[&Path],
    flatpak_marker: &Path,
) -> Option<ScopeLauncher> {
    if flatpak_marker.exists() {
        return None;
    }
    let runner = runners.iter().find(|candidate| candidate.is_file())?;
    let env = envs.iter().find(|candidate| candidate.is_file())?;
    let none = BTreeMap::new();
    let environment = wrapper_environment(&none);
    let arguments = scope_arguments(
        device_memory_ceilings(),
        env,
        &wrapper_only_variables(&none),
        env,
        &["-i".to_string()],
    );
    let arguments = arguments
        .iter()
        .map(|argument| argument.to_str())
        .collect::<Option<Vec<_>>>()?;
    let output = run_bounded_command(BoundedCommand {
        executable: runner,
        arguments: &arguments,
        working_directory: Path::new("/"),
        environment: &environment,
        output_limit_bytes: PROBE_OUTPUT_LIMIT_BYTES,
        timeout: PROBE_TIMEOUT,
    })
    .await
    .ok()?;
    output.successful.then(|| ScopeLauncher {
        runner: runner.to_path_buf(),
        env: env.to_path_buf(),
    })
}

static SCOPE_LAUNCHER: OnceLock<Option<ScopeLauncher>> = OnceLock::new();
#[cfg(target_os = "linux")]
static SCOPE_DISABLED: AtomicBool = AtomicBool::new(false);

/// Probes resource scope support once for this app run and caches the answer. Desktop startup
/// runs it in the background; launches before it finishes, and launches in processes that never
/// run it, start directly, so the probe never delays a provider.
pub async fn probe_resource_scope() {
    if SCOPE_LAUNCHER.get().is_some() {
        return;
    }
    let launcher = if cfg!(target_os = "linux") {
        let runners = SYSTEMD_RUN_CANDIDATES.map(Path::new);
        let envs = ENV_CANDIDATES.map(Path::new);
        probe_scope_launcher(&runners, &envs, Path::new(FLATPAK_MARKER)).await
    } else {
        None
    };
    let _ = SCOPE_LAUNCHER.set(launcher);
}

/// The cached launcher while scopes work on this device.
#[cfg(target_os = "linux")]
pub(crate) fn scope_launcher() -> Option<&'static ScopeLauncher> {
    if SCOPE_DISABLED.load(Ordering::Acquire) {
        return None;
    }
    SCOPE_LAUNCHER.get().and_then(Option::as_ref)
}

/// Stops scoping for the rest of this app run after a scoped launch failed to start.
#[cfg(target_os = "linux")]
pub(crate) fn disable_resource_scope() {
    SCOPE_DISABLED.store(true, Ordering::Release);
}

/// Memory ceilings for this device, or `None` when `/proc/meminfo` cannot be read.
pub(crate) fn device_memory_ceilings() -> Option<MemoryCeilings> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .as_deref()
        .and_then(parse_mem_total)
        .map(memory_ceilings)
}

/// How long a closed session waits for the provider's exit before deciding it was not a memory
/// ceiling kill.
const MEMORY_LIMIT_VERDICT_DEADLINE: Duration = Duration::from_secs(1);

/// Whether the memory ceiling stopped a provider whose stream closed unexpectedly; `false` for an
/// unscoped provider or when its exit is not observed within a short deadline.
pub(crate) async fn reached_memory_limit(watch: Option<&MemoryLimitWatch>) -> bool {
    match watch {
        Some(watch) => watch.reached(MEMORY_LIMIT_VERDICT_DEADLINE).await,
        None => false,
    }
}

/// Warning a driver emits before the session failure when the memory ceiling stopped the provider.
pub(crate) fn memory_limit_warning() -> NotificationEvent {
    NotificationEvent {
        code: "provider_memory_limit".to_string(),
        title: "Provider reached its memory limit".to_string(),
        detail: Some(
            "The system stopped the provider because it used more memory than its resource scope allows"
                .to_string(),
        ),
    }
}

#[cfg(target_os = "linux")]
pub use linux::MemoryLimitWatch;
#[cfg(target_os = "linux")]
pub(crate) use linux::{confirm_scope_entry, watch_memory_limit};

#[cfg(not(target_os = "linux"))]
pub use other::MemoryLimitWatch;

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};
    use tokio::process::Child;
    use tokio::sync::watch;

    const CGROUP_ROOT: &str = "/sys/fs/cgroup";
    const SCOPE_START_DEADLINE: Duration = Duration::from_millis(500);
    const SCOPE_START_POLL: Duration = Duration::from_millis(5);
    const SCOPE_ENTRY_POLL: Duration = Duration::from_millis(20);
    const SCOPE_ENTRY_ATTEMPTS: u32 = 100;
    /// How often a running scope's counters are sampled. A parent counter rise is attributed to
    /// the scope only within one sample of the provider's exit, which bounds misattribution.
    const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

    /// Waits a bounded time for `systemd-run` to create the scope and replace itself. `false`
    /// when the wrapper exited or is still running itself at the deadline.
    pub fn confirm_scope_entry(child: &mut Child, runner: &Path) -> bool {
        let Some(pid) = child.id() else {
            return false;
        };
        let runner = std::fs::canonicalize(runner).unwrap_or_else(|_| runner.to_path_buf());
        let deadline = Instant::now() + SCOPE_START_DEADLINE;
        loop {
            if !matches!(child.try_wait(), Ok(None)) {
                return false;
            }
            match std::fs::read_link(format!("/proc/{pid}/exe")) {
                Ok(executable) if executable != runner => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(SCOPE_START_POLL);
        }
    }

    /// Tells whether a scoped provider was stopped by its memory ceiling. The verdict is known
    /// once the process has exited.
    #[derive(Clone, Debug)]
    pub struct MemoryLimitWatch {
        verdict: watch::Receiver<Option<bool>>,
    }

    impl MemoryLimitWatch {
        /// Whether the kernel killed a process of this scope for memory and the provider exited by
        /// a signal, waiting up to `deadline` for the exit to be observed.
        pub async fn reached(&self, deadline: Duration) -> bool {
            let mut verdict = self.verdict.clone();
            tokio::time::timeout(deadline, verdict.wait_for(Option::is_some))
                .await
                .ok()
                .and_then(Result::ok)
                .and_then(|value| *value)
                .unwrap_or(false)
        }
    }

    /// Unified hierarchy path of a process, from `/proc/<pid>/cgroup` content.
    pub(super) fn parse_cgroup_path(content: &str) -> Option<&str> {
        content
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .filter(|path| path.starts_with('/'))
    }

    /// `oom_kill` counter from `memory.events` content.
    pub(super) fn parse_oom_kills(content: &str) -> Option<u64> {
        content.lines().find_map(|line| {
            line.strip_prefix("oom_kill ")
                .and_then(|value| value.trim().parse().ok())
        })
    }

    fn cgroup_of(pid: &str) -> Option<String> {
        let content = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
        parse_cgroup_path(&content).map(str::to_string)
    }

    fn oom_kills(cgroup: &Path) -> Option<u64> {
        std::fs::read_to_string(cgroup.join("memory.events"))
            .ok()
            .as_deref()
            .and_then(parse_oom_kills)
    }

    /// Memory kill counters of one scope. The scope's own counter is exact, but systemd removes
    /// the scope as soon as its last process exits, before the exit can be read, so the parent's
    /// hierarchical counter covers the final sample interval.
    struct Counters {
        scope: PathBuf,
        scope_start: u64,
        scope_rose: bool,
        parent: Option<(PathBuf, u64)>,
    }

    impl Counters {
        fn sample(&mut self) {
            let Some(kills) = oom_kills(&self.scope) else {
                return;
            };
            self.scope_rose |= kills > self.scope_start;
            if let Some((parent, last)) = self.parent.as_mut()
                && let Some(kills) = oom_kills(parent)
            {
                *last = kills;
            }
        }

        fn reached(&mut self) -> bool {
            self.sample();
            self.scope_rose
                || self
                    .parent
                    .as_ref()
                    .and_then(|(parent, last)| oom_kills(parent).map(|kills| kills > *last))
                    .unwrap_or(false)
        }
    }

    /// Reads the starting counters once the wrapper has moved the process into its scope.
    async fn enter_scope(pid: &str) -> Option<Counters> {
        let app = cgroup_of("self")?;
        for _ in 0..SCOPE_ENTRY_ATTEMPTS {
            match cgroup_of(pid) {
                Some(path) if path != app => {
                    let scope = Path::new(CGROUP_ROOT).join(path.trim_start_matches('/'));
                    let scope_start = oom_kills(&scope)?;
                    let parent = scope
                        .parent()
                        .filter(|parent| *parent != Path::new(CGROUP_ROOT))
                        .and_then(|parent| {
                            oom_kills(parent).map(|kills| (parent.to_path_buf(), kills))
                        });
                    return Some(Counters {
                        scope,
                        scope_start,
                        scope_rose: false,
                        parent,
                    });
                }
                Some(_) => tokio::time::sleep(SCOPE_ENTRY_POLL).await,
                None => return None,
            }
        }
        None
    }

    /// Watches a scoped provider for a memory ceiling kill. The exit is observed without reaping,
    /// so the process ID stays reserved for the handle that owns it. `None` when the exit cannot
    /// be watched.
    pub fn watch_memory_limit(child: &Child) -> Option<MemoryLimitWatch> {
        let pid = child.id()?.to_string();
        let mut exit = crate::process::watch_exit_unreaped(child).ok()?;
        let (sender, verdict) = watch::channel(None);
        tokio::spawn(async move {
            let Some(mut counters) = enter_scope(&pid).await else {
                let _ = sender.send(Some(false));
                return;
            };
            let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let killed_by_signal = loop {
                tokio::select! {
                    result = &mut exit => break matches!(result, Ok(Ok(None))),
                    _ = ticker.tick() => counters.sample(),
                }
            };
            let _ = sender.send(Some(killed_by_signal && counters.reached()));
        });
        Some(MemoryLimitWatch { verdict })
    }
}

#[cfg(not(target_os = "linux"))]
mod other {
    use std::time::Duration;

    /// Resource scopes exist only on Linux, so no limit is ever reported here.
    #[derive(Clone, Debug)]
    pub struct MemoryLimitWatch;

    impl MemoryLimitWatch {
        /// Always `false` outside Linux.
        pub async fn reached(&self, _deadline: Duration) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests;
