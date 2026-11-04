//! Repository metadata and secret paths that workspace tools never read, list,
//! mention, or write.
//!
//! Matching is per path component and follows how Windows and macOS resolve
//! names: ASCII case is folded, trailing dots and spaces are ignored, an NTFS
//! alternate data stream suffix names its base file, 8.3 short names alias the
//! long name, and code points HFS+ ignores are dropped. Git applies the same
//! rules in `is_ntfs_dotgit` and `is_hfs_dotgit`.

/// Directory names protected at any depth, including nested repositories and
/// worktree gitfiles.
const PROTECTED_AT_ANY_DEPTH: &[&str] = &[".git"];

/// Top-level directories that hold credentials.
const PROTECTED_TOP_LEVEL_DIRECTORIES: &[&str] = &[".ssh", ".gnupg", ".aws"];

/// Exact provider credential files below the folder root.
const PROTECTED_FILES: &[&[&str]] = &[&[".codex", "auth.json"], &[".claude", ".credentials.json"]];

/// Top-level environment file and the one variant meant for committing.
const ENVIRONMENT_FILE: &str = ".env";
const ENVIRONMENT_EXAMPLE_FILE: &str = ".env.example";

/// Length of the long-name prefix Windows keeps in an 8.3 short name.
const SHORT_NAME_PREFIX_CHARACTERS: usize = 6;
/// Long-name characters kept before the hash in a hashed 8.3 short name.
const HASHED_SHORT_NAME_PREFIX_CHARACTERS: usize = 2;
/// Hexadecimal hash characters in a hashed 8.3 short name.
const HASHED_SHORT_NAME_HASH_CHARACTERS: usize = 4;

/// Reports whether a slash-separated relative path names protected repository
/// metadata or credentials.
pub fn path_is_protected(relative_path: &str) -> bool {
    let components = relative_path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(normalize_component)
        .collect::<Vec<_>>();
    let Some(first) = components.first() else {
        return false;
    };
    components.iter().any(|component| {
        PROTECTED_AT_ANY_DEPTH
            .iter()
            .any(|name| component_matches(component, name))
    }) || PROTECTED_TOP_LEVEL_DIRECTORIES
        .iter()
        .any(|name| component_matches(first, name))
        || (components.len() == 1 && environment_file_is_protected(first))
        || PROTECTED_FILES.iter().any(|file| {
            file.len() == components.len()
                && file
                    .iter()
                    .zip(&components)
                    .all(|(name, component)| component_matches(component, name))
        })
}

/// Reports whether a name or path contains control characters, which would
/// break the line structure of provider prompts that quote it.
pub fn text_has_control_characters(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// Reports whether one path component names a Git directory or gitfile.
pub fn component_is_git_metadata(component: &str) -> bool {
    component_matches(&normalize_component(component), ".git")
}

/// Reports whether a component cannot name an ordinary file on Windows: it
/// contains a stream separator or names a reserved device.
pub fn component_is_windows_reserved(component: &str) -> bool {
    if component.contains(':') {
        return true;
    }
    let trimmed = component.trim_end_matches([' ', '.']);
    let stem = trimmed
        .split_once('.')
        .map_or(trimmed, |(stem, _)| stem)
        .trim_end_matches(' ')
        .to_ascii_lowercase();
    matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) || ["com", "lpt"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
                || matches!(suffix, "\u{b9}" | "\u{b2}" | "\u{b3}")
        })
    })
}

fn environment_file_is_protected(component: &str) -> bool {
    if component == ENVIRONMENT_EXAMPLE_FILE {
        return false;
    }
    let dotless = &ENVIRONMENT_FILE[1..];
    component == ENVIRONMENT_FILE
        || component
            .strip_prefix(ENVIRONMENT_FILE)
            .is_some_and(|rest| rest.starts_with('.'))
        || short_name_aliases(component, dotless)
}

/// Folds a component the way case-insensitive Windows and macOS lookups do.
fn normalize_component(component: &str) -> String {
    let without_ignorable = component
        .chars()
        .filter(|character| !hfs_ignorable(*character))
        .collect::<String>();
    let base = without_ignorable
        .split_once(':')
        .map_or(without_ignorable.as_str(), |(base, _)| base);
    base.trim_end_matches(['.', ' ']).to_ascii_lowercase()
}

/// Code points HFS+ drops when comparing names, as listed by Git.
fn hfs_ignorable(character: char) -> bool {
    matches!(
        character,
        '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
    )
}

fn component_matches(normalized: &str, name: &str) -> bool {
    normalized == name || short_name_aliases(normalized, name.trim_start_matches('.'))
}

