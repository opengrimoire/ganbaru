//! Replay transcript format, loader, placeholders, and frame matching.
//!
//! The in-process runner and the fake provider CLI share this module, so it depends only on
//! `serde` and `serde_json`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

/// The largest transcript file the loader accepts.
pub const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;
/// The most entries one transcript may hold, `transcript_start` included.
pub const MAX_TRANSCRIPT_ENTRIES: usize = 2_000;

const WORKSPACE_PLACEHOLDER: &str = "workspace";
const SESSION_PLACEHOLDER: &str = "session";
/// The session `<session>` stands for when no launch argument or outbound frame bound one
/// before it is first used, so a provider can announce the session inbound.
pub const REPLAY_SESSION: &str = "00000000-0000-4000-8000-000000000001";
const REDACTED_PLACEHOLDER: &str = "redacted";
const REFERENCE_PLACEHOLDER: &str = "ref";
/// Angle-bracketed words a provider writes itself, kept as literal text: Claude names the model of
/// the messages it generates locally `<synthetic>`.
const PROVIDER_LITERALS: &[&str] = &["synthetic"];

/// One line of a replay transcript.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayEntry {
    /// The header: which CLI and protocol produced the transcript, and how.
    TranscriptStart {
        provider: String,
        protocol: String,
        cli_version: String,
        scenario: String,
        generated_by: String,
        #[serde(default)]
        launch_arguments: Option<Vec<String>>,
    },
    /// A frame the driver must write next.
    ExpectOutbound {
        #[serde(default)]
        label: Option<String>,
        frame: Value,
    },
    /// A frame the provider writes back, optionally after a delay. A JSON string frame is written
    /// as the raw line it holds, which lets a transcript emit malformed output.
    EmitInbound {
        #[serde(default)]
        label: Option<String>,
        frame: Value,
        #[serde(default)]
        after_ms: Option<u64>,
    },
    /// The provider process exits; nothing follows.
    RuntimeExit {
        status: i32,
        #[serde(default)]
        stderr: Option<String>,
    },
}

/// The `transcript_start` header fields.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptHeader {
    pub provider: String,
    pub protocol: String,
    pub cli_version: String,
    pub scenario: String,
    pub generated_by: String,
    pub launch_arguments: Option<Vec<String>>,
}

/// One entry after the header, with its 1-based line number in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptLine {
    pub line: usize,
    pub entry: ReplayEntry,
}

/// A loaded and validated transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub header: TranscriptHeader,
    pub lines: Vec<TranscriptLine>,
}

/// Why a transcript could not be loaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReplayError {
    /// A line is not a valid entry, or entries are out of order.
    Malformed { line: usize, reason: String },
    /// The file or its entry count exceeds the transcript bounds.
    TooLarge,
    /// A string names a placeholder the format does not define, or a reference to a label no
    /// earlier outbound frame carries.
    UnknownPlaceholder { line: usize, placeholder: String },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { line, reason } => {
                write!(formatter, "transcript line {line} is malformed: {reason}")
            }
            Self::TooLarge => formatter.write_str("transcript exceeds the replay bounds"),
            Self::UnknownPlaceholder { line, placeholder } => {
                write!(
                    formatter,
                    "transcript line {line} uses unknown placeholder {placeholder}"
                )
            }
        }
    }
}

impl std::error::Error for ReplayError {}

