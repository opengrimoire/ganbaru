//! Recording real CLI transcripts: drives a real provider CLI through the fake CLI's record mode
//! with a scenario script, then redacts the raw exchange into a scenario transcript.
//!
//! Recording is manual and never runs in the normal suite:
//!
//! ```text
//! cargo build -p ganbaru-chat-providers --bin ganbaru-provider-replay
//! GANBARU_REPLAY_RECORD=claude:simple \
//!   GANBARU_REPLAY_RECORD_CLI=/absolute/path/to/claude \
//!   GANBARU_REPLAY_RECORD_HOME=/absolute/path/to/provider/home \
//!   cargo test -p ganbaru-chat-providers --lib replay::record -- --ignored
//! ```
//!
//! The transcript lands in the provider's scenario directory. Redaction replaces the workspace,
//! the session from `--session-id`, the provider home (as `<home>`), the user home, email addresses, bearer tokens, and
//! secret-named string fields, and turns inbound echoes of outbound request ids into references.
//! A home path inside a longer outbound string cannot match on replay and needs a hand edit, as
//! does recorded text that happens to look like a placeholder.

use super::runner::{ReplayDirectories, ReplayProvider, ReplaySetup, ReplaySink, run_script};
use super::transcript::{ReplayEntry, ReplayMask, load_transcript, mask_for_provider};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long a recorded step waits for the real CLI, in wall time.
const RECORD_STEP_TIMEOUT: Duration = Duration::from_secs(300);
/// Name of the fake CLI binary the driver spawns in record mode.
const FAKE_CLI_NAME: &str = "ganbaru-provider-replay";
/// Field name fragments whose whole value describes the signed-in account, so every string in it
/// is redacted.
const ACCOUNT_FIELDS: &[&str] = &["account"];
/// Field name fragments whose string values are always redacted.
const SECRET_FIELDS: &[&str] = &[
    "token",
    "secret",
    "password",
    "apikey",
    "api_key",
    "authorization",
    "email",
];
/// The replacement for redacted text.
const REDACTED: &str = "<redacted>";
/// The replacement for the provider home path.
const HOME: &str = "<home>";

/// What redaction replaces in one recording.
#[derive(Debug)]
pub(super) struct RedactionContext {
    /// The canonical workspace path, replaced by `<workspace>`.
    pub(super) workspace: String,
    /// The provider home the recording ran with, replaced by `<home>`.
    pub(super) home: String,
    /// Other private paths such as the user home, replaced by `<redacted>`.
    pub(super) private_paths: Vec<String>,
}

#[test]
#[ignore = "records a real provider CLI; set GANBARU_REPLAY_RECORD=<provider>:<scenario>"]
fn record() {
    let Ok(request) = std::env::var("GANBARU_REPLAY_RECORD") else {
        return;
    };
    match record_scenario(&request) {
        Ok(path) => println!("recorded {}", path.display()),
        Err(error) => panic!("recording {request} failed: {error}"),
    }
}

/// Records `<provider>:<scenario>` and writes the redacted transcript, returning its path.
fn record_scenario(request: &str) -> Result<PathBuf, String> {
    let (provider_name, scenario) = request
        .split_once(':')
        .ok_or("GANBARU_REPLAY_RECORD must be <provider>:<scenario>")?;
    let provider = ReplayProvider::from_name(provider_name)
        .ok_or_else(|| format!("no replay provider named {provider_name}"))?;
    let script = super::scripts::script(provider, scenario)
        .ok_or_else(|| format!("no replay script for {request}"))?;
    let cli = absolute_variable("GANBARU_REPLAY_RECORD_CLI")?;
    let home = absolute_variable("GANBARU_REPLAY_RECORD_HOME")?;
    let fake_cli = fake_cli_path()?;
    let directories = ReplayDirectories::new().map_err(|failure| failure.to_string())?;
    let raw_path = directories.root.join("recording.jsonl");
    let environment = BTreeMap::from([
        (
            "GANBARU_REPLAY_RECORD_TARGET".to_string(),
            path_string(&cli),
        ),
        (
            "GANBARU_REPLAY_RECORD_OUTPUT".to_string(),
            path_string(&raw_path),
        ),
        (
            "GANBARU_REPLAY_RECORD_PROVIDER".to_string(),
            provider_name.to_string(),
        ),
        (
            "GANBARU_REPLAY_RECORD_SCENARIO".to_string(),
            scenario.to_string(),
        ),
        (
            "GANBARU_REPLAY_RECORD_PROTOCOL".to_string(),
            provider.protocol().to_string(),
        ),
    ]);
    let setup = ReplaySetup {
        workspace: &directories.workspace,
        home: &home,
        organizational: script.organizational,
        cli_version: "",
    };
    let driver = provider
        .recording_driver(&setup, &fake_cli, environment)
        .map_err(|error| format!("driver setup: {error:?}"))?;
    let (_finished_sender, finished) = tokio::sync::watch::channel(true);
    let calls = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot start the recording runtime: {error}"))?
        .block_on(run_script(
            driver,
            &script,
            &setup,
            Arc::new(ReplaySink::with_step_timeout(RECORD_STEP_TIMEOUT)),
            finished,
        ))
        .map_err(|failure| failure.to_string())?;
    let raw = std::fs::read_to_string(&raw_path)
        .map_err(|error| format!("the fake CLI wrote no recording ({error}); calls: {calls:?}"))?;
    let private_paths = std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .into_iter()
        .collect();
    let context = RedactionContext {
        workspace: path_string(&directories.workspace),
        home: path_string(&home),
        private_paths,
    };
    let transcript = redact_recording(&raw, &context, &source_revision())?;
    let path = provider
        .scenario_directory()
        .join(format!("{scenario}.jsonl"));
    std::fs::write(&path, transcript)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}

