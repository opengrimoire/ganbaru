//! Notes about instruction sources a provider loads that the shown files do not cover, such as
//! account-held rules, override files, and home folders teammate runs skip.

use super::{
    ProviderFileFormat, ProviderFileKind, ProviderFileRead, configured_environment, display_path,
};
use ganbaru_chat_contracts::models::ProviderInstanceConfig;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// File ID of Codex's `AGENTS.override.md`, shown only when the file exists.
pub(super) const CODEX_OVERRIDE_FILE_ID: &str = "instructions_override";

/// Instruction filenames Grok recognizes in each home or project folder it scans.
const GROK_INSTRUCTION_NAMES: &[&str] = &[
    "AGENTS.md",
    "Agents.md",
    "AGENT.md",
    "CLAUDE.md",
    "Claude.md",
    "CLAUDE.local.md",
];

/// Variables that stop OpenCode from using `~/.claude/CLAUDE.md` as global instructions.
const OPENCODE_CLAUDE_PROMPT_SWITCHES: &[&str] = &[
    "OPENCODE_DISABLE_CLAUDE_CODE",
    "OPENCODE_DISABLE_CLAUDE_CODE_PROMPT",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFileNoteKind {
    /// Cursor keeps User Rules in the Cursor account, outside any local file.
    CursorUserRules,
    /// An instance with its own Grok folder does not use the user's Grok home.
    GrokPrivateHome,
    /// Grok loads the Markdown files of the rules folder in its home.
    GrokRules,
    /// Grok also loads Claude and Cursor home instructions, which teammates receive as text.
    GrokOtherHomeInstructions,
    /// Codex uses a non-empty `AGENTS.override.md` instead of `AGENTS.md`.
    CodexOverride,
    /// OpenCode uses Claude Code's user `CLAUDE.md` while its global `AGENTS.md` is missing.
    OpencodeClaudeFallback,
    /// OpenCode also loads the files a configuration lists under `instructions`.
    OpencodeInstructions,
    /// Teammates receive the Markdown rules of Claude's user rules folder.
    ClaudeRules,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFileNote {
    pub kind: ProviderFileNoteKind,
    /// Display paths the note names, home-relative when inside the home folder.
    pub paths: Vec<String>,
}

/// Folders the notes of one instance inspect.
pub(super) struct NoteFolders<'a> {
    /// Folder the shown files live in.
    pub home: &'a Path,
    /// The user's own provider home, when the instance uses a private home instead.
    pub replaced_home: Option<PathBuf>,
    /// The user's home folder.
    pub user_home: &'a Path,
}

/// Notes for the files of one instance, in a stable order.
pub(super) fn provider_file_notes(
    configuration: &ProviderInstanceConfig,
    folders: &NoteFolders<'_>,
    files: &[ProviderFileRead],
) -> Vec<ProviderFileNote> {
    let mut notes = Vec::new();
    match configuration.family_id.as_str() {
        "cursor" => notes.push(note(ProviderFileNoteKind::CursorUserRules, Vec::new())),
        "grok" => grok_notes(folders, &mut notes),
        "codex" => {
            // Codex skips an empty override file and reads `AGENTS.md` instead.
            if let Some(file) = files.iter().find(|file| {
                file.file_id == CODEX_OVERRIDE_FILE_ID && !file.contents.trim().is_empty()
            }) {
                notes.push(note(
                    ProviderFileNoteKind::CodexOverride,
                    vec![file.path.clone()],
                ));
            }
        }
        "opencode" => opencode_notes(configuration, folders, files, &mut notes),
        "claude" => {
            let rules = folders.home.join("rules");
            if rules.is_dir() {
                notes.push(note(
                    ProviderFileNoteKind::ClaudeRules,
                    vec![display_path(&rules)],
                ));
            }
        }
        _ => {}
    }
    notes
}

fn grok_notes(folders: &NoteFolders<'_>, notes: &mut Vec<ProviderFileNote>) {
    if let Some(replaced) = &folders.replaced_home {
        notes.push(note(
            ProviderFileNoteKind::GrokPrivateHome,
            vec![display_path(replaced)],
        ));
    }
    let rules = folders.home.join("rules");
    if rules.is_dir() {
        notes.push(note(
            ProviderFileNoteKind::GrokRules,
            vec![display_path(&rules)],
        ));
    }
    let skipped: Vec<String> = [".claude", ".cursor"]
        .iter()
        .flat_map(|vendor| {
            let root = folders.user_home.join(vendor);
            GROK_INSTRUCTION_NAMES
                .iter()
                .map(|name| root.join(name))
                .filter(|path| path.is_file())
                .chain(Some(root.join("rules")).filter(|path| path.is_dir()))
                .collect::<Vec<_>>()
        })
        .map(|path| display_path(&path))
        .collect();
    if !skipped.is_empty() {
        notes.push(note(
            ProviderFileNoteKind::GrokOtherHomeInstructions,
            skipped,
        ));
    }
}

fn opencode_notes(
    configuration: &ProviderInstanceConfig,
    folders: &NoteFolders<'_>,
    files: &[ProviderFileRead],
    notes: &mut Vec<ProviderFileNote>,
) {
    let global_instructions_missing = files
        .iter()
        .any(|file| file.kind == ProviderFileKind::Instructions && !file.exists);
    let claude_prompt = folders.user_home.join(".claude").join("CLAUDE.md");
    let claude_prompt_disabled = OPENCODE_CLAUDE_PROMPT_SWITCHES.iter().any(|name| {
        configured_environment(configuration, name).is_some_and(|value| truthy(&value))
    });
    if global_instructions_missing && !claude_prompt_disabled && claude_prompt.is_file() {
        notes.push(note(
            ProviderFileNoteKind::OpencodeClaudeFallback,
            vec![display_path(&claude_prompt)],
        ));
    }
    let listing: Vec<String> = files
        .iter()
        .filter(|file| file.kind == ProviderFileKind::Configuration && lists_instructions(file))
        .map(|file| file.path.clone())
        .collect();
    if !listing.is_empty() {
        notes.push(note(ProviderFileNoteKind::OpencodeInstructions, listing));
    }
}

/// Whether an OpenCode configuration names instruction files. JSONC files are matched on the
/// key text, since comments keep them from parsing as JSON.
fn lists_instructions(file: &ProviderFileRead) -> bool {
    match file.format {
        ProviderFileFormat::Json => serde_json::from_str::<serde_json::Value>(&file.contents)
            .ok()
            .and_then(|document| document.get("instructions").cloned())
            .is_some_and(|value| value.as_array().is_some_and(|items| !items.is_empty())),
        ProviderFileFormat::Jsonc => file.contents.contains("\"instructions\""),
        ProviderFileFormat::Toml | ProviderFileFormat::Markdown => false,
    }
}

/// OpenCode's reading of a boolean flag variable.
fn truthy(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true")
}

fn note(kind: ProviderFileNoteKind, paths: Vec<String>) -> ProviderFileNote {
    ProviderFileNote { kind, paths }
}
