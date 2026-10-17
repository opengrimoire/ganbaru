//! Provider prompt, command, and skill catalog discovery.

use super::super::models::{
    ChatError, ChatErrorCode, ChatPromptCatalogEntry, ChatResult, ProviderInstanceConfig,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_DISCOVERED_ENTRIES: usize = 500;
const MAX_SCANNED_ENTRIES: usize = 2_000;
const MAX_METADATA_BYTES: u64 = 16 * 1024;

/// Reads provider-owned and workspace-owned prompt resources from disk.
pub fn read_static_prompt_catalog(
    workspace: &Path,
    configuration: &ProviderInstanceConfig,
    stale: bool,
) -> Vec<ChatPromptCatalogEntry> {
    let family = configuration.family_id.as_str();
    let mut entries = provider_command_fallbacks(family, stale);
    entries.extend(read_workspace_command_entries(workspace, family, stale));
    if let Some(home) = provider_skill_home(configuration) {
        if family == "codex" {
            entries.extend(read_skill_entries(&home, stale).unwrap_or_default());
        }
        entries.extend(read_user_command_entries(&home, family, stale));
    }
    entries
}

/// Resolves duplicate catalog entries using the same source precedence as providers.
pub fn merge_prompt_entries(entries: Vec<ChatPromptCatalogEntry>) -> Vec<ChatPromptCatalogEntry> {
    let mut merged = BTreeMap::new();
    for entry in entries {
        let key = format!("{}:{}", entry.kind, entry.value.to_ascii_lowercase());
        let replace = merged
            .get(&key)
            .is_none_or(|existing: &ChatPromptCatalogEntry| {
                prompt_source_rank(&entry.source) <= prompt_source_rank(&existing.source)
                    || (existing.stale && !entry.stale)
            });
        if replace {
            merged.insert(key, entry);
        }
    }
    let mut output = merged.into_values().collect::<Vec<_>>();
    output.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| prompt_source_rank(&left.source).cmp(&prompt_source_rank(&right.source)))
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });
    output
}

fn read_skill_entries(home: &Path, stale: bool) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
    let root = home.join("skills");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let mut pending = vec![(root, 0_u8)];
    let mut seen = BTreeSet::new();
    let mut scanned = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if scanned >= MAX_SCANNED_ENTRIES || entries.len() >= MAX_DISCOVERED_ENTRIES {
            break;
        }
        for child in fs::read_dir(directory)
            .map_err(catalog_io_error)?
            .take(MAX_SCANNED_ENTRIES.saturating_sub(scanned))
        {
            scanned += 1;
            let child = child.map_err(catalog_io_error)?;
            let file_type = child.file_type().map_err(catalog_io_error)?;
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            let skill_file = child.path().join("SKILL.md");
            if skill_file.is_file() {
                let name = child.file_name().to_string_lossy().into_owned();
                if name.is_empty()
                    || name.len() > 200
                    || name.chars().any(char::is_control)
                    || !seen.insert(name.clone())
                {
                    continue;
                }
                entries.push(ChatPromptCatalogEntry {
                    value: format!("${name}"),
                    label: name,
                    description: read_skill_description(&skill_file),
                    argument_hint: None,
                    kind: "skill".to_string(),
                    source: "user".to_string(),
                    stale,
                });
            } else if depth < 3 && entries.len() < MAX_DISCOVERED_ENTRIES {
                pending.push((child.path(), depth + 1));
            }
        }
    }
    entries.sort_by(|left, right| left.label.cmp(&right.label));
    Ok(entries)
}

fn read_skill_description(path: &Path) -> Option<String> {
    let text = read_metadata_prefix(path)?;
    text.lines()
        .take(80)
        .find_map(|line| line.trim().strip_prefix("description:"))
        .map(str::trim)
        .map(|value| value.trim_matches(['\'', '"']))
        .filter(|value| !value.is_empty() && value.len() <= 1_000)
        .map(str::to_string)
}

