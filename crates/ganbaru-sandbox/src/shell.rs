//! Shell mode: the helper stands in for the shell a provider runs commands with.
//!
//! Some providers run each command as `SHELL -c COMMAND` or `PREFIX COMMAND` and let the
//! configuration name only the shell program, not its arguments. In shell mode the helper finds
//! its policy in a file named by [`POLICY_FILE_VARIABLE`] and runs the program written there
//! with the command appended as its last argument. The file holds the encoded invocation of
//! [`crate::encode_invocation`], one argument per NUL terminated entry.
//!
//! The file must be a regular file owned by the helper's user, writable by no one else, and
//! outside every path the policy makes writable, so a confined command can never change the
//! policy of the commands that follow it.

use std::ffi::OsString;

use crate::{SandboxError, SandboxInvocation, SandboxPolicy, decode_invocation, encode_invocation};

/// Environment variable that names the policy file and puts the helper in shell mode.
pub const POLICY_FILE_VARIABLE: &str = "GANBARU_SANDBOX_POLICY_FILE";
/// Largest policy file the helper reads.
#[cfg(unix)]
const MAX_POLICY_FILE_BYTES: u64 = 64 * 1024;
/// Shell switch that introduces the command, as in `sh -c COMMAND`.
const COMMAND_SWITCH: &str = "-c";

/// Encodes the policy file content that runs `program` with `arguments`, followed by the
/// command, under `policy`.
pub fn encode_policy_file(
    policy: &SandboxPolicy,
    program: impl Into<OsString>,
    arguments: impl IntoIterator<Item = impl Into<OsString>>,
) -> Result<Vec<u8>, SandboxError> {
    let mut content = Vec::new();
    for argument in encode_invocation(policy, program, arguments)? {
        let bytes = argument.as_encoded_bytes();
        if bytes.contains(&0) {
            return Err(invalid("an argument holds a NUL byte"));
        }
        content.extend_from_slice(bytes);
        content.push(0);
    }
    Ok(content)
}

