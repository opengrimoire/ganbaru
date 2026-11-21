//! Command line form of a confined program: the policy, then the program and its arguments.
//!
//! The form is `[--read PATH]... [--execute PATH]... [--write PATH]... --network NETWORK --
//! PROGRAM [ARGUMENT]...`, where `NETWORK` is `denied` or `tcp:` followed by comma separated
//! ports. Paths travel as raw operating system strings, so any file name the policy names
//! reaches the helper unchanged.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::{NetworkAccess, SandboxError, SandboxPolicy};

const READ: &str = "--read";
const EXECUTE: &str = "--execute";
const WRITE: &str = "--write";
const NETWORK: &str = "--network";
const SEPARATOR: &str = "--";
const NETWORK_DENIED: &str = "denied";
const NETWORK_TCP_PREFIX: &str = "tcp:";

/// A policy and the program the helper runs under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxInvocation {
    /// Access the program keeps.
    pub policy: SandboxPolicy,
    /// Program to execute, looked up on `PATH` when it has no directory part.
    pub program: OsString,
    /// Arguments after the program.
    pub arguments: Vec<OsString>,
}

/// Encodes the helper arguments that run `program` with `arguments` under `policy`.
pub fn encode_invocation(
    policy: &SandboxPolicy,
    program: impl Into<OsString>,
    arguments: impl IntoIterator<Item = impl Into<OsString>>,
) -> Result<Vec<OsString>, SandboxError> {
    policy.validate()?;
    let mut encoded = Vec::new();
    for (flag, paths) in [
        (READ, &policy.read),
        (EXECUTE, &policy.execute),
        (WRITE, &policy.write),
    ] {
        for path in paths {
            encoded.push(OsString::from(flag));
            encoded.push(path.clone().into_os_string());
        }
    }
    encoded.push(OsString::from(NETWORK));
    encoded.push(OsString::from(match &policy.network {
        NetworkAccess::Denied => NETWORK_DENIED.to_string(),
        NetworkAccess::TcpConnect(ports) => format!(
            "{NETWORK_TCP_PREFIX}{}",
            ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
    }));
    encoded.push(OsString::from(SEPARATOR));
    encoded.push(program.into());
    encoded.extend(arguments.into_iter().map(Into::into));
    Ok(encoded)
}

/// Decodes helper arguments, without the helper's own name, into an invocation.
///
/// The network flag is required so that a missing or misspelled flag never leaves a program
/// with access it was not meant to keep.
pub fn decode_invocation(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<SandboxInvocation, SandboxError> {
    let mut arguments = arguments.into_iter();
    let mut policy = SandboxPolicy::deny_all();
    let mut network = None;
    loop {
        let Some(flag) = arguments.next() else {
            return Err(invalid("the program is missing"));
        };
        if flag == SEPARATOR {
            break;
        }
        let value = arguments
            .next()
            .ok_or_else(|| invalid(format!("{} needs a value", flag.to_string_lossy())))?;
        match flag.to_str() {
            Some(READ) => policy.read.push(PathBuf::from(value)),
            Some(EXECUTE) => policy.execute.push(PathBuf::from(value)),
            Some(WRITE) => policy.write.push(PathBuf::from(value)),
            Some(NETWORK) if network.is_none() => network = Some(decode_network(&value)?),
            Some(NETWORK) => return Err(invalid("the network access is given twice")),
            _ => {
                return Err(invalid(format!(
                    "unknown argument {}",
                    flag.to_string_lossy()
                )));
            }
        }
    }
    policy.network = network.ok_or_else(|| invalid("the network access is missing"))?;
    policy.validate()?;
    let program = arguments
        .next()
        .ok_or_else(|| invalid("the program is missing"))?;
    if program.is_empty() {
        return Err(invalid("the program is empty"));
    }
    Ok(SandboxInvocation {
        policy,
        program,
        arguments: arguments.collect(),
    })
}

fn decode_network(value: &OsStr) -> Result<NetworkAccess, SandboxError> {
    let value = value
        .to_str()
        .ok_or_else(|| invalid("the network access is not text"))?;
    if value == NETWORK_DENIED {
        return Ok(NetworkAccess::Denied);
    }
    let ports = value
        .strip_prefix(NETWORK_TCP_PREFIX)
        .ok_or_else(|| invalid(format!("unknown network access {value}")))?;
    let ports = ports
        .split(',')
        .map(|port| {
            port.parse::<u16>()
                .map_err(|_| invalid(format!("invalid TCP port {port}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NetworkAccess::TcpConnect(ports))
}

fn invalid(reason: impl Into<String>) -> SandboxError {
    SandboxError::InvalidPolicy(reason.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SandboxPolicy {
        SandboxPolicy {
            read: vec![PathBuf::from("/usr"), PathBuf::from("/etc")],
            execute: vec![PathBuf::from("/usr/bin")],
            write: vec![PathBuf::from("/work space/with --read in it")],
            network: NetworkAccess::TcpConnect(vec![443, 80]),
        }
    }

    #[test]
    fn invocations_round_trip_with_paths_and_arguments_that_look_like_flags() {
        let encoded = encode_invocation(&policy(), "sh", ["-c", "--", "--write /"]).unwrap();
        let decoded = decode_invocation(encoded).unwrap();
        assert_eq!(decoded.policy, policy());
        assert_eq!(decoded.program, OsString::from("sh"));
        assert_eq!(
            decoded.arguments,
            ["-c", "--", "--write /"].map(OsString::from)
        );
    }

    #[test]
    fn denied_network_round_trips() {
        let mut denied = policy();
        denied.network = NetworkAccess::Denied;
        let decoded =
            decode_invocation(encode_invocation(&denied, "true", Vec::<OsString>::new()).unwrap())
                .unwrap();
        assert_eq!(decoded.policy.network, NetworkAccess::Denied);
        assert!(decoded.arguments.is_empty());
    }

    #[test]
    fn decoding_refuses_a_missing_or_repeated_network_access() {
        let missing = ["--read", "/usr", "--", "true"].map(OsString::from);
        assert!(decode_invocation(missing).is_err());
        let repeated =
            ["--network", "denied", "--network", "tcp:443", "--", "true"].map(OsString::from);
        assert!(decode_invocation(repeated).is_err());
    }

    #[test]
    fn decoding_refuses_relative_paths_unknown_flags_and_bad_ports() {
        for arguments in [
            vec!["--read", "usr", "--network", "denied", "--", "true"],
            vec!["--readonly", "/usr", "--network", "denied", "--", "true"],
            vec!["--network", "tcp:", "--", "true"],
            vec!["--network", "tcp:0", "--", "true"],
            vec!["--network", "tcp:70000", "--", "true"],
            vec!["--network", "udp:53", "--", "true"],
            vec!["--network", "denied", "--"],
            vec!["--network", "denied", "--", ""],
            vec!["--network", "denied"],
            vec!["--read"],
        ] {
            let arguments = arguments.into_iter().map(OsString::from);
            assert!(decode_invocation(arguments).is_err());
        }
    }

    #[test]
    fn encoding_refuses_relative_paths() {
        let mut relative = policy();
        relative.write.push(PathBuf::from("relative"));
        assert!(encode_invocation(&relative, "true", Vec::<OsString>::new()).is_err());
    }
}