fn provider_skill_home(configuration: &ProviderInstanceConfig) -> Option<PathBuf> {
    if let Some(home) = configuration
        .provider_home
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(home));
    }
    match configuration.family_id.as_str() {
        "codex" => {
            if let Some(home) = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty()) {
                return Some(PathBuf::from(home));
            }
        }
        "claude" => {
            if let Some(home) =
                std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty())
            {
                return Some(PathBuf::from(home));
            }
        }
        "opencode" => {
            if let Some(home) = std::env::var_os("XDG_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
            {
                return Some(home.join("opencode"));
            }
        }
        _ => return None,
    }
    #[cfg(windows)]
    let home_names = ["USERPROFILE", "HOME"];
    #[cfg(not(windows))]
    let home_names = ["HOME", "USERPROFILE"];
    home_names
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .map(PathBuf::from)
        .map(|home| match configuration.family_id.as_str() {
            "codex" => home.join(".codex"),
            "claude" => home.join(".claude"),
            "opencode" => home.join(".config").join("opencode"),
            _ => home,
        })
}

fn provider_command_fallbacks(family: &str, stale: bool) -> Vec<ChatPromptCatalogEntry> {
    let commands: &[(&str, &str, &str, Option<&str>)] = match family {
        "codex" => &[
            (
                "/compact",
                "Compact",
                "Compact the active Codex context",
                None,
            ),
            (
                "/goal",
                "Goal",
                "Show, set, pause, resume, or clear the thread goal",
                Some("[objective | pause | resume | clear]"),
            ),
            (
                "/mcp",
                "MCP",
                "Show configured MCP servers and their status",
                None,
            ),
            (
                "/review",
                "Code review",
                "Start a provider-native code review",
                Some("[instructions]"),
            ),
        ],
        "claude" => &[
            (
                "/clear",
                "Clear",
                "Clear conversation history and free context",
                None,
            ),
            (
                "/compact",
                "Compact",
                "Compact conversation history while preserving important context",
                Some("[focus instructions]"),
            ),
            (
                "/context",
                "Context",
                "Show how Claude is using the context window",
                None,
            ),
            (
                "/usage",
                "Usage",
                "Show plan usage limits and rate limit status",
                None,
            ),
        ],
        _ => &[],
    };
    commands
        .iter()
        .map(
            |(value, label, description, argument_hint)| ChatPromptCatalogEntry {
                value: (*value).to_string(),
                label: (*label).to_string(),
                description: Some((*description).to_string()),
                argument_hint: argument_hint.map(|value| value.to_string()),
                kind: "command".to_string(),
                source: "provider".to_string(),
                stale,
            },
        )
        .collect()
}

fn read_workspace_command_entries(
    workspace: &Path,
    family: &str,
    stale: bool,
) -> Vec<ChatPromptCatalogEntry> {
    let relative_roots: &[(&str, bool)] = match family {
        "claude" => &[(".claude/commands", true)],
        "cursor" => &[(".cursor/commands", false)],
        "opencode" => &[(".opencode/commands", false), (".opencode/command", false)],
        _ => &[],
    };
    let mut entries = Vec::new();
    for (relative_root, nested) in relative_roots {
        if let Ok(found) =
            read_command_directory(&workspace.join(relative_root), "workspace", stale, *nested)
        {
            entries.extend(found);
        }
    }
    entries
}

fn read_user_command_entries(
    home: &Path,
    family: &str,
    stale: bool,
) -> Vec<ChatPromptCatalogEntry> {
    let roots: &[(&str, bool)] = match family {
        "claude" => &[("commands", true)],
        "opencode" => &[("commands", false), ("command", false)],
        _ => &[],
    };
    let mut entries = Vec::new();
    for (root, nested) in roots {
        if let Ok(found) = read_command_directory(&home.join(root), "user", stale, *nested) {
            entries.extend(found);
        }
    }
    entries
}

