//! Kernel confinement shared by teammate providers.
//!
//! Organizational teammates and the commands they run start under the `ganbaru-sandbox` helper
//! when the device has one that the kernel enforces. This module locates that helper once per
//! app run and holds the pieces every provider builds its policies from: the confined terminal
//! policy, the toolchain homes a confined command uses in place, the Git metadata guard, and
//! the refusals a provider reports when it cannot be confined.

use crate::models::*;
use ganbaru_sandbox::{SandboxHelper, SandboxPolicy};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod git_guard;
mod terminal;
#[cfg(test)]
mod tests;
mod toolchains;

pub(crate) use git_guard::{GitMetadataGuard, changed_error as git_metadata_changed_error};
pub(crate) use terminal::{TerminalSandbox, confined_env_arguments};

/// Files every confined process may write without reaching user data.
pub(crate) const DEVICE_WRITE: &[&str] = &["/dev/null"];
/// Port of a provider API, reached over TLS.
pub(crate) const PROVIDER_API_PORT: u16 = 443;
/// Prefix of the private temporary directory of one confined process.
pub(crate) const CONFINED_TEMPORARY_PREFIX: &str = "ganbaru-confined-";

/// The sandbox helper of this installation, when it is present and the kernel enforces a
/// policy; checked once per app run.
///
/// Off Linux there is no helper, so this is always `None`.
pub(crate) fn sandbox_helper() -> Option<&'static SandboxHelper> {
    static HELPER: OnceLock<Option<SandboxHelper>> = OnceLock::new();
    HELPER.get_or_init(locate_enforcing_helper).as_ref()
}

#[cfg(target_os = "linux")]
fn locate_enforcing_helper() -> Option<SandboxHelper> {
    let helper = SandboxHelper::locate().ok()?;
    ganbaru_sandbox::probe(&helper).ok()?;
    Some(helper)
}

#[cfg(not(target_os = "linux"))]
fn locate_enforcing_helper() -> Option<SandboxHelper> {
    None
}

/// The loopback port of the internal MCP endpoint; `provider` names the teammate in a refusal.
pub(crate) fn internal_mcp_port(url: &str, provider: &str) -> ChatResult<u16> {
    let parsed = reqwest::Url::parse(url).map_err(|_| sandbox_policy_error(provider))?;
    let loopback = parsed.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    parsed
        .port_or_known_default()
        .filter(|port| loopback && *port != 0)
        .ok_or_else(|| sandbox_policy_error(provider))
}

/// The file `/etc/resolv.conf` points to, which name resolution reads.
pub(crate) fn resolver_configuration() -> Option<PathBuf> {
    std::fs::canonicalize("/etc/resolv.conf").ok()
}

/// The entries of `paths` that `exists` accepts.
pub(crate) fn existing_paths(paths: &[&str], exists: &impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    paths
        .iter()
        .map(PathBuf::from)
        .filter(|path| exists(path))
        .collect()
}

/// Helper arguments as provider process arguments; a path that is not valid Unicode is refused.
pub(crate) fn helper_arguments(
    helper: &SandboxHelper,
    policy: &SandboxPolicy,
    program: &Path,
    arguments: &[String],
    provider: &str,
) -> ChatResult<Vec<String>> {
    helper
        .arguments(policy, program, arguments)
        .map_err(|_| sandbox_unavailable_error(provider))?
        .into_iter()
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| sandbox_policy_error(provider))
        })
        .collect()
}

/// The refusal of a folder launch when the device has no enforcing helper.
pub(crate) fn sandbox_unavailable_error(provider: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        format!(
            "{provider} needs the Ganbaru sandbox to work in a folder, and it is unavailable on \
             this device"
        ),
        false,
    )
}

/// The refusal of a launch whose policy could not be built.
pub(crate) fn sandbox_policy_error(provider: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        format!("Ganbaru could not prepare the sandbox for this {provider} session"),
        false,
    )
}

/// A recoverable refusal of one action outside the teammate's folder scope.
pub(crate) fn scope_refusal(message: &str) -> ChatError {
    ChatError::new(ChatErrorCode::Permission, message, true)
}