/// Reports whether a normalized component can be an 8.3 short name of a long
/// name whose leading dots are removed, such as `git~1` or `gi1a2b~1` for
/// `.git`.
fn short_name_aliases(normalized: &str, dotless: &str) -> bool {
    let Some((stem, tail)) = normalized.split_once('~') else {
        return false;
    };
    let ordinal = tail.split_once('.').map_or(tail, |(ordinal, _)| ordinal);
    let ordinal_is_valid = !ordinal.is_empty()
        && !ordinal.starts_with('0')
        && ordinal.bytes().all(|byte| byte.is_ascii_digit());
    if !ordinal_is_valid || stem.is_empty() {
        return false;
    }
    let long_prefix = dotless
        .chars()
        .filter(|character| *character != '.')
        .take(SHORT_NAME_PREFIX_CHARACTERS)
        .collect::<String>();
    if stem == long_prefix {
        return true;
    }
    let hashed_prefix = long_prefix
        .chars()
        .take(HASHED_SHORT_NAME_PREFIX_CHARACTERS)
        .collect::<String>();
    stem.len() == HASHED_SHORT_NAME_PREFIX_CHARACTERS + HASHED_SHORT_NAME_HASH_CHARACTERS
        && stem.starts_with(&hashed_prefix)
        && stem[hashed_prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_detected_in_names_and_paths() {
        for value in [
            "a\nb",
            "tab\there",
            "nul\0",
            "bell\u{7}",
            "del\u{7f}",
            "next\u{85}line",
        ] {
            assert!(text_has_control_characters(value), "{value:?}");
        }
        for value in ["src/main.rs", "notes/año 2026.md", "emoji 🎉.txt", ""] {
            assert!(!text_has_control_characters(value), "{value:?}");
        }
    }

    #[test]
    fn rejects_git_metadata_under_every_windows_and_macos_alias() {
        for path in [
            ".git",
            ".git/config",
            ".GIT/config",
            ".Git/hooks/pre-commit",
            ".git./config",
            ".git /config",
            ".git. . /config",
            "GIT~1/config",
            "git~12/hooks",
            "GI7A3F~1/config",
            ".git::$DATA",
            ".git:stream/config",
            "sub/.git/config",
            "vendor/lib/.git",
            ".g\u{200c}it/config",
            "\u{feff}.git/config",
        ] {
            assert!(path_is_protected(path), "{path} must be protected");
        }
    }

    #[test]
    fn rejects_credential_paths_under_case_and_trailing_aliases() {
        for path in [
            ".ssh/id_ed25519",
            ".SSH/id_ed25519",
            "SSH~1/id_ed25519",
            ".gnupg/pubring.kbx",
            ".AWS./credentials",
            ".env",
            ".ENV",
            ".env.local",
            ".Env.Production",
            "ENV~1.LOC",
            ".codex/auth.json",
            ".CODEX/AUTH.JSON",
            ".claude/.credentials.json",
            ".claude/.credentials.json:stream",
        ] {
            assert!(path_is_protected(path), "{path} must be protected");
        }
    }

    #[test]
    fn keeps_ordinary_git_and_configuration_files_editable() {
        for path in [
            ".github/workflows/ci.yml",
            ".gitignore",
            ".gitattributes",
            ".gitmodules",
            ".gitkeep",
            "git/notes.md",
            "docs/.github/README.md",
            "src/git~backup.rs",
            "git~0/file",
            "gitx~1/file",
            ".env.example",
            "src/.env",
            "templates/.ssh/config",
            ".codex/config.toml",
            ".claude/settings.json",
            "src/config.ts",
            "",
        ] {
            assert!(!path_is_protected(path), "{path} must stay editable");
        }
    }

    #[test]
    fn identifies_git_metadata_components() {
        assert!(component_is_git_metadata(".git"));
        assert!(component_is_git_metadata(".GIT."));
        assert!(component_is_git_metadata("git~1"));
        assert!(!component_is_git_metadata(".github"));
        assert!(!component_is_git_metadata(".gitignore"));
    }

    #[test]
    fn identifies_windows_reserved_components() {
        for component in [
            "CON",
            "con.txt",
            "Nul",
            "aux.tar.gz",
            "COM1",
            "lpt9.log",
            "com\u{b9}",
            "prn ",
            "file.txt:stream",
            "CONIN$",
        ] {
            assert!(component_is_windows_reserved(component), "{component}");
        }
        for component in [
            "console.log",
            "com10",
            "lpt0",
            "auxiliary",
            "nul-device",
            "readme",
        ] {
            assert!(!component_is_windows_reserved(component), "{component}");
        }
    }
}
