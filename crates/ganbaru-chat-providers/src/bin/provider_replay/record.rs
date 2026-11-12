//! Record mode of the fake provider CLI: runs the real CLI with the same arguments, relays its
//! stdio both ways, and appends every frame to `GANBARU_REPLAY_RECORD_OUTPUT` as it passes.
//!
//! The output is written line by line, so a recording survives when the driver stops the
//! process tree before the CLI exits on its own; only the closing `runtime_exit` is missing then.
//! The output holds the unredacted exchange and stays in a temporary directory until the recorder
//! test redacts it into a scenario transcript.

use super::SETUP_FAILURE;
use super::transcript::ReplayEntry;
use serde_json::Value;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};

/// Exit status when the real CLI cannot run or the recording cannot be written.
const RECORD_FAILURE: u8 = 71;
/// The variable that selects record mode; the real CLI never sees it, so a recording of the fake
/// CLI itself replays instead of recording again.
pub(crate) const RECORD_TARGET_VARIABLE: &str = "GANBARU_REPLAY_RECORD_TARGET";

/// The append-only recording shared by the relay threads.
#[derive(Clone)]
struct Recording {
    file: Arc<Mutex<File>>,
}

impl Recording {
    fn create(path: &PathBuf) -> Result<Self, (u8, String)> {
        let file = File::create(path).map_err(|error| {
            (
                RECORD_FAILURE,
                format!("cannot create {}: {error}", path.display()),
            )
        })?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Appends one entry as a JSON line. A failed write is reported on stderr and leaves the
    /// relay running, so the driver still sees the real CLI's behavior.
    fn append(&self, entry: &ReplayEntry) {
        let line = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("ganbaru-provider-replay: cannot encode a recorded entry: {error}");
                return;
            }
        };
        let mut file = match self.file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(error) = writeln!(file, "{line}").and_then(|()| file.flush()) {
            eprintln!("ganbaru-provider-replay: cannot write the recording: {error}");
        }
    }
}

/// Runs `target` with this process's arguments and records the exchange.
pub(crate) fn run(target: PathBuf) -> Result<ExitCode, (u8, String)> {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--version") {
        return relay_version(&target, &arguments);
    }
    let output = required_variable("GANBARU_REPLAY_RECORD_OUTPUT")?;
    let provider = required_string("GANBARU_REPLAY_RECORD_PROVIDER")?;
    let scenario = required_string("GANBARU_REPLAY_RECORD_SCENARIO")?;
    let protocol = required_string("GANBARU_REPLAY_RECORD_PROTOCOL")?;
    let cli_version = real_version(&target)?;
    let recording = Recording::create(&PathBuf::from(output))?;
    recording.append(&ReplayEntry::TranscriptStart {
        provider,
        protocol,
        cli_version,
        scenario,
        generated_by: "recorded".to_string(),
        launch_arguments: Some(
            arguments
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect(),
        ),
    });

    let mut child = Command::new(&target)
        .args(&arguments)
        .env_remove(RECORD_TARGET_VARIABLE)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            (
                RECORD_FAILURE,
                format!("cannot start {}: {error}", target.display()),
            )
        })?;
    let (Some(child_stdin), Some(child_stdout), Some(mut child_stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.kill();
        return Err((
            RECORD_FAILURE,
            "the real CLI has no piped stdio".to_string(),
        ));
    };

    let outbound = recording.clone();
    std::thread::spawn(move || relay_stdin(child_stdin, &outbound));
    let stderr = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = child_stderr.read_to_string(&mut text);
        text
    });
    relay_stdout(child_stdout, &recording);

    let status = child.wait().map_err(|error| {
        (
            RECORD_FAILURE,
            format!("cannot wait for the real CLI: {error}"),
        )
    })?;
    let stderr = stderr.join().unwrap_or_default();
    eprint!("{stderr}");
    let code = exit_status_code(status);
    recording.append(&ReplayEntry::RuntimeExit {
        status: code,
        stderr: Some(stderr).filter(|stderr| !stderr.is_empty()),
    });
    Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
}

fn required_variable(name: &str) -> Result<OsString, (u8, String)> {
    std::env::var_os(name).ok_or((SETUP_FAILURE, format!("{name} is not set")))
}

fn required_string(name: &str) -> Result<String, (u8, String)> {
    std::env::var(name).map_err(|_| (SETUP_FAILURE, format!("{name} is not set or not UTF-8")))
}

/// Runs the real `--version` with inherited stdio and exits as it did.
fn relay_version(target: &PathBuf, arguments: &[OsString]) -> Result<ExitCode, (u8, String)> {
    let status = Command::new(target)
        .args(arguments)
        .env_remove(RECORD_TARGET_VARIABLE)
        .status()
        .map_err(|error| {
            (
                RECORD_FAILURE,
                format!("cannot run {}: {error}", target.display()),
            )
        })?;
    Ok(ExitCode::from(
        u8::try_from(exit_status_code(status)).unwrap_or(1),
    ))
}

/// The version the real CLI reports: the first token of `--version` that starts with a digit.
fn real_version(target: &PathBuf) -> Result<String, (u8, String)> {
    let output = Command::new(target)
        .arg("--version")
        .env_remove(RECORD_TARGET_VARIABLE)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            (
                RECORD_FAILURE,
                format!("cannot run {}: {error}", target.display()),
            )
        })?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .find(|token| token.starts_with(|character: char| character.is_ascii_digit()))
        .map(str::to_string)
        .ok_or((
            RECORD_FAILURE,
            format!("the real CLI printed no version: {}", text.trim()),
        ))
}

/// Forwards each driver line to the real CLI, recording it as an expected outbound frame. The
/// real CLI's stdin closes when the driver closes ours.
fn relay_stdin(mut child_stdin: std::process::ChildStdin, recording: &Recording) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        recording.append(&ReplayEntry::ExpectOutbound {
            label: None,
            frame: frame_of(&line),
        });
        if writeln!(child_stdin, "{line}")
            .and_then(|()| child_stdin.flush())
            .is_err()
        {
            break;
        }
    }
}

/// Forwards each real CLI line to the driver, recording it as an inbound frame.
fn relay_stdout(child_stdout: std::process::ChildStdout, recording: &Recording) {
    let mut output = std::io::stdout().lock();
    for line in BufReader::new(child_stdout).lines() {
        let Ok(line) = line else {
            break;
        };
        recording.append(&ReplayEntry::EmitInbound {
            label: None,
            frame: frame_of(&line),
            after_ms: None,
        });
        if writeln!(output, "{line}")
            .and_then(|()| output.flush())
            .is_err()
        {
            break;
        }
    }
}

/// A line as a transcript frame: parsed JSON, or the raw line as a string when it is not JSON or
/// is a bare JSON string, which a string frame would otherwise replay without its quotes.
fn frame_of(line: &str) -> Value {
    match serde_json::from_str(line) {
        Ok(Value::String(_)) | Err(_) => Value::String(line.to_string()),
        Ok(frame) => frame,
    }
}

/// The exit code of `status`, or 128 plus the signal number when a signal ended it on Unix.
fn exit_status_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}
