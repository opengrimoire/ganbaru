//! Helper that confines itself under a sandbox policy and then executes the confined program.
//!
//! Ganbaru starts commands and provider processes of Chat teammates through this binary so that
//! no restriction is ever applied to the app itself. The binary refuses to run the program when
//! any part of the policy cannot be enforced. See [`ganbaru_sandbox::decode_invocation`] for
//! the argument form; `--probe` confines the helper under a policy that denies everything,
//! checks that the kernel refuses what the policy leaves out, and exits.

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
    let invocation = match ganbaru_sandbox::decode_invocation(arguments) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("ganbaru-sandbox: {error}");
            return ExitCode::from(REFUSED);
        }
    };
    if let Err(error) = ganbaru_sandbox::enforce(&invocation.policy) {
        eprintln!("ganbaru-sandbox: {error}");
        return ExitCode::from(REFUSED);
    }
    let error = std::process::Command::new(&invocation.program)
        .args(&invocation.arguments)
        .exec();
    eprintln!(
        "ganbaru-sandbox: {} could not be executed: {error}",
        invocation.program.to_string_lossy()
    );
    ExitCode::from(NOT_EXECUTED)
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
