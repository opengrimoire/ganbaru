//! Landlock and seccomp enforcement of a [`SandboxPolicy`] on Linux.
//!
//! Landlock ABI 4 (Linux 6.7) is required: it is the first one that restricts both every
//! filesystem right Ganbaru relies on and TCP. Newer kernels also scope abstract Unix sockets
//! and signals to the sandbox. Landlock cannot restrict UDP or local sockets, so a seccomp
//! filter refuses the socket families each network mode leaves out, the calls that connect or
//! listen on local sockets, and `io_uring`, which would otherwise start socket operations the
//! filter never sees.

use std::collections::BTreeMap;
use std::path::Path;

use landlock::{
    ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, NetPort, PathBeneath,
    PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, RulesetStatus, Scope,
};
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule, TargetArch,
};

use crate::{NetworkAccess, SandboxError, SandboxPolicy};

/// The Landlock ABI whose rights are all required.
const REQUIRED_ABI: ABI = ABI::V4;
/// The Landlock ABI whose scopes are applied when the kernel has them.
const SCOPE_ABI: ABI = ABI::V6;
/// Bit that marks an x32 system call on x86_64, which a filter must match on its own.
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: i64 = 0x4000_0000;
/// Mask of the socket type bits, without `SOCK_NONBLOCK` and `SOCK_CLOEXEC`.
const SOCKET_TYPE_MASK: u64 = 0xf;
/// The only local socket type a process without network access may create unconnected.
const STREAM_TYPES: &[u64] = &[libc::SOCK_STREAM as u64];
/// The local socket types a process may create as connected pairs. Both are connection
/// oriented, so a pair cannot be redirected: `connect` on it fails and the kernel ignores the
/// address a send names. Process spawning in common runtimes reports exec errors through a
/// sequenced packet pair.
const PAIR_TYPES: &[u64] = &[libc::SOCK_STREAM as u64, libc::SOCK_SEQPACKET as u64];

pub(crate) fn enforce(policy: &SandboxPolicy) -> Result<(), SandboxError> {
    // Build the seccomp program first so that a filter error leaves the process unrestricted
    // and the helper exits before it runs anything.
    let filter = socket_filter(&policy.network)?;
    restrict_filesystem_and_tcp(policy)?;
    seccompiler::apply_filter(&filter)
        .map_err(|error| SandboxError::Enforcement(format!("seccomp filter: {error}")))
}

fn restrict_filesystem_and_tcp(policy: &SandboxPolicy) -> Result<(), SandboxError> {
    let ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(REQUIRED_ABI))
        .and_then(|ruleset| ruleset.handle_access(AccessNet::from_all(REQUIRED_ABI)))
        .map_err(|error| unsupported(format!("Landlock ABI 4: {error}")))?
        .set_compatibility(CompatLevel::BestEffort)
        .scope(Scope::from_all(SCOPE_ABI))
        .map_err(|error| unsupported(format!("Landlock scopes: {error}")))?;
    let mut created = ruleset
        .create()
        .map_err(|error| unsupported(format!("Landlock ruleset: {error}")))?
        .set_compatibility(CompatLevel::HardRequirement);
    let read = AccessFs::ReadFile | AccessFs::ReadDir;
    let write =
        read | (AccessFs::from_write(REQUIRED_ABI) & !(AccessFs::MakeChar | AccessFs::MakeBlock));
    for (paths, access) in [
        (&policy.read, read),
        (&policy.execute, read | AccessFs::Execute),
        (&policy.write, write),
    ] {
        for path in paths {
            created = created
                .add_rule(path_rule(path, access)?)
                .map_err(|error| SandboxError::Enforcement(format!("Landlock rule: {error}")))?;
        }
    }
    if let NetworkAccess::TcpConnect(ports) = &policy.network {
        for port in ports {
            created = created
                .add_rule(NetPort::new(*port, AccessNet::ConnectTcp))
                .map_err(|error| SandboxError::Enforcement(format!("Landlock port: {error}")))?;
        }
    }
    let status = created
        .restrict_self()
        .map_err(|error| SandboxError::Enforcement(format!("Landlock: {error}")))?;
    if status.ruleset == RulesetStatus::NotEnforced || !status.no_new_privs {
        return Err(SandboxError::Enforcement(
            "Landlock did not restrict the process".to_string(),
        ));
    }
    Ok(())
}

