//! Instruction files a teammate run receives as text.
//!
//! Teammate runs keep provider configuration out of the native process, and Claude, Grok, and
//! OpenCode then skip some of the instruction files a direct run loads: Claude's restricted mode
//! drops every `CLAUDE.md` and rule, OpenCode's disabled project configuration drops the project
//! `AGENTS.md`, and Grok loads project files at startup only in folders the user trusted in
//! Grok. Dispatch reads those files itself, bounded and without leaving the working folder, and
//! appends them to the run's instructions, so the continuation key changes whenever a file
//! does. Codex and Cursor load the same files natively in teammate runs and get nothing here.

use super::common::truncate_utf8;
use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped, require_directories_within};
use ganbaru_chat_contracts::models::{
    MAX_DEVELOPER_INSTRUCTIONS_BYTES, ProjectWorkingFolderId, ProviderInstanceId,
};
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Largest share of the developer instruction cap the files may take, in UTF-8 bytes. Codex
/// applies the same default limit to the project instructions it loads natively.
const MAX_INSTRUCTION_FILES_BYTES: usize = 32 * 1024;

/// Largest file read, in bytes. A larger file is skipped; one that is read but does not fit the
/// remaining budget is cut and marked.
const MAX_INSTRUCTION_FILE_READ_BYTES: u64 = 1024 * 1024;

/// Most files read for one run, so a rules folder with many entries cannot slow dispatch.
const MAX_INSTRUCTION_FILES: usize = 64;

/// Most entries listed in one rules folder.
const MAX_RULES_DIRECTORY_ENTRIES: usize = 256;

/// Deepest rules subfolder read where the provider reads rules recursively.
const MAX_RULES_DEPTH: usize = 4;

/// Room kept for the note that counts the files left out.
const OMISSION_NOTE_RESERVE_BYTES: usize = 128;

/// Smallest part of a file worth sending when it has to be cut.
const MIN_CUT_TEXT_BYTES: usize = 256;

const SEPARATOR: &str = "\n\n";

const INSTRUCTION_FILES_INTRO: &str = "The user keeps the instruction files below for this \
provider. Follow them where they apply to the assignment; they grant no access beyond it.";

const TRUNCATED_MARKER: &str = "\n\n[Cut to fit the instruction limit.]";

/// A provider whose teammate runs need its instruction files sent as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InstructionFileFamily {
    Claude,
    Grok,
    OpenCode,
}

impl InstructionFileFamily {
    /// The family whose instruction files dispatch reads, or `None` for providers that load
    /// them natively in teammate runs (Codex and Cursor) or document no files (generic ACP).
    pub(super) fn from_family_id(family_id: &str) -> Option<Self> {
        match family_id {
            "claude" => Some(Self::Claude),
            "grok" => Some(Self::Grok),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
}

/// Where an instruction file lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InstructionScope {
    /// The provider home, which applies to every folder.
    User,
    /// The working folder the run executes in.
    Project,
}

/// One instruction file's text, labeled by its path relative to where it lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InstructionFile {
    pub(super) scope: InstructionScope,
    pub(super) label: String,
    pub(super) text: String,
}

/// Appends the instruction files of `provider_instance_id` to a run's composed instructions.
///
/// Files come from the provider home where the provider skips them in teammate runs, and from
/// the folder the run executes in when it has a working folder target. A provider, folder, or
/// file that cannot be read adds nothing, because dispatch reports those failures itself when
/// it starts the run.
pub(super) async fn with_instruction_files(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    provider_instance_id: &ProviderInstanceId,
    working_folder_id: Option<&ProjectWorkingFolderId>,
    execution_environment_id: Option<&str>,
    instructions: &str,
) -> String {
    let Ok(provider) = crate::chat::settings_commands::read_provider(app, provider_instance_id)
    else {
        return instructions.to_string();
    };
    let Some(family) =
        InstructionFileFamily::from_family_id(provider.configuration.family_id.as_str())
    else {
        return instructions.to_string();
    };
    let provider_home = match family {
        InstructionFileFamily::Claude => {
            crate::chat::provider_files::provider_home(&provider.configuration).ok()
        }
        InstructionFileFamily::Grok | InstructionFileFamily::OpenCode => None,
    };
    let working_folder = match working_folder_id {
        Some(working_folder_id) => {
            execution_folder(app, pool, working_folder_id, execution_environment_id).await
        }
        None => None,
    };
    let files = tauri::async_runtime::spawn_blocking(move || {
        collect_instruction_files(family, provider_home.as_deref(), working_folder.as_deref())
    })
    .await
    .unwrap_or_default();
    append_instruction_files(instructions, &files)
}

/// The folder a run with a working folder target executes in: the folder itself or its
/// execution environment's worktree.
async fn execution_folder(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    working_folder_id: &ProjectWorkingFolderId,
    execution_environment_id: Option<&str>,
) -> Option<PathBuf> {
    let authorized = crate::chat::workspace::commands::authorize_working_folder(
        app,
        pool,
        working_folder_id,
        ganbaru_chat::workspace::WorkingFolderAuthorizationOperation::ProviderStart,
    )
    .await
    .ok()?;
    crate::chat::execution_environment::resolve_environment_workspace(
        app,
        pool,
        authorized,
        execution_environment_id,
    )
    .await
    .ok()
    .map(|resolved| resolved.canonical_path)
}