fn absolute_variable(name: &str) -> Result<PathBuf, String> {
    let value = std::env::var_os(name).ok_or_else(|| format!("{name} is not set"))?;
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{name} must be an absolute path"))
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The fake CLI next to this test binary's `deps` directory, where Cargo builds binaries.
fn fake_cli_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate the test binary: {error}"))?;
    let path = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("the test binary has no target directory")?
        .join(format!("{FAKE_CLI_NAME}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "{} is missing; build it with `cargo build -p ganbaru-chat-providers --bin {FAKE_CLI_NAME}`",
            path.display()
        ))
    }
}

/// The short commit of the checkout, or `unknown` outside a Git checkout.
fn source_revision() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|revision| !revision.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Turns a raw recording into a scenario transcript and checks that it loads.
pub(super) fn redact_recording(
    raw: &str,
    context: &RedactionContext,
    revision: &str,
) -> Result<String, String> {
    let mut entries = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str::<ReplayEntry>(line)
                .map_err(|error| format!("recording line {}: {error}", index + 1))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let Some(ReplayEntry::TranscriptStart {
        provider,
        cli_version,
        generated_by,
        launch_arguments,
        ..
    }) = entries.first_mut()
    else {
        return Err("the recording has no header".to_string());
    };
    let mask =
        mask_for_provider(provider).ok_or_else(|| format!("no replay mask for {provider}"))?;
    *generated_by = format!("recorded {cli_version} ganbaru {revision}");
    let session = launch_arguments.as_deref().and_then(session_argument);
    let redactor = Redactor {
        context,
        session: session.as_deref(),
    };
    let mut references: Vec<(Value, String)> = Vec::new();
    for entry in &mut entries {
        match entry {
            ReplayEntry::TranscriptStart {
                launch_arguments, ..
            } => {
                for argument in launch_arguments.iter_mut().flatten() {
                    *argument = redactor.text(argument);
                }
            }
            ReplayEntry::ExpectOutbound { label, frame } => {
                if let Some(id) = frame.pointer(mask.request_id).filter(|id| !id.is_null()) {
                    let name = format!("request-{}", references.len() + 1);
                    references.push((id.clone(), name.clone()));
                    *label = Some(name);
                    mask_request_id(frame, &mask);
                }
                redactor.frame(frame);
            }
            ReplayEntry::EmitInbound { frame, .. } => {
                reference_request_ids(frame, None, &references);
                redactor.frame(frame);
            }
            ReplayEntry::RuntimeExit { stderr, .. } => {
                if let Some(stderr) = stderr {
                    *stderr = redactor.text(stderr);
                }
            }
        }
    }
    let mut transcript = String::new();
    for entry in &entries {
        let line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        transcript.push_str(&line);
        transcript.push('\n');
    }
    load_transcript(&transcript)
        .map_err(|error| format!("the redacted recording does not load: {error}"))?;
    Ok(transcript)
}

/// The value after `--session-id`, when the CLI was launched with one.
fn session_argument(arguments: &[String]) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == "--session-id")
        .and_then(|index| arguments.get(index + 1))
        .filter(|session| !session.is_empty())
        .cloned()
}

