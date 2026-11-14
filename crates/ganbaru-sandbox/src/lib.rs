//! Kernel-enforced confinement for the processes that Chat teammates run.
//!
//! A [`SandboxPolicy`] names the paths a process may read, execute, and write, and the network
//! access it keeps. The `ganbaru-sandbox` helper binary applies a policy to itself and then
//! executes the confined program, so the restriction holds for that program and for every
//! process it starts. Ganbaru never applies a policy to its own process.
//!
//! Enforcement uses Landlock for the filesystem and TCP and seccomp for the other socket
//! families, and exists only on Linux. It fails closed: when the kernel cannot enforce every part
//! of a policy, the helper refuses to start the program, and [`probe`] reports the sandbox as
//! unavailable so callers keep the protections they use without it.

mod invocation;
#[cfg(target_os = "linux")]
mod linux;
mod locate;
mod shell;

use std::fmt;
use std::path::PathBuf;

pub use invocation::{SandboxInvocation, decode_invocation, encode_invocation};
pub use locate::{SandboxHelper, probe};
#[cfg(unix)]
pub use shell::shell_invocation;
pub use shell::{POLICY_FILE_VARIABLE, decode_policy_file, encode_policy_file};

/// Network access a confined process keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkAccess {
    /// No connection of any family: TCP is refused, no Internet or other non-local socket can
    /// be created, and local sockets can only be the already connected pairs a process creates
    /// for itself.
    Denied,
    /// Outgoing TCP connections to the listed ports only, plus the UDP a resolver needs. Local
    /// sockets are refused, so the process cannot reach other local services.
    TcpConnect(Vec<u16>),
}

/// The filesystem and network access of one confined process.
///
/// Every path must be absolute and must exist when the policy is applied. Each list grants its
/// access to the named file or to the whole tree under a named directory. Write access includes
/// read access, and execute access includes read access; nothing outside the lists is reachable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxPolicy {
    /// Paths the process may read and list.
    pub read: Vec<PathBuf>,
    /// Paths the process may read and execute.
    pub execute: Vec<PathBuf>,
    /// Paths the process may read, create, change, rename, and delete in.
    pub write: Vec<PathBuf>,
    /// Network access the process keeps.
    pub network: NetworkAccess,
}

impl SandboxPolicy {
    /// A policy that reaches nothing on disk and has no network access.
    pub fn deny_all() -> Self {
        Self {
            read: Vec::new(),
            execute: Vec::new(),
            write: Vec::new(),
            network: NetworkAccess::Denied,
        }
    }

    /// Checks that every path is absolute.
    pub fn validate(&self) -> Result<(), SandboxError> {
        for path in self.read.iter().chain(&self.execute).chain(&self.write) {
            if !path.is_absolute() {
                return Err(SandboxError::InvalidPolicy(format!(
                    "path {} is not absolute",
                    path.display()
                )));
            }
        }
        if let NetworkAccess::TcpConnect(ports) = &self.network {
            if ports.is_empty() || ports.contains(&0) {
                return Err(SandboxError::InvalidPolicy(
                    "TCP access needs at least one nonzero port".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Why a policy could not be described, applied, or used.
#[derive(Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// The policy or its encoded form is malformed.
    InvalidPolicy(String),
    /// The platform or the running kernel cannot enforce the policy.
    Unsupported(String),
    /// A policy path could not be opened.
    Path { path: PathBuf, reason: String },
    /// The kernel rejected the restriction.
    Enforcement(String),
    /// No usable helper binary was found, or it failed its own check.
    HelperUnavailable(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy(reason) => write!(formatter, "invalid sandbox policy: {reason}"),
            Self::Unsupported(reason) => write!(formatter, "sandbox unsupported: {reason}"),
            Self::Path { path, reason } => {
                write!(formatter, "sandbox path {}: {reason}", path.display())
            }
            Self::Enforcement(reason) => write!(formatter, "sandbox not enforced: {reason}"),
            Self::HelperUnavailable(reason) => {
                write!(formatter, "sandbox helper unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for SandboxError {}

/// Applies `policy` to the calling process and every process it starts later.
///
/// The restriction cannot be lifted. Only the helper binary calls this, right before it
/// executes the confined program; it returns an error, and restricts nothing it can avoid, when
/// any part of the policy cannot be enforced.
pub fn enforce(policy: &SandboxPolicy) -> Result<(), SandboxError> {
    policy.validate()?;
    #[cfg(target_os = "linux")]
    {
        linux::enforce(policy)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(SandboxError::Unsupported(
            "kernel confinement is available only on Linux".to_string(),
        ))
    }
}
