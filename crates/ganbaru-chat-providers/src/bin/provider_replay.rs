//! Fake provider CLI that replays one transcript over stdio for spawn-path tests.
//!
//! `GANBARU_REPLAY_TRANSCRIPT` names the transcript and `GANBARU_REPLAY_WORKSPACE` the path
//! `<workspace>` stands for (the working directory when unset). `--version` prints the
//! transcript's CLI version in the provider's format. Any other invocation checks argv against
//! `launch_arguments` when the header has them, then plays the transcript: each driver line on
//! stdin must match the next expected frame, and inbound frames go to stdout. The process exits
//! with the `runtime_exit` status, or 0 when stdin closes after the last entry. On a mismatch
//! it exits with a distinct status and, when `GANBARU_REPLAY_FAILURE_LOG` is set, appends the
//! reason to that file.
//!
//! With `GANBARU_REPLAY_RECORD_TARGET` set, it records instead: it runs that real CLI with the
//! same arguments, relays stdio both ways, and writes the unredacted exchange to
//! `GANBARU_REPLAY_RECORD_OUTPUT` when the CLI exits. The recorder test redacts it.

#[path = "provider_replay/record.rs"]
mod record;
#[allow(
    dead_code,
    reason = "the in-process runner uses the rest of this module"
)]
#[path = "../replay/transcript.rs"]
mod transcript;

use serde_json::Value;
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;
use transcript::{
    ReplayAction, ReplayBindings, ReplayCursor, TranscriptHeader, load_transcript,
    mask_for_provider, match_frame,
};

/// Exit status when the transcript cannot be loaded or the environment is incomplete.
pub(crate) const SETUP_FAILURE: u8 = 70;
/// Exit status when argv or a driver frame differs from the transcript.
const REPLAY_MISMATCH: u8 = 65;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err((code, message)) => {
            let message = format!("ganbaru-provider-replay: {message}\n");
            eprint!("{message}");
            record_failure(&message);
            ExitCode::from(code)
        }
    }
}

/// Appends `message` to `GANBARU_REPLAY_FAILURE_LOG`, so a test can show why the replay
/// stopped even when the driver keeps stderr to itself.
fn record_failure(message: &str) {
    let Some(path) = std::env::var_os("GANBARU_REPLAY_FAILURE_LOG") else {
        return;
    };
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(message.as_bytes()));
    if let Err(error) = written {
        eprintln!("ganbaru-provider-replay: cannot write the failure log: {error}");
    }
}

fn run() -> Result<ExitCode, (u8, String)> {
    if let Some(target) = std::env::var_os(record::RECORD_TARGET_VARIABLE) {
        return record::run(target.into());
    }
    let path = std::env::var_os("GANBARU_REPLAY_TRANSCRIPT").ok_or((
        SETUP_FAILURE,
        "GANBARU_REPLAY_TRANSCRIPT is not set".to_string(),
    ))?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        (
            SETUP_FAILURE,
            format!("cannot read the transcript: {error}"),
        )
    })?;
    let transcript = load_transcript(&text).map_err(|error| (SETUP_FAILURE, error.to_string()))?;
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--version") {
        println!("{}", version_output(&transcript.header));
        return Ok(ExitCode::SUCCESS);
    }
    let mask = mask_for_provider(&transcript.header.provider).ok_or((
        SETUP_FAILURE,
        format!("no replay mask for provider {}", transcript.header.provider),
    ))?;
    let workspace = match std::env::var("GANBARU_REPLAY_WORKSPACE") {
        Ok(workspace) => workspace,
        Err(_) => std::env::current_dir()
            .map_err(|error| (SETUP_FAILURE, format!("no working directory: {error}")))?
            .to_string_lossy()
            .into_owned(),
    };
    let mut bindings = ReplayBindings::new(workspace, None);
    if let Some(expected) = &transcript.header.launch_arguments {
        check_arguments(expected, &arguments, &mut bindings)?;
    }
    let mut cursor = ReplayCursor::new(transcript, mask, bindings);
    play(&mut cursor)
}

/// The `--version` line the provider's own CLI prints.
fn version_output(header: &TranscriptHeader) -> String {
    match header.provider.as_str() {
        "claude" => format!("{} (Claude Code)", header.cli_version),
        _ => header.cli_version.clone(),
    }
}

/// Compares argv with the expected arguments; the first `<session>` binds the session.
fn check_arguments(
    expected: &[String],
    actual: &[String],
    bindings: &mut ReplayBindings,
) -> Result<(), (u8, String)> {
    let expected_value = Value::from(expected.to_vec());
    let actual_value = Value::from(actual.to_vec());
    let mask = transcript::ReplayMask {
        request_id: "",
        presence_only: &[],
    };
    match_frame(&expected_value, &actual_value, &mask, bindings).map_err(|reason| {
        (
            REPLAY_MISMATCH,
            format!("launch arguments differ at {reason}\nexpected: {expected_value}\nactual: {actual_value}"),
        )
    })
}

fn play(cursor: &mut ReplayCursor) -> Result<ExitCode, (u8, String)> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut output = std::io::stdout().lock();
    loop {
        let action = cursor.advance().map_err(|(line, reason)| {
            (REPLAY_MISMATCH, format!("transcript line {line}: {reason}"))
        })?;
        match action {
            ReplayAction::Expect { line, label, frame } => {
                let label = label.unwrap_or_default();
                let actual = read_frame(&mut input).map_err(|reason| {
                    (
                        REPLAY_MISMATCH,
                        format!("transcript line {line} ({label}): {reason}"),
                    )
                })?;
                let Some(actual) = actual else {
                    return Err((
                        REPLAY_MISMATCH,
                        format!(
                            "transcript line {line} ({label}): stdin closed before the expected frame"
                        ),
                    ));
                };
                cursor
                    .accept(&frame, Some(label.as_str()).filter(|label| !label.is_empty()), &actual)
                    .map_err(|reason| {
                        (
                            REPLAY_MISMATCH,
                            format!(
                                "transcript line {line} ({label}): outbound frame differs at {reason}\nexpected: {}\nactual: {actual}",
                                cursor.display_expected(&frame)
                            ),
                        )
                    })?;
            }
            ReplayAction::Emit {
                frame, after_ms, ..
            } => {
                if after_ms > 0 {
                    std::thread::sleep(Duration::from_millis(after_ms));
                }
                let line = match frame {
                    Value::String(raw) => raw,
                    frame => frame.to_string(),
                };
                writeln!(output, "{line}")
                    .and_then(|()| output.flush())
                    .map_err(|error| (REPLAY_MISMATCH, format!("cannot write stdout: {error}")))?;
            }
            ReplayAction::Exit { status, stderr, .. } => {
                if let Some(stderr) = stderr {
                    eprint!("{stderr}");
                }
                std::process::exit(status);
            }
            ReplayAction::Done => {
                return match read_frame(&mut input) {
                    Ok(None) => Ok(ExitCode::SUCCESS),
                    Ok(Some(frame)) => Err((
                        REPLAY_MISMATCH,
                        format!("the driver wrote a frame after the transcript ended: {frame}"),
                    )),
                    Err(reason) => Err((REPLAY_MISMATCH, reason)),
                };
            }
        }
    }
}

fn read_frame(input: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) => Ok(None),
        Ok(_) => serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| format!("the driver wrote invalid JSON ({error}): {line}")),
        Err(error) => Err(format!("cannot read stdin: {error}")),
    }
}