/// Replaces a driver-numbered request id with `<redacted>` when the mask compares it by
/// presence only, so the transcript does not pin the driver's numbering.
fn mask_request_id(frame: &mut Value, mask: &ReplayMask) {
    if !mask.presence_only.contains(&mask.request_id) {
        return;
    }
    if let Some(id) = frame.pointer_mut(mask.request_id) {
        *id = Value::String(REDACTED.to_string());
    }
}

/// Replaces inbound echoes of recorded outbound request ids with `<ref:label>`. Strings match
/// anywhere; numbers match only under an `id` or `request_id` key, where they cannot be counts.
fn reference_request_ids(value: &mut Value, key: Option<&str>, references: &[(Value, String)]) {
    let referenced = match value {
        Value::String(_) => true,
        Value::Number(_) => matches!(key, Some("id" | "request_id" | "requestId")),
        _ => false,
    };
    if referenced {
        if let Some((_, label)) = references.iter().find(|(id, _)| id == value) {
            *value = Value::String(format!("<ref:{label}>"));
        }
        return;
    }
    match value {
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| reference_request_ids(item, None, references)),
        Value::Object(fields) => fields
            .iter_mut()
            .for_each(|(key, item)| reference_request_ids(item, Some(key), references)),
        _ => {}
    }
}

/// Applies the text and field redactions of one recording.
struct Redactor<'a> {
    context: &'a RedactionContext,
    session: Option<&'a str>,
}

impl Redactor<'_> {
    fn frame(&self, frame: &mut Value) {
        match frame {
            Value::String(text) => *text = self.text(text),
            Value::Array(items) => items.iter_mut().for_each(|item| self.frame(item)),
            Value::Object(fields) => self.fields(fields),
            _ => {}
        }
    }

    fn fields(&self, fields: &mut Map<String, Value>) {
        for (key, value) in fields.iter_mut() {
            let key = key.to_ascii_lowercase();
            if ACCOUNT_FIELDS.iter().any(|fragment| key.contains(fragment)) {
                redact_strings(value);
                continue;
            }
            let secret = SECRET_FIELDS.iter().any(|fragment| key.contains(fragment));
            match value {
                Value::String(_) if secret => *value = Value::String(REDACTED.to_string()),
                value => self.frame(value),
            }
        }
    }

    /// Replaces the workspace, session, provider home, private paths, bearer tokens, and email
    /// addresses. Longer paths go first, so a provider home inside the user home keeps `<home>`.
    fn text(&self, text: &str) -> String {
        let mut output = text.replace(&self.context.workspace, "<workspace>");
        if let Some(session) = self.session {
            output = output.replace(session, "<session>");
        }
        let mut private_paths: Vec<(&String, &str)> = self
            .context
            .private_paths
            .iter()
            .map(|path| (path, REDACTED))
            .chain(std::iter::once((&self.context.home, HOME)))
            .filter(|(path, _)| !path.is_empty())
            .collect();
        private_paths.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        for (path, replacement) in private_paths {
            output = output.replace(path.as_str(), replacement);
        }
        redact_emails(&redact_bearer_tokens(&output))
    }
}

/// Replaces the credential after each `Bearer ` with `<redacted>`, keeping `${NAME}` references
/// to environment variables, which carry no secret.
fn redact_bearer_tokens(text: &str) -> String {
    const MARKER: &str = "Bearer ";
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(MARKER) {
        let (before, after) = rest.split_at(position + MARKER.len());
        output.push_str(before);
        let end = after
            .find(|character: char| character.is_whitespace() || character == '"')
            .unwrap_or(after.len());
        let credential = &after[..end];
        if credential.is_empty() || credential.starts_with("${") || credential == REDACTED {
            output.push_str(credential);
        } else {
            output.push_str(REDACTED);
        }
        rest = &after[end..];
    }
    output.push_str(rest);
    output
}

/// Replaces each word shaped like `local@domain.tld` with `<redacted>`.
fn redact_emails(text: &str) -> String {
    let is_word = |character: char| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '%' | '+' | '-' | '@')
    };
    let mut output = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, output: &mut String| {
        let address = word.trim_end_matches('.');
        if looks_like_email(address) {
            output.push_str(REDACTED);
            output.push_str(&word[address.len()..]);
        } else {
            output.push_str(word);
        }
        word.clear();
    };
    for character in text.chars() {
        if is_word(character) {
            word.push(character);
        } else {
            flush(&mut word, &mut output);
            output.push(character);
        }
    }
    flush(&mut word, &mut output);
    output
}