/// A rule that grants `access` on `path`, keeping only the rights that apply to files when the
/// path is not a directory.
fn path_rule(path: &Path, access: BitFlags<AccessFs>) -> Result<PathBeneath<PathFd>, SandboxError> {
    let metadata = std::fs::metadata(path).map_err(|error| SandboxError::Path {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    let access = if metadata.is_dir() {
        access
    } else {
        access & AccessFs::from_file(REQUIRED_ABI)
    };
    let descriptor = PathFd::new(path).map_err(|error| SandboxError::Path {
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    Ok(PathBeneath::new(descriptor, access))
}

/// The seccomp program that refuses the socket operations `network` leaves out.
fn socket_filter(network: &NetworkAccess) -> Result<BpfProgram, SandboxError> {
    let arch = target_arch()?;
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    for syscall in [
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
    ] {
        rules.insert(syscall, Vec::new());
    }
    // Pairs from socketpair are the local sockets every mode keeps, and only connection
    // oriented pairs: a datagram pair can still send to, or connect to, a named socket outside
    // the sandbox.
    rules.insert(libc::SYS_socketpair, local_types_only(PAIR_TYPES)?);
    match network {
        NetworkAccess::Denied => {
            // Without connect, bind, and accept, a local stream socket can reach nothing, so
            // the only usable ones are the connected pairs from socketpair.
            for syscall in [
                libc::SYS_connect,
                libc::SYS_bind,
                libc::SYS_listen,
                libc::SYS_accept,
                libc::SYS_accept4,
            ] {
                rules.insert(syscall, Vec::new());
            }
            rules.insert(libc::SYS_socket, local_types_only(STREAM_TYPES)?);
        }
        NetworkAccess::TcpConnect(_) => {
            // Landlock limits TCP to the allowed ports; only Internet sockets may be created,
            // so the process cannot reach local services through Unix or netlink sockets.
            rules.insert(
                libc::SYS_socket,
                vec![rule(vec![
                    condition(0, SeccompCmpOp::Ne, libc::AF_INET as u64)?,
                    condition(0, SeccompCmpOp::Ne, libc::AF_INET6 as u64)?,
                ])?],
            );
        }
    }
    add_x32_variants(&mut rules);
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )
    .map_err(|error| SandboxError::Enforcement(format!("seccomp filter: {error}")))?;
    filter
        .try_into()
        .map_err(|error| SandboxError::Enforcement(format!("seccomp program: {error}")))
}

/// Rules that match every `socket` or `socketpair` call except one for a local socket of one of
/// the `allowed` types.
///
/// Every other value of the type field is matched, so no type the kernel adds later slips by.
fn local_types_only(allowed: &[u64]) -> Result<Vec<SeccompRule>, SandboxError> {
    let mut rules = vec![rule(vec![condition(
        0,
        SeccompCmpOp::Ne,
        libc::AF_UNIX as u64,
    )?])?];
    for socket_type in (0..=SOCKET_TYPE_MASK).filter(|value| !allowed.contains(value)) {
        rules.push(rule(vec![condition(
            1,
            SeccompCmpOp::MaskedEq(SOCKET_TYPE_MASK),
            socket_type,
        )?])?);
    }
    Ok(rules)
}

/// Matches x32 calls too, because an x86_64 filter sees them under their own numbers.
#[cfg(target_arch = "x86_64")]
fn add_x32_variants(rules: &mut BTreeMap<i64, Vec<SeccompRule>>) {
    let x32 = rules
        .iter()
        .map(|(syscall, syscall_rules)| (syscall | X32_SYSCALL_BIT, syscall_rules.clone()))
        .collect::<Vec<_>>();
    rules.extend(x32);
}

#[cfg(not(target_arch = "x86_64"))]
fn add_x32_variants(_rules: &mut BTreeMap<i64, Vec<SeccompRule>>) {}

fn target_arch() -> Result<TargetArch, SandboxError> {
    TargetArch::try_from(std::env::consts::ARCH).map_err(|_| {
        unsupported(format!(
            "no seccomp filter for the {} architecture",
            std::env::consts::ARCH
        ))
    })
}

fn condition(
    index: u8,
    operator: SeccompCmpOp,
    value: u64,
) -> Result<SeccompCondition, SandboxError> {
    SeccompCondition::new(index, SeccompCmpArgLen::Dword, operator, value)
        .map_err(|error| SandboxError::Enforcement(format!("seccomp condition: {error}")))
}

fn rule(conditions: Vec<SeccompCondition>) -> Result<SeccompRule, SandboxError> {
    SeccompRule::new(conditions)
        .map_err(|error| SandboxError::Enforcement(format!("seccomp rule: {error}")))
}

fn unsupported(reason: String) -> SandboxError {
    SandboxError::Unsupported(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_network_modes_compile_to_seccomp_programs() {
        assert!(!socket_filter(&NetworkAccess::Denied).unwrap().is_empty());
        assert!(
            !socket_filter(&NetworkAccess::TcpConnect(vec![443]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn file_rules_keep_only_file_rights() {
        let rule = path_rule(
            Path::new("/proc/self/exe"),
            AccessFs::from_all(REQUIRED_ABI),
        );
        assert!(rule.is_ok());
        let missing = path_rule(
            Path::new("/nonexistent/ganbaru-sandbox"),
            AccessFs::from_all(REQUIRED_ABI),
        );
        assert!(matches!(missing, Err(SandboxError::Path { .. })));
    }
}
