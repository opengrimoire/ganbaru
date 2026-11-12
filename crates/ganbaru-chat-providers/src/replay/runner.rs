//! In-process replay: plays a transcript over a duplex pair while a script drives the driver.

use super::diff::json_diff;
use super::scripts::{ReplayScript, ReplayStep};
use super::transcript::{
    ReplayAction, ReplayBindings, ReplayCursor, ReplayMask, Transcript, load_transcript,
    mask_for_provider,
};
use crate::events::CanonicalRuntimeEvent;
use crate::models::*;
use crate::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink,
};
use serde_json::{Map, Value, json};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::{Notify, watch};

/// How long the provider side waits for an expected frame, on the paused clock.
const EXPECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a script step waits for an event or the transcript, on the paused clock.
const STEP_TIMEOUT: Duration = Duration::from_secs(20);
/// The deadline of every driver call a script makes.
const CALL_DEADLINE: Duration = Duration::from_secs(60);
/// The idle time left for the driver to process trailing frames before the replay ends.
const SETTLE_TIME: Duration = Duration::from_millis(50);
/// Buffer size of each duplex direction.
const DUPLEX_BYTES: usize = 1024 * 1024;
/// Payload fields an event snapshot keeps; everything else is too volatile or too large.
const SNAPSHOT_FIELDS: &[&str] = &[
    "kind",
    "status",
    "state",
    "streamKind",
    "delta",
    "stopReason",
    "reason",
    "recoverable",
    "code",
    "requestId",
    "decision",
    "limited",
    "resetsAt",
];

/// The provider reference schema of the rollback cursor a `Rollback` step sends.
const ROLLBACK_CURSOR_SCHEMA_VERSION: u32 = 1;

static NEXT_REPLAY_DIRECTORY: AtomicU64 = AtomicU64::new(1);

/// The driver ends of one replay connection.
pub(crate) struct ReplayConnection {
    /// What the driver reads: the provider's stdout.
    pub(crate) reader: DuplexStream,
    /// What the driver writes: the provider's stdin.
    pub(crate) writer: DuplexStream,
}

/// The directories and authority one replayed driver runs with.
pub(crate) struct ReplaySetup<'a> {
    pub(crate) workspace: &'a Path,
    pub(crate) home: &'a Path,
    pub(crate) organizational: bool,
    /// The CLI version the transcript header names; empty while recording a real CLI.
    pub(crate) cli_version: &'a str,
}

/// A connection factory body that hands out `connection` once and refuses every later launch.
pub(crate) fn one_shot<T>(connection: T) -> impl Fn() -> ChatResult<T> + Send + Sync
where
    T: Send,
{
    let slot = Mutex::new(Some(connection));
    move || {
        slot.lock()
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::TransportUnavailable,
                    "the replay connection lock was poisoned",
                    false,
                )
            })?
            .take()
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::TransportUnavailable,
                    "the replay connection was already used",
                    false,
                )
            })
    }
}

/// The internal MCP endpoint an organizational replay launches with. Nothing listens on it; the
/// transcript answers the CLI's MCP status itself, and a real CLI being recorded reports it as
/// unreachable.
pub(crate) fn organizational_mcp() -> ProviderInternalMcpConfig {
    ProviderInternalMcpConfig {
        name: "ganbaru-chat".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "replay-token".to_string(),
        organizational_authority: true,
    }
}

/// A provider the replay harness can drive in process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReplayProvider {
    Claude,
    Codex,
    Cursor,
}

/// Every provider the harness drives, in scenario directory order.
const REPLAY_PROVIDERS: [ReplayProvider; 3] = [
    ReplayProvider::Claude,
    ReplayProvider::Codex,
    ReplayProvider::Cursor,
];

