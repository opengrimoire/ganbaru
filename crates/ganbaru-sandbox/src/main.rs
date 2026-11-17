//! Helper that confines itself under a sandbox policy and then executes the confined program.
//!
//! Ganbaru starts commands and provider processes of Chat agents through this binary so that
//! no restriction is ever applied to the app itself. The binary refuses to run the program when
//! any part of the policy cannot be enforced or a descriptor beyond the standard streams is open.
//! See [`ganbaru_sandbox::decode_invocation`] for the argument form. When
//! [`ganbaru_sandbox::POLICY_FILE_VARIABLE`] is set, the helper runs in shell mode instead and
//! reads the policy from that file; see [`ganbaru_sandbox::shell_invocation`]. `--probe` confines the
//! helper under a policy that denies everything, checks that the kernel refuses what the policy
//! leaves out, and exits.

use std::process::ExitCode;

/// Exit status when the policy is malformed or cannot be enforced, so no program ran.
const REFUSED: u8 = 125;
/// Exit status when the confined program could not be executed.
#[cfg(target_os = "linux")]
const NOT_EXECUTED: u8 = 126;

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    use std::os::unix::process::CommandExt;

    let mut arguments = std::env::args_os().skip(1).peekable();
    if arguments
        .peek()
        .is_some_and(|argument| argument == "--probe")
    {
        return match probe() {
            Ok(()) => ExitCode::SUCCESS,
            Err(reason) => {
                eprintln!("ganbaru-sandbox: {reason}");
                ExitCode::from(REFUSED)
            }
        };
    }
    let shell_policy = std::env::var_os(ganbaru_sandbox::POLICY_FILE_VARIABLE);
    let decoded = match &shell_policy {
        Some(path) => ganbaru_sandbox::shell_invocation(std::path::Path::new(path), arguments),
        None => ganbaru_sandbox::decode_invocation(arguments),
    };
    let invocation = match decoded {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("ganbaru-sandbox: {error}");
            return ExitCode::from(REFUSED);
        }
    };
    if let Err(reason) = refuse_inherited_descriptors() {
        eprintln!("ganbaru-sandbox: {reason}");
        return ExitCode::from(REFUSED);
    }
    if let Err(error) = ganbaru_sandbox::enforce(&invocation.policy) {
        eprintln!("ganbaru-sandbox: {error}");
        return ExitCode::from(REFUSED);
    }
    let error = std::process::Command::new(&invocation.program)
        .args(&invocation.arguments)
        .env_remove(ganbaru_sandbox::POLICY_FILE_VARIABLE)
        .exec();
    eprintln!(
        "ganbaru-sandbox: {} could not be executed: {error}",
        invocation.program.to_string_lossy()
    );
    ExitCode::from(NOT_EXECUTED)
}

/// Refuses to continue when the helper inherited a descriptor beyond the standard streams.
///
/// Landlock checks access when a file is opened, so an open descriptor would keep whatever
/// access it was opened with inside the sandbox. Only standard input, output, and error may
/// cross into the confined program.
#[cfg(target_os = "linux")]
fn refuse_inherited_descriptors() -> Result<(), String> {
    let describe = |error: std::io::Error| format!("open descriptors cannot be listed: {error}");
    // The listing holds a descriptor of its own, which names this process's descriptor folder.
    let own_listing = std::path::PathBuf::from(format!("/proc/{}/fd", std::process::id()));
    let mut inherited = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd").map_err(describe)? {
        let entry = entry.map_err(describe)?;
        let number = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .ok_or_else(|| format!("unexpected descriptor entry {:?}", entry.file_name()))?;
        if number <= 2 {
            continue;
        }
        if std::fs::read_link(entry.path()).is_ok_and(|target| target == own_listing) {
            continue;
        }
        inherited.push(number);
    }
    if inherited.is_empty() {
        Ok(())
    } else {
        inherited.sort_unstable();
        Err(format!(
            "inherited descriptors {inherited:?} would bypass the sandbox"
        ))
    }
}

/// Confines the helper under a policy that denies everything and checks that the kernel
/// refuses a filesystem read, a UDP socket, and a local datagram socket.
#[cfg(target_os = "linux")]
fn probe() -> Result<(), String> {
    ganbaru_sandbox::enforce(&ganbaru_sandbox::SandboxPolicy::deny_all())
        .map_err(|error| error.to_string())?;
    if std::fs::read_dir("/").is_ok() {
        return Err("the filesystem stayed readable".to_string());
    }
    if std::net::UdpSocket::bind("127.0.0.1:0").is_ok() {
        return Err("UDP sockets stayed available".to_string());
    }
    if std::os::unix::net::UnixDatagram::unbound().is_ok() {
        return Err("local datagram sockets stayed available".to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("ganbaru-sandbox: kernel confinement is available only on Linux");
    ExitCode::from(REFUSED)
}
