//! Bounded execution of fixed system commands used by desktop integrations.

use std::io::Read;
use std::process::{Command, Stdio};

/// Run a fixed program and return its UTF-8 stdout, failing when output exceeds `max_bytes`.
pub(crate) fn fixed_command_output(
    program: &str,
    args: &[&str],
    max_bytes: usize,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{program} stdout pipe was unavailable"))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(4096) + 1);
    let mut limited = (&mut stdout).take((max_bytes + 1) as u64);
    std::io::Read::read_to_end(&mut limited, &mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > max_bytes {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{program} output exceeded {max_bytes} bytes"));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("{program} exited with status {status}"));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Run a fixed program with discarded output and report whether it exited successfully.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn fixed_command_status(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