/// Parses and validates a transcript: a header first, at most one `runtime_exit` and only as
/// the last entry, known placeholders, and references only to earlier outbound labels.
pub fn load_transcript(text: &str) -> Result<Transcript, ReplayError> {
    if text.len() > MAX_TRANSCRIPT_BYTES {
        return Err(ReplayError::TooLarge);
    }
    let mut header = None;
    let mut lines = Vec::new();
    let mut outbound_labels = Vec::<String>::new();
    let mut exited = false;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        if raw.trim().is_empty() {
            continue;
        }
        if lines.len() + usize::from(header.is_some()) >= MAX_TRANSCRIPT_ENTRIES {
            return Err(ReplayError::TooLarge);
        }
        let entry: ReplayEntry =
            serde_json::from_str(raw).map_err(|error| ReplayError::Malformed {
                line,
                reason: error.to_string(),
            })?;
        if exited {
            return Err(malformed(line, "an entry follows runtime_exit"));
        }
        check_placeholders(&entry, line, &outbound_labels)?;
        match entry {
            ReplayEntry::TranscriptStart {
                provider,
                protocol,
                cli_version,
                scenario,
                generated_by,
                launch_arguments,
            } => {
                if header.is_some() {
                    return Err(malformed(line, "a second transcript_start"));
                }
                header = Some(TranscriptHeader {
                    provider,
                    protocol,
                    cli_version,
                    scenario,
                    generated_by,
                    launch_arguments,
                });
            }
            entry => {
                if header.is_none() {
                    return Err(malformed(line, "the first entry must be transcript_start"));
                }
                match &entry {
                    ReplayEntry::ExpectOutbound {
                        label: Some(label), ..
                    } => outbound_labels.push(label.clone()),
                    ReplayEntry::RuntimeExit { .. } => exited = true,
                    _ => {}
                }
                lines.push(TranscriptLine { line, entry });
            }
        }
    }
    let header = header.ok_or_else(|| malformed(1, "the transcript has no transcript_start"))?;
    Ok(Transcript { header, lines })
}

fn malformed(line: usize, reason: &str) -> ReplayError {
    ReplayError::Malformed {
        line,
        reason: reason.to_string(),
    }
}

fn check_placeholders(
    entry: &ReplayEntry,
    line: usize,
    outbound_labels: &[String],
) -> Result<(), ReplayError> {
    let mut strings = Vec::new();
    match entry {
        ReplayEntry::TranscriptStart {
            launch_arguments, ..
        } => strings.extend(launch_arguments.iter().flatten().cloned()),
        ReplayEntry::ExpectOutbound { frame, .. } | ReplayEntry::EmitInbound { frame, .. } => {
            collect_strings(frame, &mut strings)
        }
        ReplayEntry::RuntimeExit { stderr, .. } => strings.extend(stderr.iter().cloned()),
    }
    for text in &strings {
        for token in placeholder_tokens(text) {
            let known = match token.split_once(':') {
                Some((REFERENCE_PLACEHOLDER, label)) => {
                    matches!(
                        entry,
                        ReplayEntry::EmitInbound { .. } | ReplayEntry::ExpectOutbound { .. }
                    ) && outbound_labels.iter().any(|known| known == label)
                }
                Some(_) => false,
                None => {
                    matches!(
                        token,
                        WORKSPACE_PLACEHOLDER | SESSION_PLACEHOLDER | REDACTED_PLACEHOLDER
                    ) || PROVIDER_LITERALS.contains(&token)
                }
            };
            if !known {
                return Err(ReplayError::UnknownPlaceholder {
                    line,
                    placeholder: format!("<{token}>"),
                });
            }
        }
    }
    Ok(())
}

fn collect_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, output)),
        Value::Object(fields) => fields.iter().for_each(|(key, item)| {
            output.push(key.clone());
            collect_strings(item, output);
        }),
        _ => {}
    }
}

/// The placeholder names in `text`: `<name>` or `<name:argument>` where the name is lowercase
/// ASCII letters and underscores and the argument has no whitespace or angle brackets.
fn placeholder_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find(['>', '<']) else {
            break;
        };
        if after.as_bytes()[end] == b'>' && is_placeholder_token(&after[..end]) {
            tokens.push(&after[..end]);
            rest = &after[end + 1..];
        } else {
            rest = after;
        }
    }
    tokens
}

fn is_placeholder_token(token: &str) -> bool {
    let (name, argument) = match token.split_once(':') {
        Some((name, argument)) => (name, Some(argument)),
        None => (token, None),
    };
    let name_ok = !name.is_empty()
        && name.starts_with(|character: char| character.is_ascii_lowercase())
        && name
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_');
    let argument_ok = argument
        .is_none_or(|argument| !argument.is_empty() && !argument.chars().any(char::is_whitespace));
    name_ok && argument_ok
}

