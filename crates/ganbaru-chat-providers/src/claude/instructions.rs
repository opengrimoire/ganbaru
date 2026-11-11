//! Session instructions in the Claude system prompt.
//!
//! Teammate instructions join the system prompt once, when the process starts, through
//! `--append-system-prompt` or, above the inline bound, `--append-system-prompt-file`. They
//! never travel in user message text. Ganbaru never sends `appendSystemPrompt` in
//! `initialize`, because the CLI replaces launch-argument append text with it.

use super::protocol::{ClaudeSystemPromptArgument, MAX_INLINE_SYSTEM_PROMPT_BYTES};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use crate::probe_directory::ProbeDirectory;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;

/// Largest instructions a Claude session accepts, matching the teammate instructions bound.
pub const MAX_DEVELOPER_INSTRUCTIONS_BYTES: usize = 64 * 1024;

/// Name of the owner-only file that carries instructions above the inline bound.
const SYSTEM_PROMPT_FILE_NAME: &str = "instructions.md";

/// Returns the instructions a session or turn carries, or none when they are absent or blank.
///
/// # Errors
///
/// Returns a `developerInstructions` validation error when the instructions exceed
/// [`MAX_DEVELOPER_INSTRUCTIONS_BYTES`] or contain a NUL byte.
pub fn session_instructions(value: Option<&str>) -> ChatResult<Option<&str>> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    if value.len() > MAX_DEVELOPER_INSTRUCTIONS_BYTES || value.contains('\0') {
        return Err(ChatError::validation(
            "developerInstructions",
            "Claude developer instructions exceed the supported limit",
        ));
    }
    Ok(Some(value))
}

/// The SHA-256 digest that identifies one instruction set within a session.
pub fn instructions_digest(instructions: &str) -> [u8; 32] {
    Sha256::digest(instructions.as_bytes()).into()
}

/// The system prompt source of a running session and the digest of its instructions.
///
/// A file source keeps its private directory alive for as long as the session holds it, and
/// removes it when dropped.
#[derive(Debug)]
pub struct ClaudeSessionInstructions {
    digest: [u8; 32],
    source: ClaudeSystemPromptSource,
}

#[derive(Debug)]
enum ClaudeSystemPromptSource {
    Inline(String),
    File {
        _directory: ProbeDirectory,
        path: PathBuf,
    },
}

impl ClaudeSessionInstructions {
    /// Prepares validated instructions for launch: inline at or under
    /// [`MAX_INLINE_SYSTEM_PROMPT_BYTES`], otherwise in an owner-only file, because Windows
    /// limits a command line to 32,767 characters.
    ///
    /// # Errors
    ///
    /// Returns a configuration error when the private directory or file cannot be written.
    pub fn prepare(instructions: &str) -> ChatResult<Self> {
        let digest = instructions_digest(instructions);
        if instructions.len() <= MAX_INLINE_SYSTEM_PROMPT_BYTES {
            return Ok(Self {
                digest,
                source: ClaudeSystemPromptSource::Inline(instructions.to_string()),
            });
        }
        let directory = ProbeDirectory::create()?;
        let path = directory.join(SYSTEM_PROMPT_FILE_NAME);
        write_instructions_file(&path, instructions)?;
        Ok(Self {
            digest,
            source: ClaudeSystemPromptSource::File {
                _directory: directory,
                path,
            },
        })
    }

    /// The launch argument that places the instructions in the system prompt.
    pub fn argument(&self) -> ClaudeSystemPromptArgument<'_> {
        match &self.source {
            ClaudeSystemPromptSource::Inline(text) => ClaudeSystemPromptArgument::Inline(text),
            ClaudeSystemPromptSource::File { path, .. } => ClaudeSystemPromptArgument::File(path),
        }
    }

    /// The digest of the instructions the session started with.
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

/// Checks a turn's instructions against the ones the session started with.
///
/// A turn without instructions is accepted, because the system prompt still holds the
/// session's own. Matching instructions are accepted. Different instructions, or any
/// instructions on a session that started without them, conflict: a system prompt cannot
/// change in a running session, so two instruction sets never mix in one session. The
/// conflict is retryable so the caller can move to a fresh session.
///
/// # Errors
///
/// Returns a validation error for invalid turn instructions and a retryable `Conflict` for
/// instructions that differ from the session's.
pub fn check_turn_instructions(
    session_digest: Option<[u8; 32]>,
    turn_instructions: Option<&str>,
) -> ChatResult<()> {
    let Some(turn_instructions) = session_instructions(turn_instructions)? else {
        return Ok(());
    };
    if session_digest == Some(instructions_digest(turn_instructions)) {
        return Ok(());
    }
    Err(ChatError::new(
        ChatErrorCode::Conflict,
        "Claude instructions changed for this session",
        true,
    ))
}

fn write_instructions_file(path: &std::path::Path, instructions: &str) -> ChatResult<()> {
    let written = ganbaru_atomic_files::create_private_file(path).and_then(|mut file| {
        file.write_all(instructions.as_bytes())?;
        file.sync_all()
    });
    written.map_err(|_| {
        ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Claude instructions file is unavailable",
            true,
        )
    })
}