impl ReplayProvider {
    /// The provider name transcript headers and scenario directories use.
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }

    /// The mask of this provider, after checking the transcript was recorded from it.
    fn mask(self, transcript: &Transcript) -> Result<ReplayMask, ReplayFailure> {
        if transcript.header.provider != self.name() {
            return Err(ReplayFailure::new(
                Some(1),
                None,
                format!(
                    "transcript names provider {}, not {}",
                    transcript.header.provider,
                    self.name()
                ),
            ));
        }
        mask_for_provider(self.name()).ok_or_else(|| {
            ReplayFailure::new(None, None, format!("no replay mask for {}", self.name()))
        })
    }

    /// The directory holding this provider's scenario transcripts.
    pub(crate) fn scenario_directory(self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(self.name())
            .join("fixtures")
            .join("scenarios")
    }

    fn driver(
        self,
        setup: &ReplaySetup<'_>,
        connection: ReplayConnection,
    ) -> ChatResult<Box<dyn ProviderDriver>> {
        match self {
            Self::Claude => crate::claude::replay::replay_driver(setup, connection),
            Self::Codex => crate::codex::replay::replay_driver(setup, connection),
            Self::Cursor => crate::cursor::replay::replay_driver(setup, connection),
        }
    }

    /// The wire protocol transcript headers name for this provider.
    pub(super) fn protocol(self) -> &'static str {
        match self {
            Self::Claude => "stream-json",
            Self::Codex => "app-server",
            Self::Cursor => "acp",
        }
    }

    /// The provider with transcript header name `name`.
    pub(super) fn from_name(name: &str) -> Option<Self> {
        REPLAY_PROVIDERS
            .into_iter()
            .find(|provider| provider.name() == name)
    }

    /// A driver that spawns `executable` with `environment` instead of a replay connection.
    pub(super) fn recording_driver(
        self,
        setup: &ReplaySetup<'_>,
        executable: &Path,
        environment: std::collections::BTreeMap<String, String>,
    ) -> ChatResult<Box<dyn ProviderDriver>> {
        match self {
            Self::Claude => crate::claude::replay::recording_driver(setup, executable, environment),
            Self::Codex => crate::codex::replay::recording_driver(setup, executable, environment),
            Self::Cursor => crate::cursor::replay::recording_driver(setup, executable, environment),
        }
    }
}

/// What a passing replay produced.
#[derive(Debug)]
pub(crate) struct ReplayReport {
    /// The calls and canonical events, in the snapshot shape.
    pub(crate) snapshot: Value,
}

/// Why a replay failed, naming the transcript line and frames involved.
#[derive(Debug)]
pub(crate) struct ReplayFailure {
    pub(crate) line: Option<usize>,
    pub(crate) label: Option<String>,
    pub(crate) message: String,
    pub(crate) expected: Option<Box<Value>>,
    pub(crate) actual: Option<Box<Value>>,
}

impl ReplayFailure {
    fn new(line: Option<usize>, label: Option<String>, message: impl Into<String>) -> Self {
        Self {
            line,
            label,
            message: message.into(),
            expected: None,
            actual: None,
        }
    }
}

impl fmt::Display for ReplayFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(formatter, "transcript line {line}")?,
            None => formatter.write_str("replay")?,
        }
        if let Some(label) = &self.label {
            write!(formatter, " ({label})")?;
        }
        write!(formatter, ": {}", self.message)?;
        match (&self.expected, &self.actual) {
            (Some(expected), Some(actual)) => {
                write!(formatter, "\n{}", json_diff(expected, actual))?
            }
            (Some(expected), None) => write!(formatter, "\nexpected: {expected}")?,
            (None, Some(actual)) => write!(formatter, "\nactual: {actual}")?,
            (None, None) => {}
        }
        Ok(())
    }
}

/// Loads a transcript file, mapping load errors to failures.
pub(crate) fn read_transcript(path: &Path) -> Result<Transcript, ReplayFailure> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        ReplayFailure::new(
            None,
            None,
            format!("cannot read {}: {error}", path.display()),
        )
    })?;
    load_transcript(&text).map_err(|error| ReplayFailure::new(None, None, error.to_string()))
}