/// Reads the instruction files `family` skips in teammate runs, in the order the provider
/// loads them: user files from `provider_home`, then project files from `working_folder`.
///
/// Project files must resolve inside the working folder through real directories; a symlinked
/// file may point at another file in the folder, such as `CLAUDE.md` at `AGENTS.md`. Files
/// with the same text are sent once. Claude rules that name `paths` load only for matching
/// files natively and are left out, and `@` imports are sent as written.
pub(super) fn collect_instruction_files(
    family: InstructionFileFamily,
    provider_home: Option<&Path>,
    working_folder: Option<&Path>,
) -> Vec<InstructionFile> {
    let mut collector = Collector::default();
    if family == InstructionFileFamily::Claude
        && let Some(home) = provider_home
    {
        collector.read_user(home, Path::new("CLAUDE.md"));
        collector.read_rules(&RulesLocation::user(home), Path::new("rules"), true, 0);
    }
    let Some(root) = working_folder.and_then(|folder| fs::canonicalize(folder).ok()) else {
        return collector.files;
    };
    match family {
        InstructionFileFamily::Claude => collector.read_claude_project(&root),
        InstructionFileFamily::Grok => collector.read_grok_project(&root),
        InstructionFileFamily::OpenCode => collector.read_opencode_project(&root),
    }
    collector.files
}

/// Appends `files` to `instructions` within the developer instruction cap, cutting the file
/// that crosses the budget and counting the ones after it.
pub(super) fn append_instruction_files(instructions: &str, files: &[InstructionFile]) -> String {
    let room = MAX_DEVELOPER_INSTRUCTIONS_BYTES
        .saturating_sub(instructions.len())
        .min(MAX_INSTRUCTION_FILES_BYTES);
    let budget = room.saturating_sub(OMISSION_NOTE_RESERVE_BYTES);
    let mut appended = format!("{SEPARATOR}{INSTRUCTION_FILES_INTRO}");
    let mut included = 0;
    for file in files {
        let heading = format!(
            "{SEPARATOR}## {} instructions: {}{SEPARATOR}",
            match file.scope {
                InstructionScope::User => "User",
                InstructionScope::Project => "Project",
            },
            file.label
        );
        let left = budget.saturating_sub(appended.len());
        if heading.len() + file.text.len() <= left {
            appended.push_str(&heading);
            appended.push_str(&file.text);
            included += 1;
            continue;
        }
        let text_room = left.saturating_sub(heading.len() + TRUNCATED_MARKER.len());
        if text_room >= MIN_CUT_TEXT_BYTES {
            appended.push_str(&heading);
            appended.push_str(truncate_utf8(&file.text, text_room));
            appended.push_str(TRUNCATED_MARKER);
            included += 1;
        }
        break;
    }
    if included == 0 {
        return instructions.to_string();
    }
    let omitted = files.len() - included;
    if omitted > 0 {
        appended.push_str(&format!(
            "{SEPARATOR}[{omitted} more instruction files were left out to fit the instruction limit.]"
        ));
    }
    format!("{instructions}{appended}")
}

#[derive(Default)]
struct Collector {
    files: Vec<InstructionFile>,
    seen_texts: HashSet<String>,
    reads: usize,
}

/// A folder rules are read from, with the boundary its files must stay in.
struct RulesLocation<'a> {
    base: &'a Path,
    scope: InstructionScope,
}

impl<'a> RulesLocation<'a> {
    fn user(base: &'a Path) -> Self {
        Self {
            base,
            scope: InstructionScope::User,
        }
    }

    fn project(base: &'a Path) -> Self {
        Self {
            base,
            scope: InstructionScope::Project,
        }
    }
}

impl Collector {
    /// Project files Claude Code loads at launch from the working folder: the three `CLAUDE`
    /// files, `AGENTS.md` only when none of them exists, and `.claude/rules`.
    fn read_claude_project(&mut self, root: &Path) {
        let named = ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"];
        let any_named = named.iter().any(|name| exists(root, name));
        for name in named {
            self.read_project(root, Path::new(name));
        }
        if !any_named {
            self.read_project(root, Path::new("AGENTS.md"));
        }
        self.read_rules(
            &RulesLocation::project(root),
            Path::new(".claude/rules"),
            true,
            0,
        );
    }

    /// Project files Grok loads from the working folder in a trusted folder: every supported
    /// name in its order, Claude's project memory files, and the top level of each rules folder.
    fn read_grok_project(&mut self, root: &Path) {
        for name in [
            "Agents.md",
            "Claude.md",
            "CLAUDE.md",
            "CLAUDE.local.md",
            "AGENT.md",
            "AGENTS.md",
            ".claude/CLAUDE.md",
            ".claude/CLAUDE.local.md",
        ] {
            self.read_project(root, Path::new(name));
        }
        for rules in [".grok/rules", ".claude/rules", ".cursor/rules"] {
            self.read_rules(&RulesLocation::project(root), Path::new(rules), false, 0);
        }
    }