/// The JSON pointers of one provider whose values a replay compares by presence only.
///
/// A pointer segment `*` matches any object key or array index.
#[derive(Clone, Copy, Debug)]
pub struct ReplayMask {
    /// Where an outbound request carries its id; `<ref:label>` resolves to that value.
    pub request_id: &'static str,
    /// Pointers compared by presence only, each with the reason its value is volatile.
    pub presence_only: &'static [&'static str],
}

impl ReplayMask {
    fn masks(&self, path: &[String]) -> bool {
        self.presence_only.iter().any(|pointer| {
            let segments: Vec<&str> = pointer.split('/').skip(1).collect();
            segments.len() == path.len()
                && segments
                    .iter()
                    .zip(path)
                    .all(|(segment, actual)| *segment == "*" || segment == actual)
        })
    }
}

/// Values the transcript placeholders resolve to during one replay.
#[derive(Clone, Debug, Default)]
pub struct ReplayBindings {
    pub workspace: String,
    pub session: Option<String>,
    references: BTreeMap<String, Value>,
}

impl ReplayBindings {
    /// Bindings for a replay in `workspace`, with the session taken from `session` when the
    /// caller already knows it (the fake CLI reads it from argv).
    pub fn new(workspace: impl Into<String>, session: Option<String>) -> Self {
        Self {
            workspace: workspace.into(),
            session,
            references: BTreeMap::new(),
        }
    }

    /// Records the request id an outbound frame labeled `label` carries.
    pub fn remember_reference(&mut self, label: &str, frame: &Value, mask: &ReplayMask) {
        if let Some(id) = frame.pointer(mask.request_id) {
            self.references.insert(label.to_string(), id.clone());
        }
    }

    /// Replaces the placeholders of an inbound frame with their bound values.
    pub fn resolve(&self, frame: &Value) -> Result<Value, String> {
        match frame {
            Value::String(text) => self.resolve_string(text),
            Value::Array(items) => items
                .iter()
                .map(|item| self.resolve(item))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(fields) => {
                let mut resolved = Map::new();
                for (key, item) in fields {
                    let key = match self.resolve_string(key)? {
                        Value::String(key) => key,
                        other => other.to_string(),
                    };
                    resolved.insert(key, self.resolve(item)?);
                }
                Ok(Value::Object(resolved))
            }
            other => Ok(other.clone()),
        }
    }

    /// Resolves one string. A string that is exactly one reference takes the referenced value
    /// with its JSON type, so numeric request ids stay numbers.
    pub fn resolve_string(&self, text: &str) -> Result<Value, String> {
        if let Some(label) = whole_reference(text) {
            return self
                .references
                .get(label)
                .cloned()
                .ok_or_else(|| format!("no request id was captured for label {label}"));
        }
        let mut output = String::with_capacity(text.len());
        let mut rest = text;
        for token in placeholder_tokens(text) {
            let marker = format!("<{token}>");
            let Some(position) = rest.find(&marker) else {
                continue;
            };
            output.push_str(&rest[..position]);
            output.push_str(&self.placeholder_value(token)?);
            rest = &rest[position + marker.len()..];
        }
        output.push_str(rest);
        Ok(Value::String(output))
    }

    fn placeholder_value(&self, token: &str) -> Result<String, String> {
        match token.split_once(':') {
            Some((_, label)) => match self.references.get(label) {
                Some(Value::String(text)) => Ok(text.clone()),
                Some(other) => Ok(other.to_string()),
                None => Err(format!("no request id was captured for label {label}")),
            },
            None => match token {
                WORKSPACE_PLACEHOLDER => Ok(self.workspace.clone()),
                SESSION_PLACEHOLDER => Ok(self
                    .session
                    .clone()
                    .unwrap_or_else(|| REPLAY_SESSION.to_string())),
                _ => Ok(format!("<{token}>")),
            },
        }
    }
}