/// Replays `<scenario>.jsonl` of `provider` against its driver with the matching script, then
/// compares the snapshot with `<scenario>.events.json`.
///
/// Setting `GANBARU_REPLAY_BLESS=1` writes the snapshot instead of comparing it.
pub(crate) fn run_scenario(
    provider: ReplayProvider,
    scenario: &str,
) -> Result<ReplayReport, ReplayFailure> {
    let directory = provider.scenario_directory();
    let transcript = read_transcript(&directory.join(format!("{scenario}.jsonl")))?;
    if transcript.header.scenario != scenario {
        return Err(ReplayFailure::new(
            Some(1),
            None,
            format!(
                "transcript names scenario {}, not {scenario}",
                transcript.header.scenario
            ),
        ));
    }
    let script = super::scripts::script(provider, scenario).ok_or_else(|| {
        ReplayFailure::new(None, None, format!("no replay script for {scenario}"))
    })?;
    let report = replay(provider, transcript, &script)?;
    compare_snapshot(&directory.join(format!("{scenario}.events.json")), &report)?;
    Ok(report)
}

/// Replays an already loaded transcript with `script`.
pub(crate) fn replay(
    provider: ReplayProvider,
    transcript: Transcript,
    script: &ReplayScript,
) -> Result<ReplayReport, ReplayFailure> {
    let directories = ReplayDirectories::new()?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .expect("replay runtime must start")
        .block_on(replay_async(provider, transcript, script, &directories))
}

async fn replay_async(
    provider: ReplayProvider,
    transcript: Transcript,
    script: &ReplayScript,
    directories: &ReplayDirectories,
) -> Result<ReplayReport, ReplayFailure> {
    let workspace = directories.workspace.to_string_lossy().into_owned();
    let (driver_reader, process_writer) = tokio::io::duplex(DUPLEX_BYTES);
    let (process_reader, driver_writer) = tokio::io::duplex(DUPLEX_BYTES);
    let cli_version = transcript.header.cli_version.clone();
    let setup = ReplaySetup {
        workspace: &directories.workspace,
        home: &directories.home,
        organizational: script.organizational,
        cli_version: &cli_version,
    };
    let driver = provider
        .driver(
            &setup,
            ReplayConnection {
                reader: driver_reader,
                writer: driver_writer,
            },
        )
        .map_err(|error| ReplayFailure::new(None, None, format!("driver setup: {error:?}")))?;
    let mask = provider.mask(&transcript)?;
    let cursor = ReplayCursor::new(
        transcript,
        mask,
        ReplayBindings::new(workspace.clone(), None)
            .with_home(directories.home.to_string_lossy().into_owned()),
    );
    let (finished_sender, finished) = watch::channel(false);
    let unexpected = Arc::new(Mutex::new(Vec::new()));
    let process = tokio::spawn(play(
        cursor,
        process_reader,
        process_writer,
        finished_sender,
        unexpected.clone(),
    ));
    let sink = Arc::new(ReplaySink::default());
    let calls = run_script(driver, script, &setup, sink.clone(), finished).await;
    let outcome = match tokio::time::timeout(SETTLE_TIME, process).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => Err(ReplayFailure::new(
            None,
            None,
            format!("replay task failed: {error}"),
        )),
        Err(_) => Ok(()),
    };
    outcome?;
    if let Some(frame) = unexpected.lock().unwrap().first() {
        let mut failure = ReplayFailure::new(
            None,
            None,
            "the driver wrote a frame after the transcript ended",
        );
        failure.actual = Some(Box::new(frame.clone()));
        return Err(failure);
    }
    let mut request_ids = Vec::new();
    let events = sink
        .events()
        .iter()
        .map(|event| summarize(event, &workspace, &mut request_ids))
        .collect::<Vec<_>>();
    Ok(ReplayReport {
        snapshot: json!({ "calls": calls?, "events": events }),
    })
}