fn read_command_directory(
    root: &Path,
    source: &str,
    stale: bool,
    nested: bool,
) -> ChatResult<Vec<ChatPromptCatalogEntry>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0_u8)];
    let mut scanned = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if scanned >= MAX_SCANNED_ENTRIES || entries.len() >= MAX_DISCOVERED_ENTRIES {
            break;
        }
        for child in fs::read_dir(&directory)
            .map_err(catalog_io_error)?
            .take(MAX_SCANNED_ENTRIES.saturating_sub(scanned))
        {
            scanned += 1;
            let child = child.map_err(catalog_io_error)?;
            let file_type = child.file_type().map_err(catalog_io_error)?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() && nested && depth < 4 {
                pending.push((child.path(), depth + 1));
                continue;
            }
            if !file_type.is_file()
                || child.path().extension().and_then(|value| value.to_str()) != Some("md")
            {
                continue;
            }
            let child_path = child.path();
            let relative = child_path.strip_prefix(root).map_err(catalog_io_error)?;
            let mut segments = relative
                .components()
                .filter_map(|component| match component {
                    Component::Normal(value) => value.to_str(),
                    _ => None,
                })
                .map(str::to_string)
                .collect::<Vec<_>>();
            let Some(last) = segments.last_mut() else {
                continue;
            };
            *last = last.trim_end_matches(".md").to_string();
            let name = if nested {
                segments.join(":")
            } else {
                segments.last().cloned().unwrap_or_default()
            };
            if name.is_empty()
                || name.len() > 200
                || name.chars().any(char::is_control)
                || name.chars().any(char::is_whitespace)
            {
                continue;
            }
            let metadata = read_command_metadata(&child_path);
            entries.push(ChatPromptCatalogEntry {
                value: format!("/{name}"),
                label: name,
                description: metadata.0,
                argument_hint: metadata.1,
                kind: "command".to_string(),
                source: source.to_string(),
                stale,
            });
        }
    }
    Ok(entries)
}

fn read_command_metadata(path: &Path) -> (Option<String>, Option<String>) {
    let Some(text) = read_metadata_prefix(path) else {
        return (None, None);
    };
    let field = |names: &[&str], maximum: usize| {
        text.lines().take(80).find_map(|line| {
            let line = line.trim();
            names.iter().find_map(|name| {
                line.strip_prefix(name)
                    .map(str::trim)
                    .map(|value| value.trim_matches(['\'', '"']))
                    .filter(|value| {
                        !value.is_empty()
                            && value.len() <= maximum
                            && !value.chars().any(char::is_control)
                    })
                    .map(str::to_string)
            })
        })
    };
    let argument_hint = field(&["argument-hint:", "argument_hint:"], 500)
        .or_else(|| command_template_uses_arguments(&text).then(|| "[arguments]".to_string()));
    (field(&["description:"], 1_000), argument_hint)
}

fn read_metadata_prefix(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_METADATA_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    String::from_utf8(bytes).ok()
}

fn command_template_uses_arguments(template: &str) -> bool {
    template.contains("$ARGUMENTS")
        || template
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'$' && matches!(pair[1], b'1'..=b'9'))
}

fn prompt_source_rank(source: &str) -> u8 {
    match source {
        "provider" => 0,
        "workspace" => 1,
        "user" => 2,
        _ => 3,
    }
}

fn catalog_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Provider prompt resources could not be read",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ganbaru-chat-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn provider_skill_catalog_reads_nested_bounded_metadata() {
        let root = temporary_root("skill");
        let skill = root.join("skills/.system/example");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: example\ndescription: A provider-scoped example\n---\n",
        )
        .unwrap();
        let entries = read_skill_entries(&root, false).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, "$example");
        assert_eq!(
            entries[0].description.as_deref(),
            Some("A provider-scoped example")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workspace_command_catalog_reads_namespaces_metadata_and_source() {
        let root = temporary_root("command");
        let command = root.join("review/security.md");
        fs::create_dir_all(command.parent().unwrap()).unwrap();
        fs::write(
            &command,
            "---\ndescription: Review security boundaries\nargument-hint: [scope]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("release.md"),
            "Prepare a release for $ARGUMENTS\n",
        )
        .unwrap();
        let entries = read_command_directory(&root, "workspace", false, true).unwrap();
        assert_eq!(entries.len(), 2);
        let review = entries
            .iter()
            .find(|entry| entry.value == "/review:security")
            .unwrap();
        assert_eq!(review.source, "workspace");
        assert_eq!(review.argument_hint.as_deref(), Some("[scope]"));
        let release = entries
            .iter()
            .find(|entry| entry.value == "/release")
            .unwrap();
        assert_eq!(release.argument_hint.as_deref(), Some("[arguments]"));
        fs::remove_dir_all(root).unwrap();
    }
}
