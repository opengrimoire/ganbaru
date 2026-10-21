//! Bounded execution of fixed system commands used by desktop integrations.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Longest a fixed command may run before it is killed. Desktop probes normally answer in
/// milliseconds; the bound covers a cold PowerShell start and a stalled session bus.
const FIXED_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Run a fixed program and return its UTF-8 stdout, failing when output exceeds `max_bytes` or
/// the program runs past [`FIXED_COMMAND_TIMEOUT`].
pub(crate) fn fixed_command_output(
    program: &str,
    args: &[&str],
    max_bytes: usize,
) -> Result<String, String> {
    fixed_command_output_within(program, args, max_bytes, FIXED_COMMAND_TIMEOUT)
}

fn fixed_command_output_within(
    program: &str,
    args: &[&str],
    max_bytes: usize,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let bytes = match read_stdout_until(&mut child, program, max_bytes, deadline) {
        Ok(bytes) => bytes,
        Err(error) => {
            stop(&mut child);
            return Err(error);
        }
    };
    let status = wait_until(&mut child, program, deadline)?;
    if !status.success() {
        return Err(format!("{program} exited with status {status}"));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Reads at most `max_bytes` of stdout on a helper thread so a program that keeps its pipe open
/// cannot hold the caller past `deadline`.
fn read_stdout_until(
    child: &mut Child,
    program: &str,
    max_bytes: usize,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{program} stdout pipe was unavailable"))?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("fixed-command-output".into())
        .spawn(move || {
            let mut bytes = Vec::with_capacity(max_bytes.min(4096) + 1);
            let result = stdout
                .take((max_bytes + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = sender.send(result);
        })
        .map_err(|e| format!("start {program} output reader: {e}"))?;
    let bytes = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| timeout_error(program))?
        .map_err(|e| e.to_string())?;
    if bytes.len() > max_bytes {
        return Err(format!("{program} output exceeded {max_bytes} bytes"));
    }
    Ok(bytes)
}

fn wait_until(child: &mut Child, program: &str, deadline: Instant) -> Result<ExitStatus, String> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            stop(child);
            return Err(timeout_error(program));
        }
        std::thread::sleep(EXIT_POLL_INTERVAL);
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn timeout_error(program: &str) -> String {
    format!("{program} did not finish before its deadline")
}

/// Run a fixed program with discarded output and report whether it exited successfully within
/// [`FIXED_COMMAND_TIMEOUT`].
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn fixed_command_status(program: &str, args: &[&str]) -> bool {
    fixed_command_status_within(program, args, FIXED_COMMAND_TIMEOUT)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn fixed_command_status_within(program: &str, args: &[&str], timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .and_then(|mut child| wait_until(&mut child, program, deadline).ok())
        .is_some_and(|status| status.success())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const SHORT_TIMEOUT: Duration = Duration::from_millis(300);

    #[test]
    fn a_command_that_outlives_its_deadline_is_killed() {
        let started = Instant::now();
        let output = fixed_command_output_within("sleep", &["30"], 64, SHORT_TIMEOUT);
        assert!(output.unwrap_err().contains("did not finish"));
        assert!(!fixed_command_status_within(
            "sleep",
            &["30"],
            SHORT_TIMEOUT
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_command_that_holds_its_output_pipe_open_cannot_hold_the_caller() {
        let started = Instant::now();
        let output =
            fixed_command_output_within("sh", &["-c", "echo ready; sleep 30"], 64, SHORT_TIMEOUT);
        assert!(output.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn output_past_its_bound_is_refused() {
        let output = fixed_command_output_within("yes", &[], 16, SHORT_TIMEOUT);
        assert!(output.unwrap_err().contains("exceeded 16 bytes"));
    }

    #[test]
    fn a_quick_command_returns_its_output_and_status() {
        assert_eq!(
            fixed_command_output_within("echo", &["ready"], 64, SHORT_TIMEOUT).unwrap(),
            "ready\n"
        );
        assert!(fixed_command_status_within("true", &[], SHORT_TIMEOUT));
        assert!(!fixed_command_status_within("false", &[], SHORT_TIMEOUT));
    }
}