fn whole_reference(text: &str) -> Option<&str> {
    text.strip_prefix("<ref:")?
        .strip_suffix('>')
        .filter(|label| !label.contains(['<', '>']))
}

/// Compares an expected outbound frame with the frame the driver wrote.
///
/// Values at masked pointers are compared by presence only, `<redacted>` matches any string,
/// and the first `<session>` binds the session from the actual frame. Returns the JSON pointer
/// of the first difference.
pub fn match_frame(
    expected: &Value,
    actual: &Value,
    mask: &ReplayMask,
    bindings: &mut ReplayBindings,
) -> Result<(), String> {
    let mut path = Vec::new();
    match_value(expected, actual, mask, bindings, &mut path)
}

fn match_value(
    expected: &Value,
    actual: &Value,
    mask: &ReplayMask,
    bindings: &mut ReplayBindings,
    path: &mut Vec<String>,
) -> Result<(), String> {
    if !path.is_empty() && mask.masks(path) {
        return Ok(());
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for key in actual.keys() {
                if !expected.contains_key(key) {
                    return Err(difference(path, Some(key), "an unexpected field"));
                }
            }
            for (key, expected) in expected {
                let Some(actual) = actual.get(key) else {
                    return Err(difference(path, Some(key), "a missing field"));
                };
                path.push(key.clone());
                match_value(expected, actual, mask, bindings, path)?;
                path.pop();
            }
            Ok(())
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                return Err(difference(path, None, "a different array length"));
            }
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                path.push(index.to_string());
                match_value(expected, actual, mask, bindings, path)?;
                path.pop();
            }
            Ok(())
        }
        (Value::String(expected), Value::String(actual)) => {
            if expected == "<redacted>" {
                return Ok(());
            }
            if expected.contains("<session>") && bindings.session.is_none() {
                if let Some(session) = capture_session(expected, actual) {
                    bindings.session = Some(session);
                }
            }
            match bindings.resolve_string(expected) {
                Ok(resolved) if resolved == Value::String(actual.clone()) => Ok(()),
                Ok(_) => Err(difference(path, None, "a different value")),
                Err(reason) => Err(format!("{}: {reason}", pointer(path))),
            }
        }
        (Value::String(expected), actual) if whole_reference(expected).is_some() => {
            match bindings.resolve_string(expected) {
                Ok(resolved) if &resolved == actual => Ok(()),
                Ok(_) => Err(difference(path, None, "a different value")),
                Err(reason) => Err(format!("{}: {reason}", pointer(path))),
            }
        }
        (expected, actual) if expected == actual => Ok(()),
        _ => Err(difference(path, None, "a different value")),
    }
}

/// The text `<session>` stands for in `actual`, when `expected` holds it once and the rest of
/// `expected` matches literally around it.
fn capture_session(expected: &str, actual: &str) -> Option<String> {
    let (prefix, suffix) = expected.split_once("<session>")?;
    if prefix.contains('<') || suffix.contains('<') {
        return None;
    }
    let middle = actual.strip_prefix(prefix)?.strip_suffix(suffix)?;
    (!middle.is_empty()).then(|| middle.to_string())
}

fn difference(path: &[String], key: Option<&str>, what: &str) -> String {
    let mut location = pointer(path);
    if let Some(key) = key {
        location.push('/');
        location.push_str(key);
    }
    if location.is_empty() {
        location.push('/');
    }
    format!("{location}: {what}")
}

fn pointer(path: &[String]) -> String {
    path.iter().map(|segment| format!("/{segment}")).collect()
}

/// What a replay does next, as a cursor walks the transcript.
#[derive(Clone, Debug, PartialEq)]
pub enum ReplayAction {
    /// Wait for the driver's next frame and compare it with `frame`.
    Expect {
        line: usize,
        label: Option<String>,
        frame: Value,
    },
    /// Write `frame` (already resolved) after `after_ms`.
    Emit {
        line: usize,
        label: Option<String>,
        frame: Value,
        after_ms: u64,
    },
    /// Exit with `status`, writing `stderr` first.
    Exit {
        line: usize,
        status: i32,
        stderr: Option<String>,
    },
    /// Every entry was consumed and the process stays open.
    Done,
}

