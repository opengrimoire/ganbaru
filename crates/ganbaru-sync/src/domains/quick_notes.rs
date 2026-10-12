//! Quick notes adapter: the canonical text run codec, the plain text projection, and lifecycle
//! priorities. The app normalizes and projects bodies with these functions, so local writes and
//! merged values agree byte for byte.

use std::fmt;

use ganbaru_sync_contracts::codec::{Reader, Writer};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::quick_notes::{ADAPTER, NOTES_TABLE, note_group};

/// Text runs in one body.
pub const MAX_RUNS: usize = 4_096;
/// Characters in one body.
pub const MAX_BODY_CHARS: usize = 65_536;
/// Characters of body text in a recovery preview.
pub const PREVIEW_CHARS: usize = 240;

const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDERLINE: u8 = 4;
const STYLE_BITS: u8 = BOLD | ITALIC | UNDERLINE;
/// Largest UTF-8 length of one run: every character of a full body in four bytes.
const MAX_RUN_BYTES: usize = MAX_BODY_CHARS * 4;

/// One styled run of note body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    /// Run text, never empty in a canonical list.
    pub content: String,
    /// Bold style.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Underline style.
    pub underline: bool,
}

impl TextRun {
    fn style(&self) -> u8 {
        (u8::from(self.bold) * BOLD)
            | (u8::from(self.italic) * ITALIC)
            | (u8::from(self.underline) * UNDERLINE)
    }

    fn from_style(content: String, style: u8) -> Self {
        Self {
            content,
            bold: style & BOLD != 0,
            italic: style & ITALIC != 0,
            underline: style & UNDERLINE != 0,
        }
    }
}

/// Why a run list cannot become a note body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    /// More than [`MAX_RUNS`] runs were given.
    TooManyRuns,
    /// The text exceeds [`MAX_BODY_CHARS`].
    TooLong,
    /// The text contains a NUL character.
    Nul,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRuns => write!(
                formatter,
                "quick note body cannot contain more than {MAX_RUNS} text runs"
            ),
            Self::TooLong => write!(
                formatter,
                "quick note body cannot exceed {MAX_BODY_CHARS} characters"
            ),
            Self::Nul => formatter.write_str("quick note body cannot contain NUL characters"),
        }
    }
}

impl std::error::Error for RunError {}

/// Canonical form of a run list: empty runs dropped and adjacent runs of equal style merged.
pub fn normalize_runs(runs: &[TextRun]) -> Result<Vec<TextRun>, RunError> {
    if runs.len() > MAX_RUNS {
        return Err(RunError::TooManyRuns);
    }
    let mut output: Vec<TextRun> = Vec::new();
    let mut chars = 0;
    for run in runs.iter().filter(|run| !run.content.is_empty()) {
        if run.content.contains('\0') {
            return Err(RunError::Nul);
        }
        chars += run.content.chars().count();
        if chars > MAX_BODY_CHARS {
            return Err(RunError::TooLong);
        }
        match output.last_mut() {
            Some(previous) if previous.style() == run.style() => {
                previous.content.push_str(&run.content);
            }
            _ => output.push(run.clone()),
        }
    }
    Ok(output)
}

/// Plain text of a body: its runs concatenated.
pub fn plain_text(runs: &[TextRun]) -> String {
    runs.iter().map(|run| run.content.as_str()).collect()
}

/// Encodes a run list: a u32 count, then per run a style byte and u32-length UTF-8 text.
pub fn encode_runs(runs: &[TextRun]) -> Vec<u8> {
    let mut writer =
        Writer::with_capacity(4 + runs.iter().map(|run| 5 + run.content.len()).sum::<usize>());
    writer.u32(runs.len() as u32);
    for run in runs {
        writer.u8(run.style());
        writer.bytes_u32(run.content.as_bytes());
    }
    writer.into_bytes()
}