/// Replaces every string inside `value` with `<redacted>`, keeping its shape.
fn redact_strings(value: &mut Value) {
    match value {
        Value::String(text) => *text = REDACTED.to_string(),
        Value::Array(items) => items.iter_mut().for_each(redact_strings),
        Value::Object(fields) => fields.values_mut().for_each(redact_strings),
        _ => {}
    }
}

fn looks_like_email(word: &str) -> bool {
    let Some((local, domain)) = word.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain
            .rsplit_once('.')
            .is_some_and(|(name, tld)| !name.is_empty() && tld.len() >= 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn context() -> RedactionContext {
        RedactionContext {
            workspace: "/tmp/ganbaru-replay-1/workspace".to_string(),
            home: "/home/person/.ganbaru-home".to_string(),
            private_paths: vec![
                "/home/person".to_string(),
                "/home/person/.claude".to_string(),
            ],
        }
    }

    fn recording(entries: &[Value]) -> String {
        entries
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn header(arguments: &[&str]) -> Value {
        json!({
            "type": "transcript_start", "provider": "claude", "protocol": "stream-json",
            "cli_version": "2.1.322", "scenario": "simple", "generated_by": "recorded",
            "launch_arguments": arguments,
        })
    }

    fn lines(transcript: &str) -> Vec<Value> {
        transcript
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn redaction_binds_the_workspace_session_and_request_references() {
        let session = "3903fbae-3879-40ce-8bb8-316737ca8bb4";
        let raw = recording(&[
            header(&["-p", "--session-id", session]),
            json!({"type": "expect_outbound", "frame": {
                "type": "control_request", "request_id": "req_1_abc",
                "request": {"subtype": "initialize"}
            }}),
            json!({"type": "emit_inbound", "frame": {
                "type": "control_response",
                "response": {"subtype": "success", "request_id": "req_1_abc"}
            }}),
            json!({"type": "emit_inbound", "frame": {
                "type": "system", "subtype": "init", "session_id": session,
                "cwd": "/tmp/ganbaru-replay-1/workspace/src"
            }}),
        ]);
        let transcript = redact_recording(&raw, &context(), "abc1234").unwrap();
        let lines = lines(&transcript);
        assert_eq!(lines[0]["generated_by"], "recorded 2.1.322 ganbaru abc1234");
        assert_eq!(
            lines[0]["launch_arguments"],
            json!(["-p", "--session-id", "<session>"])
        );
        assert_eq!(lines[1]["label"], "request-1");
        assert_eq!(lines[1]["frame"]["request_id"], "<redacted>");
        assert_eq!(
            lines[2]["frame"]["response"]["request_id"],
            "<ref:request-1>"
        );
        assert_eq!(lines[3]["frame"]["session_id"], "<session>");
        assert_eq!(lines[3]["frame"]["cwd"], "<workspace>/src");
    }

    #[test]
    fn redaction_removes_private_paths_emails_tokens_and_secret_fields() {
        let raw = recording(&[
            header(&[
                "--mcp-config",
                r#"{"Authorization":"Bearer ${GANBARU_CHAT_MCP_TOKEN}"}"#,
            ]),
            json!({"type": "emit_inbound", "frame": {
                "account": {"email": "person@example.com", "organization": "Example"},
                "apiKeySource": "/home/person/.claude/key",
                "note": "Signed in as person@example.com.",
                "header": "Bearer sk-live-123 rest",
                "codexHome": "/home/person/.ganbaru-home/sessions",
                "usage": {"input_tokens": 12}
            }}),
            json!({"type": "runtime_exit", "status": 1, "stderr": "cannot read /home/person/.config\n"}),
        ]);
        let transcript = redact_recording(&raw, &context(), "abc1234").unwrap();
        let lines = lines(&transcript);
        assert_eq!(
            lines[0]["launch_arguments"][1],
            r#"{"Authorization":"Bearer ${GANBARU_CHAT_MCP_TOKEN}"}"#,
            "environment references carry no secret"
        );
        let frame = &lines[1]["frame"];
        assert_eq!(frame["account"]["email"], "<redacted>");
        assert_eq!(
            frame["account"]["organization"], "<redacted>",
            "account labels are private"
        );
        assert_eq!(
            frame["apiKeySource"], "<redacted>",
            "the longest private path wins"
        );
        assert_eq!(
            frame["codexHome"], "<home>/sessions",
            "the provider home inside the user home keeps its placeholder"
        );
        assert_eq!(frame["note"], "Signed in as <redacted>.");
        assert_eq!(frame["header"], "Bearer <redacted> rest");
        assert_eq!(
            frame["usage"]["input_tokens"], 12,
            "token counts are not secrets"
        );
        assert_eq!(lines[2]["stderr"], "cannot read <redacted>/.config\n");
    }

    #[test]
    fn numeric_request_ids_are_referenced_only_under_id_keys() {
        let raw = recording(&[
            header(&[]),
            json!({"type": "expect_outbound", "frame": {"request_id": 1, "method": "x"}}),
            json!({"type": "emit_inbound", "frame": {"id": 1, "result": {"count": 1}}}),
        ]);
        let transcript = redact_recording(&raw, &context(), "abc1234").unwrap();
        let lines = lines(&transcript);
        assert_eq!(lines[2]["frame"]["id"], "<ref:request-1>");
        assert_eq!(lines[2]["frame"]["result"]["count"], 1);
    }

    #[test]
    fn a_recording_that_would_not_load_is_refused() {
        let raw = recording(&[
            header(&[]),
            json!({"type": "emit_inbound", "frame": {"text": "an <unknown> tag"}}),
        ]);
        let error = redact_recording(&raw, &context(), "abc1234").unwrap_err();
        assert!(error.contains("<unknown>"), "{error}");
        assert!(
            redact_recording("", &context(), "abc1234")
                .unwrap_err()
                .contains("no header")
        );
    }

    #[test]
    fn a_redacted_recording_with_an_inbound_session_replays() {
        let session = "3903fbae-3879-40ce-8bb8-316737ca8bb4";
        let stream = |event: Value| {
            json!({"type": "emit_inbound", "frame": {
                "type": "stream_event", "parent_tool_use_id": null, "event": event
            }})
        };
        let raw = recording(&[
            header(&["-p", "--session-id", session]),
            json!({"type": "expect_outbound", "frame": {
                "type": "control_request", "request_id": "req_1_a",
                "request": {"subtype": "initialize", "supportedDialogKinds": []}
            }}),
            json!({"type": "emit_inbound", "frame": {"type": "control_response", "response": {
                "subtype": "success", "request_id": "req_1_a",
                "response": {"commands": [], "models": [], "account": null}
            }}}),
            json!({"type": "expect_outbound", "frame": {
                "type": "control_request", "request_id": "req_2_b",
                "request": {"subtype": "set_permission_mode", "mode": "acceptEdits"}
            }}),
            json!({"type": "emit_inbound", "frame": {"type": "control_response", "response": {
                "subtype": "success", "request_id": "req_2_b", "response": {}
            }}}),
            json!({"type": "expect_outbound", "frame": {
                "type": "user", "parent_tool_use_id": null, "session_id": "",
                "message": {"role": "user", "content": [{"type": "text", "text": "Say hello"}]}
            }}),
            json!({"type": "emit_inbound", "frame": {
                "type": "system", "subtype": "init", "session_id": session,
                "cwd": context().workspace, "model": "claude-sonnet-4-5", "tools": []
            }}),
            stream(json!({"type": "message_start", "message": {
                "id": "msg_hello", "model": "claude-sonnet-4-5",
                "usage": {"input_tokens": 12, "output_tokens": 1}
            }})),
            stream(json!({"type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}})),
            stream(json!({"type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"}})),
            stream(json!({"type": "content_block_stop", "index": 0})),
            json!({"type": "emit_inbound", "frame": {
                "type": "result", "subtype": "success", "is_error": false, "session_id": session,
                "duration_ms": 120, "duration_api_ms": 100, "num_turns": 1, "result": "Hello",
                "stop_reason": "end_turn", "usage": {"input_tokens": 12, "output_tokens": 3}
            }}),
        ]);
        let transcript = redact_recording(&raw, &context(), "abc1234").unwrap();
        assert!(
            transcript.contains("\"session_id\":\"<session>\""),
            "{transcript}"
        );
        let script = crate::replay::scripts::script(ReplayProvider::Claude, "simple").unwrap();
        let report = crate::replay::runner::replay(
            ReplayProvider::Claude,
            load_transcript(&transcript).unwrap(),
            &script,
        );
        assert!(report.is_ok(), "{}", report.unwrap_err());
    }
}