/// Walks a transcript one entry at a time, resolving placeholders as it goes.
#[derive(Debug)]
pub struct ReplayCursor {
    transcript: Transcript,
    mask: ReplayMask,
    bindings: ReplayBindings,
    next: usize,
}

impl ReplayCursor {
    pub fn new(transcript: Transcript, mask: ReplayMask, bindings: ReplayBindings) -> Self {
        Self {
            transcript,
            mask,
            bindings,
            next: 0,
        }
    }

    /// Advances to the next entry. An emit whose placeholders cannot resolve yet is an error
    /// naming its line.
    pub fn advance(&mut self) -> Result<ReplayAction, (usize, String)> {
        let Some(TranscriptLine { line, entry }) = self.transcript.lines.get(self.next).cloned()
        else {
            return Ok(ReplayAction::Done);
        };
        self.next += 1;
        Ok(match entry {
            ReplayEntry::ExpectOutbound { label, frame } => {
                ReplayAction::Expect { line, label, frame }
            }
            ReplayEntry::EmitInbound {
                label,
                frame,
                after_ms,
            } => ReplayAction::Emit {
                line,
                label,
                frame: self
                    .bindings
                    .resolve(&frame)
                    .map_err(|reason| (line, reason))?,
                after_ms: after_ms.unwrap_or(0),
            },
            ReplayEntry::RuntimeExit { status, stderr } => ReplayAction::Exit {
                line,
                status,
                stderr: match stderr {
                    Some(text) => match self.bindings.resolve_string(&text) {
                        Ok(Value::String(text)) => Some(text),
                        Ok(other) => Some(other.to_string()),
                        Err(reason) => return Err((line, reason)),
                    },
                    None => None,
                },
            },
            ReplayEntry::TranscriptStart { .. } => {
                return Err((line, "a second transcript_start".to_string()));
            }
        })
    }

    /// Compares `actual` with an expected frame and records its request id under `label`.
    pub fn accept(
        &mut self,
        expected: &Value,
        label: Option<&str>,
        actual: &Value,
    ) -> Result<(), String> {
        match_frame(expected, actual, &self.mask, &mut self.bindings)?;
        if let Some(label) = label {
            self.bindings.remember_reference(label, actual, &self.mask);
        }
        Ok(())
    }

    /// The expected frame with every bound placeholder resolved, for failure messages.
    pub fn display_expected(&self, expected: &Value) -> Value {
        self.bindings
            .resolve(expected)
            .unwrap_or_else(|_| expected.clone())
    }
}

/// Claude frames compared by presence only.
pub const CLAUDE_MASK: ReplayMask = ReplayMask {
    request_id: "/request_id",
    presence_only: &[
        // The driver numbers its own control requests.
        "/request_id",
        // Each user message carries a fresh driver-generated UUID.
        "/uuid",
    ],
};

/// Codex app-server frames compared by presence only. Request ids are not masked: the driver
/// numbers its requests from 1, so a reply carrying the wrong id fails the replay.
pub const CODEX_MASK: ReplayMask = ReplayMask {
    request_id: "/id",
    presence_only: &[
        // The client version follows the Ganbaru release.
        "/params/clientInfo/version",
    ],
};

/// Cursor ACP frames compared by presence only. Request ids are not masked, as for Codex.
pub const CURSOR_MASK: ReplayMask = ReplayMask {
    request_id: "/id",
    presence_only: &[
        // The client version follows the Ganbaru release.
        "/params/clientInfo/version",
    ],
};

/// The mask of the provider a transcript header names.
pub fn mask_for_provider(provider: &str) -> Option<ReplayMask> {
    match provider {
        "claude" => Some(CLAUDE_MASK),
        "codex" => Some(CODEX_MASK),
        "cursor" => Some(CURSOR_MASK),
        _ => None,
    }
}