/// Decodes a canonical run list, refusing any other encoding.
pub fn decode_runs(bytes: &[u8]) -> Result<Vec<TextRun>, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note body");
    let mut reader = Reader::new(bytes);
    let count = reader.u32().map_err(|_| INVALID)? as usize;
    if count > MAX_RUNS {
        return Err(INVALID);
    }
    let mut runs: Vec<TextRun> = Vec::with_capacity(count.min(reader.remaining() / 6));
    let mut chars = 0;
    for _ in 0..count {
        let style = reader.u8().map_err(|_| INVALID)?;
        if style & !STYLE_BITS != 0 {
            return Err(INVALID);
        }
        let text = reader
            .bytes_u32(MAX_RUN_BYTES, "run")
            .map_err(|_| INVALID)?;
        let text = std::str::from_utf8(text).map_err(|_| INVALID)?;
        if text.is_empty() || text.contains('\0') {
            return Err(INVALID);
        }
        chars += text.chars().count();
        if chars > MAX_BODY_CHARS {
            return Err(INVALID);
        }
        if runs
            .last()
            .is_some_and(|previous| previous.style() == style)
        {
            return Err(INVALID);
        }
        runs.push(TextRun::from_style(text.to_owned(), style));
    }
    reader.finish().map_err(|_| INVALID)?;
    Ok(runs)
}

/// The Quick notes adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickNotesAdapter;

fn body_blob(value: &Value) -> Result<&[u8], AdapterError> {
    match value.fields() {
        [Field::Blob(bytes)] => Ok(bytes),
        _ => Err(AdapterError("quick note body")),
    }
}

fn run_from_fields(fields: Vec<Field>) -> Result<TextRun, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note text run");
    let flag = |field: &Field| match field {
        Field::Integer(0) => Ok(false),
        Field::Integer(1) => Ok(true),
        _ => Err(INVALID),
    };
    match fields.as_slice() {
        [Field::Text(_), bold, italic, underline] => {
            let (bold, italic, underline) = (flag(bold)?, flag(italic)?, flag(underline)?);
            let Some(Field::Text(content)) = fields.into_iter().next() else {
                return Err(INVALID);
            };
            Ok(TextRun {
                content,
                bold,
                italic,
                underline,
            })
        }
        _ => Err(INVALID),
    }
}

fn is_body(table: TableId, group: GroupId) -> bool {
    table == NOTES_TABLE && group == note_group::BODY
}

impl DomainAdapter for QuickNotesAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn encode_owned(
        &self,
        table: TableId,
        group: GroupId,
        rows: Vec<Vec<Field>>,
    ) -> Result<Value, AdapterError> {
        if !is_body(table, group) {
            return Err(AdapterError("owned group"));
        }
        let runs = rows
            .into_iter()
            .map(run_from_fields)
            .collect::<Result<Vec<_>, _>>()?;
        Value::new(vec![Field::Blob(encode_runs(&runs))])
            .map_err(|_| AdapterError("quick note body"))
    }

    fn decode_owned(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        if !is_body(table, group) {
            return Err(AdapterError("owned group"));
        }
        Ok(decode_runs(body_blob(value)?)?
            .into_iter()
            .map(|run| {
                vec![
                    Field::Text(run.content),
                    Field::Integer(i64::from(run.bold)),
                    Field::Integer(i64::from(run.italic)),
                    Field::Integer(i64::from(run.underline)),
                ]
            })
            .collect())
    }

    fn derived(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        if !is_body(table, group) {
            return Ok(Vec::new());
        }
        let runs = decode_runs(body_blob(value)?)?;
        Ok(vec![("body_plain_text", Field::Text(plain_text(&runs)))])
    }

    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        if table != NOTES_TABLE || group != note_group::LIFECYCLE {
            return 0;
        }
        match value.fields() {
            [_, _, Field::Text(_)] => 2,
            [_, Field::Integer(1), _] => 1,
            _ => 0,
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        if table != NOTES_TABLE {
            return Ok(());
        }
        if group == note_group::BODY {
            decode_runs(body_blob(value)?)?;
        } else if group == note_group::LIFECYCLE {
            if let [Field::Integer(1), archived, trashed_at] = value.fields() {
                if *archived != Field::Integer(0) || *trashed_at != Field::Null {
                    return Err(AdapterError("quick note lifecycle"));
                }
            }
        }
        Ok(())
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        if table != NOTES_TABLE {
            return presentation;
        }
        for (group, value) in values {
            if *group == note_group::TITLE {
                if let [Field::Text(title)] = value.fields() {
                    presentation.title = title.clone();
                }
            } else if *group == note_group::BODY {
                if let Ok(runs) = body_blob(value).and_then(decode_runs) {
                    presentation.preview = plain_text(&runs).chars().take(PREVIEW_CHARS).collect();
                }
            }
        }
        presentation
    }
}