/// The provider side: compares driver frames with expectations and writes inbound frames.
async fn play(
    mut cursor: ReplayCursor,
    reader: DuplexStream,
    mut writer: DuplexStream,
    finished: watch::Sender<bool>,
    unexpected: Arc<Mutex<Vec<Value>>>,
) -> Result<(), ReplayFailure> {
    let mut reader = BufReader::new(reader);
    let result = loop {
        let action = match cursor.advance() {
            Ok(action) => action,
            Err((line, reason)) => break Err(ReplayFailure::new(Some(line), None, reason)),
        };
        match action {
            ReplayAction::Expect { line, label, frame } => {
                let actual = match read_frame(&mut reader).await {
                    Ok(Some(actual)) => actual,
                    Ok(None) => {
                        let mut failure = ReplayFailure::new(
                            Some(line),
                            label,
                            "the driver closed its input before writing the expected frame",
                        );
                        failure.expected = Some(Box::new(cursor.display_expected(&frame)));
                        break Err(failure);
                    }
                    Err(reason) => {
                        let mut failure = ReplayFailure::new(Some(line), label, reason);
                        failure.expected = Some(Box::new(cursor.display_expected(&frame)));
                        break Err(failure);
                    }
                };
                if let Err(reason) = cursor.accept(&frame, label.as_deref(), &actual) {
                    let mut failure = ReplayFailure::new(
                        Some(line),
                        label,
                        format!("outbound frame differs at {reason}"),
                    );
                    failure.expected = Some(Box::new(cursor.display_expected(&frame)));
                    failure.actual = Some(Box::new(actual));
                    break Err(failure);
                }
            }
            ReplayAction::Emit {
                line,
                label,
                frame,
                after_ms,
            } => {
                if after_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(after_ms)).await;
                }
                if let Err(reason) = write_frame(&mut writer, &frame).await {
                    break Err(ReplayFailure::new(Some(line), label, reason));
                }
            }
            ReplayAction::Exit { .. } => {
                drop(writer);
                drop(reader);
                let _ = finished.send(true);
                return Ok(());
            }
            ReplayAction::Done => {
                let _ = finished.send(true);
                while let Ok(Some(frame)) = read_frame(&mut reader).await {
                    unexpected.lock().unwrap().push(frame);
                }
                return Ok(());
            }
        }
    };
    let _ = finished.send(true);
    result
}

async fn read_frame(reader: &mut BufReader<DuplexStream>) -> Result<Option<Value>, String> {
    let mut line = String::new();
    match tokio::time::timeout(EXPECT_TIMEOUT, reader.read_line(&mut line)).await {
        Err(_) => Err("the driver never wrote the expected frame".to_string()),
        Ok(Err(error)) => Err(format!("cannot read the driver frame: {error}")),
        Ok(Ok(0)) => Ok(None),
        Ok(Ok(_)) => serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| format!("the driver wrote invalid JSON ({error}): {line}")),
    }
}

async fn write_frame(writer: &mut DuplexStream, frame: &Value) -> Result<(), String> {
    let mut bytes = match frame {
        Value::String(raw) => raw.clone().into_bytes(),
        frame => serde_json::to_vec(frame).map_err(|error| error.to_string())?,
    };
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .map_err(|error| format!("cannot write the inbound frame: {error}"))
}