    /// The project file OpenCode loads: the first of `AGENTS.md`, `CLAUDE.md`, and
    /// `CONTEXT.md` that exists.
    fn read_opencode_project(&mut self, root: &Path) {
        if let Some(name) = ["AGENTS.md", "CLAUDE.md", "CONTEXT.md"]
            .into_iter()
            .find(|name| exists(root, name))
        {
            self.read_project(root, Path::new(name));
        }
    }

    fn read_user(&mut self, home: &Path, relative: &Path) {
        if self.reads >= MAX_INSTRUCTION_FILES {
            return;
        }
        // The provider home is the user's own configuration, which the provider itself reads
        // through symlinks, so a file linked from elsewhere loads as it does natively.
        let Ok(bytes) = read_regular_file_capped(
            &home.join(relative),
            MAX_INSTRUCTION_FILE_READ_BYTES,
            FinalSymlink::Follow,
        ) else {
            return;
        };
        self.reads += 1;
        self.push(InstructionScope::User, relative, &bytes, false);
    }

    fn read_project(&mut self, root: &Path, relative: &Path) {
        if self.reads >= MAX_INSTRUCTION_FILES {
            return;
        }
        let Some(bytes) = read_project_file(root, relative) else {
            return;
        };
        self.reads += 1;
        self.push(InstructionScope::Project, relative, &bytes, false);
    }

    /// Reads the Markdown files of a rules folder in name order, descending into subfolders
    /// when `recursive`. A rule that names `paths` is left out.
    fn read_rules(
        &mut self,
        location: &RulesLocation<'_>,
        relative: &Path,
        recursive: bool,
        depth: usize,
    ) {
        let directory = location.base.join(relative);
        if location.scope == InstructionScope::Project
            && require_directories_within(location.base, &directory).is_err()
        {
            return;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            return;
        };
        let mut names: Vec<_> = entries
            .filter_map(Result::ok)
            .take(MAX_RULES_DIRECTORY_ENTRIES)
            .map(|entry| entry.file_name())
            .collect();
        names.sort();
        for name in names {
            if self.reads >= MAX_INSTRUCTION_FILES {
                return;
            }
            let child = relative.join(&name);
            let path = location.base.join(&child);
            // Project rules never follow a symlinked folder; user rules follow links as the
            // provider does.
            let metadata = match location.scope {
                InstructionScope::Project => fs::symlink_metadata(&path),
                InstructionScope::User => fs::metadata(&path),
            };
            let Ok(metadata) = metadata else {
                continue;
            };
            if metadata.is_dir() {
                if recursive && depth + 1 < MAX_RULES_DEPTH {
                    self.read_rules(location, &child, recursive, depth + 1);
                }
                continue;
            }
            let is_markdown = Path::new(&name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
            if !is_markdown {
                continue;
            }
            let bytes = match location.scope {
                InstructionScope::Project => read_project_file(location.base, &child),
                InstructionScope::User => read_regular_file_capped(
                    &path,
                    MAX_INSTRUCTION_FILE_READ_BYTES,
                    FinalSymlink::Follow,
                )
                .ok(),
            };
            let Some(bytes) = bytes else {
                continue;
            };
            self.reads += 1;
            self.push(location.scope, &child, &bytes, true);
        }
    }

    fn push(&mut self, scope: InstructionScope, relative: &Path, bytes: &[u8], rule: bool) {
        let text = String::from_utf8_lossy(bytes).trim().to_string();
        if text.is_empty() || (rule && names_paths(&text)) || !self.seen_texts.insert(text.clone())
        {
            return;
        }
        self.files.push(InstructionFile {
            scope,
            label: label(relative),
            text,
        });
    }
}

fn exists(root: &Path, relative: &str) -> bool {
    fs::symlink_metadata(root.join(relative)).is_ok()
}

/// Reads `relative` below `root` when every folder on the way is a real folder and the file,
/// or the file a final symlink names, is inside `root`.
fn read_project_file(root: &Path, relative: &Path) -> Option<Vec<u8>> {
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).ok()?;
    require_directories_within(root, path.parent()?).ok()?;
    let target = if metadata.file_type().is_symlink() {
        let target = fs::canonicalize(&path).ok()?;
        if !target.starts_with(root) {
            return None;
        }
        target
    } else {
        path
    };
    read_regular_file_capped(
        &target,
        MAX_INSTRUCTION_FILE_READ_BYTES,
        FinalSymlink::Refuse,
    )
    .ok()
}

/// Whether a rule's front matter limits it to matching files with a `paths` field.
fn names_paths(text: &str) -> bool {
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    lines
        .take_while(|line| line.trim_end() != "---")
        .any(|line| line.starts_with("paths:"))
}

/// A path label with forward slashes on every platform.
fn label(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests;