/// Decodes a policy file and the shell arguments of one command into an invocation.
///
/// The shell arguments are `-c COMMAND` or a single `COMMAND`; any other shape is refused, so a
/// provider can never pass options to the confined program.
pub fn decode_policy_file(
    content: &[u8],
    shell_arguments: impl IntoIterator<Item = OsString>,
) -> Result<SandboxInvocation, SandboxError> {
    let command = shell_command(shell_arguments)?;
    let entries = content
        .strip_suffix(&[0])
        .ok_or_else(|| invalid("the policy file is not NUL terminated"))?;
    let arguments = entries
        .split(|byte| *byte == 0)
        .map(|entry| os_string(entry.to_vec()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut invocation = decode_invocation(arguments)?;
    invocation.arguments.push(command);
    Ok(invocation)
}

/// Reads the policy file at `path` and decodes it with the shell arguments of one command.
///
/// The file is refused when it is a symbolic link, not a regular file, owned by another user,
/// writable by its group or others, too large, or inside a writable path of its own policy.
#[cfg(unix)]
pub fn shell_invocation(
    path: &std::path::Path,
    shell_arguments: impl IntoIterator<Item = OsString>,
) -> Result<SandboxInvocation, SandboxError> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let refuse = |reason: &str| SandboxError::Path {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };
    if !path.is_absolute() {
        return Err(refuse("the policy file path is relative"));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(no_follow_flag())
        .open(path)
        .map_err(|error| refuse(&error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| refuse(&error.to_string()))?;
    // The owner of this process's own `/proc` entry is its effective user.
    let user = std::fs::metadata("/proc/self")
        .map_err(|error| refuse(&error.to_string()))?
        .uid();
    if !metadata.is_file() {
        return Err(refuse("the policy file is not a regular file"));
    }
    if metadata.uid() != user || metadata.mode() & 0o022 != 0 {
        return Err(refuse("the policy file is writable by another user"));
    }
    if metadata.len() > MAX_POLICY_FILE_BYTES {
        return Err(refuse("the policy file is too large"));
    }
    let mut content = Vec::new();
    file.take(MAX_POLICY_FILE_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|error| refuse(&error.to_string()))?;
    let invocation = decode_policy_file(&content, shell_arguments)?;
    let canonical = std::fs::canonicalize(path).map_err(|error| refuse(&error.to_string()))?;
    if invocation.policy.write.iter().any(|writable| {
        canonical.starts_with(writable)
            || std::fs::canonicalize(writable).is_ok_and(|writable| canonical.starts_with(writable))
    }) {
        return Err(refuse(
            "the policy file lies inside a writable sandbox path",
        ));
    }
    Ok(invocation)
}

/// `O_NOFOLLOW`, so the policy file is never reached through a symbolic link.
#[cfg(target_os = "linux")]
fn no_follow_flag() -> i32 {
    libc::O_NOFOLLOW
}

#[cfg(all(unix, not(target_os = "linux")))]
fn no_follow_flag() -> i32 {
    0
}

/// The command of a shell invocation: `-c COMMAND` or a single `COMMAND`.
fn shell_command(
    shell_arguments: impl IntoIterator<Item = OsString>,
) -> Result<OsString, SandboxError> {
    let mut arguments = shell_arguments.into_iter();
    let first = arguments
        .next()
        .ok_or_else(|| invalid("the shell command is missing"))?;
    let command = if first == COMMAND_SWITCH {
        arguments
            .next()
            .ok_or_else(|| invalid("the shell command is missing"))?
    } else {
        first
    };
    if arguments.next().is_some() {
        return Err(invalid("the shell takes only one command"));
    }
    Ok(command)
}

#[cfg(unix)]
fn os_string(bytes: Vec<u8>) -> Result<OsString, SandboxError> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn os_string(bytes: Vec<u8>) -> Result<OsString, SandboxError> {
    String::from_utf8(bytes)
        .map(OsString::from)
        .map_err(|_| invalid("the policy file is not text"))
}

fn invalid(reason: &str) -> SandboxError {
    SandboxError::InvalidPolicy(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NetworkAccess;
    use std::path::{Path, PathBuf};

    fn policy(write: &Path) -> SandboxPolicy {
        SandboxPolicy {
            read: vec![PathBuf::from("/etc")],
            execute: vec![PathBuf::from("/usr")],
            write: vec![write.to_path_buf()],
            network: NetworkAccess::Denied,
        }
    }

    #[test]
    fn policy_files_append_the_command_to_the_shell_program() {
        let content =
            encode_policy_file(&policy(Path::new("/work")), "/usr/bin/bash", ["-c"]).unwrap();
        for arguments in [
            vec!["-c", "echo --write / && ls"],
            vec!["echo --write / && ls"],
        ] {
            let invocation =
                decode_policy_file(&content, arguments.into_iter().map(OsString::from)).unwrap();
            assert_eq!(invocation.policy, policy(Path::new("/work")));
            assert_eq!(invocation.program, OsString::from("/usr/bin/bash"));
            assert_eq!(
                invocation.arguments,
                ["-c", "echo --write / && ls"].map(OsString::from)
            );
        }
    }

    #[test]
    fn policy_files_refuse_other_shell_shapes_and_malformed_content() {
        let content =
            encode_policy_file(&policy(Path::new("/work")), "/usr/bin/bash", ["-c"]).unwrap();
        for arguments in [
            vec![],
            vec!["-c"],
            vec!["-c", "ls", "extra"],
            vec!["ls", "extra"],
            vec!["-l", "-c", "ls"],
        ] {
            assert!(
                decode_policy_file(&content, arguments.into_iter().map(OsString::from)).is_err()
            );
        }
        let command = || [OsString::from("ls")];
        assert!(decode_policy_file(&content[..content.len() - 1], command()).is_err());
        assert!(decode_policy_file(b"--network\0denied\0--\0", command()).is_err());
        assert!(decode_policy_file(b"--\0/usr/bin/bash\0", command()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn policy_files_that_others_or_the_policy_can_change_are_refused() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("ganbaru-shell-{}", std::process::id()));
        let writable = root.join("work");
        std::fs::create_dir_all(&writable).unwrap();
        let root = std::fs::canonicalize(&root).unwrap();
        let writable = root.join("work");
        let content = encode_policy_file(&policy(&writable), "/usr/bin/bash", ["-c"]).unwrap();
        let command = || [OsString::from("-c"), OsString::from("ls")];

        let kept = root.join("policy");
        std::fs::write(&kept, &content).unwrap();
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(shell_invocation(&kept, command()).is_ok());

        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o622)).unwrap();
        assert!(shell_invocation(&kept, command()).is_err());
        std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o600)).unwrap();

        let link = root.join("link");
        std::os::unix::fs::symlink(&kept, &link).unwrap();
        assert!(shell_invocation(&link, command()).is_err());

        let inside = writable.join("policy");
        std::fs::write(&inside, &content).unwrap();
        std::fs::set_permissions(&inside, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(shell_invocation(&inside, command()).is_err());

        assert!(shell_invocation(Path::new("relative"), command()).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