/// Runs the script steps against the driver, returning one outcome per step.
pub(super) async fn run_script(
    mut driver: Box<dyn ProviderDriver>,
    script: &ReplayScript,
    setup: &ReplaySetup<'_>,
    sink: Arc<ReplaySink>,
    mut finished: watch::Receiver<bool>,
) -> Result<Vec<Value>, ReplayFailure> {
    let mut calls = Vec::new();
    let mut session: Option<ProviderSessionId> = None;
    let mut command = 0_u32;
    for (index, step) in script.steps.iter().enumerate() {
        command += 1;
        let context = DriverOperationContext {
            operation_id: format!("replay-step-{}", index + 1),
            deadline: Instant::now() + CALL_DEADLINE,
            cancellation: DriverCancellation::default(),
        };
        let command_context = ChatCommandContext {
            client_command_id: ChatCommandId::new(format!("replay-command-{command}"))
                .expect("replay command id is valid"),
            expected_thread_revision: None,
        };
        let outcome = match step {
            ReplayStep::Start {
                safety,
                interaction,
            } => {
                let request = start_request(
                    setup,
                    driver.instance_configuration(),
                    *safety,
                    *interaction,
                );
                match driver.start_session(request, sink.clone(), &context).await {
                    Ok(snapshot) => {
                        session = Some(snapshot.session_id);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            ReplayStep::Send {
                turn,
                prompt,
                safety,
                interaction,
            } => {
                let session_id = require_session(&session, index)?;
                driver
                    .send_turn(
                        SendTurnRequest {
                            command: command_context,
                            session_id,
                            turn_id: turn_id(turn),
                            prompt: (*prompt).to_string(),
                            attachments: Vec::new(),
                            mentions: Vec::new(),
                            model_id: None,
                            model_options: Vec::new(),
                            modes: TurnModeSnapshot {
                                safety_mode: *safety,
                                interaction_mode: *interaction,
                            },
                            developer_instructions: None,
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
            ReplayStep::Steer { turn, prompt } => {
                let session_id = require_session(&session, index)?;
                driver
                    .steer_turn(
                        SteerTurnRequest {
                            command: command_context,
                            session_id,
                            turn_id: turn_id(turn),
                            prompt: (*prompt).to_string(),
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
            ReplayStep::Interrupt { turn } => {
                let session_id = require_session(&session, index)?;
                driver
                    .interrupt_turn(
                        InterruptTurnRequest {
                            command: command_context,
                            session_id,
                            turn_id: turn_id(turn),
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
            ReplayStep::Rollback { provider_turn } => {
                let session_id = require_session(&session, index)?;
                driver
                    .rollback(
                        RollbackRequest {
                            command: command_context,
                            session_id,
                            checkpoint_id: None,
                            provider_cursor: Some(VersionedJson {
                                schema_version: ROLLBACK_CURSOR_SCHEMA_VERSION,
                                value: json!({
                                    "method": "turn/completed",
                                    "turnId": provider_turn,
                                }),
                            }),
                            target_turn_id: None,
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
            ReplayStep::Resolve { decision } => {
                let session_id = require_session(&session, index)?;
                let request_id = sink.latest_request().ok_or_else(|| {
                    ReplayFailure::new(
                        None,
                        None,
                        format!("step {}: no request is open", index + 1),
                    )
                })?;
                driver
                    .resolve_approval(
                        ResolveApprovalRequest {
                            command: command_context,
                            session_id,
                            request_id: ChatRequestId::new(format!("replay-request-{command}"))
                                .expect("replay request id is valid"),
                            provider_request_id: request_id,
                            decision: ApprovalDecision {
                                kind: *decision,
                                provider_option_id: None,
                                updated_tool_input: None,
                            },
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
            ReplayStep::AwaitEvent { kind, count } => {
                if !sink.wait_for(kind, *count).await {
                    return Err(ReplayFailure::new(
                        None,
                        None,
                        format!(
                            "step {}: the driver emitted fewer than {count} {kind} events; events: {}",
                            index + 1,
                            Value::Array(sink.events().iter().map(event_kind).collect())
                        ),
                    ));
                }
                continue;
            }
            ReplayStep::AwaitTranscript => {
                await_transcript(&mut finished, index).await?;
                continue;
            }
            ReplayStep::Stop => {
                let session_id = require_session(&session, index)?;
                driver
                    .stop_session(
                        StopSessionRequest {
                            session_id,
                            force: false,
                        },
                        &context,
                    )
                    .await
                    .map(|_| ())
            }
        };
        calls.push(call_summary(step, outcome));
    }
    await_transcript(&mut finished, script.steps.len()).await?;
    tokio::time::sleep(SETTLE_TIME).await;
    drop(driver);
    Ok(calls)
}

async fn await_transcript(
    finished: &mut watch::Receiver<bool>,
    index: usize,
) -> Result<(), ReplayFailure> {
    match tokio::time::timeout(STEP_TIMEOUT, finished.wait_for(|done| *done)).await {
        Ok(_) => Ok(()),
        Err(_) => Err(ReplayFailure::new(
            None,
            None,
            format!("after step {index}: the transcript still waits for driver frames"),
        )),
    }
}

fn require_session(
    session: &Option<ProviderSessionId>,
    index: usize,
) -> Result<ProviderSessionId, ReplayFailure> {
    session.clone().ok_or_else(|| {
        ReplayFailure::new(
            None,
            None,
            format!("step {}: no session was started", index + 1),
        )
    })
}

fn turn_id(turn: &str) -> ChatTurnId {
    ChatTurnId::new(turn.to_string()).expect("replay turn id is valid")
}

fn start_request(
    setup: &ReplaySetup<'_>,
    configuration: &ProviderInstanceConfig,
    safety: SafetyMode,
    interaction: InteractionMode,
) -> StartSessionRequest {
    StartSessionRequest {
        thread_id: ChatThreadId::new("replay-thread".to_string()).expect("thread id is valid"),
        workspace: VerifiedWorkspaceContext {
            working_folder_id: ProjectWorkingFolderId::new("replay-folder".to_string())
                .expect("working folder id is valid"),
            canonical_path: setup.workspace.to_string_lossy().into_owned(),
            repository_kind: RepositoryKind::None,
            repository_identity: None,
            project_configuration: ProjectConfigurationAccess::Restricted,
        },
        provider_instance_id: configuration.instance_id.clone(),
        modes: TurnModeSnapshot {
            safety_mode: safety,
            interaction_mode: interaction,
        },
        model_id: None,
        model_options: Vec::new(),
        developer_instructions: None,
    }
}

fn call_summary(step: &ReplayStep, outcome: ChatResult<()>) -> Value {
    let mut summary = Map::new();
    summary.insert("call".to_string(), json!(step.name()));
    match outcome {
        Ok(()) => {
            summary.insert("result".to_string(), json!("ok"));
        }
        Err(error) => {
            summary.insert("result".to_string(), json!("error"));
            summary.insert(
                "code".to_string(),
                serde_json::to_value(error.code).unwrap_or(Value::Null),
            );
        }
    }
    Value::Object(summary)
}

fn event_kind(event: &CanonicalRuntimeEvent) -> Value {
    serde_json::to_value(&event.event)
        .ok()
        .and_then(|value| value.get("type").cloned())
        .unwrap_or(Value::Null)
}

/// The snapshot name of `request_id`: `request-<n>` in order of first appearance, because some
/// drivers derive request ids from a random session id.
fn stable_request_id(request_id: &str, seen: &mut Vec<String>) -> String {
    let index = match seen.iter().position(|known| known == request_id) {
        Some(index) => index,
        None => {
            seen.push(request_id.to_string());
            seen.len() - 1
        }
    };
    format!("request-{}", index + 1)
}

/// The snapshot shape of one event: its kind, turn, and a few stable payload fields, with the
/// workspace path written as `<workspace>` and request ids numbered by `stable_request_id`.
fn summarize(
    event: &CanonicalRuntimeEvent,
    workspace: &str,
    request_ids: &mut Vec<String>,
) -> Value {
    let mut summary = Map::new();
    summary.insert("type".to_string(), event_kind(event));
    if let Some(turn) = &event.turn_id {
        summary.insert("turn".to_string(), json!(turn.as_str()));
    }
    let payload = serde_json::to_value(&event.event)
        .ok()
        .and_then(|value| value.get("payload").cloned());
    if let Some(Value::Object(payload)) = payload {
        for field in SNAPSHOT_FIELDS {
            match payload.get(*field) {
                Some(Value::String(text)) if *field == "requestId" => {
                    summary.insert(
                        (*field).to_string(),
                        json!(stable_request_id(text, request_ids)),
                    );
                }
                Some(Value::String(text)) => {
                    summary.insert(
                        (*field).to_string(),
                        json!(text.replace(workspace, "<workspace>")),
                    );
                }
                Some(value @ (Value::Bool(_) | Value::Number(_))) => {
                    summary.insert((*field).to_string(), value.clone());
                }
                Some(Value::Object(object)) if *field == "decision" => {
                    if let Some(kind) = object.get("kind") {
                        summary.insert((*field).to_string(), kind.clone());
                    }
                }
                _ => {}
            }
        }
    }
    Value::Object(summary)
}

fn compare_snapshot(path: &Path, report: &ReplayReport) -> Result<(), ReplayFailure> {
    let rendered =
        serde_json::to_string_pretty(&report.snapshot).expect("snapshot serializes") + "\n";
    if std::env::var_os("GANBARU_REPLAY_BLESS").is_some() {
        return std::fs::write(path, rendered).map_err(|error| {
            ReplayFailure::new(
                None,
                None,
                format!("cannot write {}: {error}", path.display()),
            )
        });
    }
    let stored = std::fs::read_to_string(path).map_err(|error| {
        let mut failure = ReplayFailure::new(
            None,
            None,
            format!(
                "cannot read the snapshot {} ({error}); run with GANBARU_REPLAY_BLESS=1 to write it",
                path.display()
            ),
        );
        failure.actual = Some(Box::new(report.snapshot.clone()));
        failure
    })?;
    let stored: Value = serde_json::from_str(&stored).map_err(|error| {
        ReplayFailure::new(
            None,
            None,
            format!("snapshot {} is invalid: {error}", path.display()),
        )
    })?;
    if stored != report.snapshot {
        let mut failure = ReplayFailure::new(
            None,
            None,
            "the driver calls or events differ from the snapshot",
        );
        failure.expected = Some(Box::new(stored));
        failure.actual = Some(Box::new(report.snapshot.clone()));
        return Err(failure);
    }
    Ok(())
}

/// Collects the driver's canonical events and wakes script steps that wait for one.
pub(crate) struct ReplaySink {
    events: Mutex<Vec<CanonicalRuntimeEvent>>,
    changed: Notify,
    /// How long a step waits for an event: paused clock time in a replay, wall time when a real
    /// CLI is recorded.
    step_timeout: Duration,
}

impl Default for ReplaySink {
    fn default() -> Self {
        Self::with_step_timeout(STEP_TIMEOUT)
    }
}

impl ReplaySink {
    /// A sink whose event waits give up after `step_timeout`.
    pub(super) fn with_step_timeout(step_timeout: Duration) -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            changed: Notify::new(),
            step_timeout,
        }
    }

    pub(crate) fn events(&self) -> Vec<CanonicalRuntimeEvent> {
        self.events.lock().unwrap().clone()
    }

    fn latest_request(&self) -> Option<ProviderRequestId> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match &event.event {
                crate::events::CanonicalEvent::RequestOpened(opened) => {
                    Some(opened.request_id.clone())
                }
                _ => None,
            })
    }

    async fn wait_for(&self, kind: &str, count: usize) -> bool {
        let reached = || {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event_kind(event) == json!(kind))
                .count()
                >= count
        };
        tokio::time::timeout(self.step_timeout, async {
            loop {
                let changed = self.changed.notified();
                if reached() {
                    return;
                }
                changed.await;
            }
        })
        .await
        .is_ok()
    }
}

impl ProviderEventSink for ReplaySink {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            self.changed.notify_waiters();
            Ok(())
        })
    }
}

/// The workspace and provider home of one replay, removed when the replay ends.
pub(super) struct ReplayDirectories {
    pub(super) root: PathBuf,
    pub(super) workspace: PathBuf,
    pub(super) home: PathBuf,
}

impl ReplayDirectories {
    pub(super) fn new() -> Result<Self, ReplayFailure> {
        let root = std::env::temp_dir().join(format!(
            "ganbaru-replay-{}-{}",
            std::process::id(),
            NEXT_REPLAY_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = root.join("workspace");
        let home = root.join("home");
        for directory in [&workspace, &home] {
            std::fs::create_dir_all(directory).map_err(|error| {
                ReplayFailure::new(
                    None,
                    None,
                    format!("cannot create {}: {error}", directory.display()),
                )
            })?;
        }
        let workspace = std::fs::canonicalize(&workspace).map_err(|error| {
            ReplayFailure::new(None, None, format!("cannot resolve the workspace: {error}"))
        })?;
        Ok(Self {
            root,
            workspace,
            home,
        })
    }
}

impl Drop for ReplayDirectories {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
