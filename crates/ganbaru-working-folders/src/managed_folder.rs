//! Location of the managed working folder each project owns inside the vault.
//!
//! A managed folder is named after its project when the project is created, as one portable
//! folder name directly inside the vault `projects/` directory. The name stays when the project
//! is renamed, because provider continuations, Git worktrees, and tool environments remember the
//! absolute path of the folder they run in.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::ProjectWorkingFolderId;

/// Vault directory that holds one managed working folder per project.
pub const MANAGED_PROJECTS_DIRECTORY: &str = "projects";

/// Longest managed folder name accepted, in bytes.
const MAX_MANAGED_FOLDER_NAME_BYTES: usize = 240;

/// Longest managed folder name generated from a project name, in bytes. It stays well below the
/// accepted bound so a numeric suffix fits and files deep inside the folder stay within the
/// default Windows path length.
const GENERATED_FOLDER_NAME_BYTES: usize = 80;

/// Folder name used when nothing of the project name can name a folder.
const FALLBACK_FOLDER_NAME: &str = "Project";

/// Highest numeric suffix tried before a name is reported as unavailable.
const MAX_FOLDER_NAME_SUFFIX: u32 = 10_000;

/// Characters that separate paths, mark drives, or are refused by Windows file names.
const FORBIDDEN_FOLDER_CHARACTERS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Invisible formatting characters that would let a folder name display differently from its
/// bytes: directional marks, embeddings, overrides, isolates, and zero-width spaces. The
/// zero-width joiner and non-joiner stay valid, since emoji and several scripts need them.
const INVISIBLE_FOLDER_CHARACTERS: &[char] = &[
    '\u{200B}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2060}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{FEFF}',
];

/// Device names Windows reserves as a file name stem in every folder, compared ignoring ASCII
/// case.
const WINDOWS_RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Checks that `value` is one managed folder name that every supported platform stores as
/// written: no separator, drive marker, wildcard, control or invisible formatting character, no
/// leading or trailing space or dot, no Windows device name, and at most 240 bytes.
///
/// A leading dot is refused because Git and several coding agents read configuration from hidden
/// folders in the parents of the folder they run in, so a project folder named `.opencode` or
/// `.git` would configure tools launched in every other project folder. Trailing spaces and dots
/// are refused because Windows drops them, which would name another folder.
///
/// # Errors
///
/// Returns a message when `value` cannot name a managed folder.
pub fn validate_managed_folder_name(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_MANAGED_FOLDER_NAME_BYTES
        || value.starts_with(['.', ' '])
        || value.ends_with(['.', ' '])
        || value.chars().any(is_refused_character)
        || has_reserved_stem(value)
    {
        return Err("name cannot be used as a project folder name".to_string());
    }
    Ok(())
}

/// Returns the portable folder name a project called `project_name` prefers.
///
/// Refused characters become spaces, runs of white space become one space, leading and trailing
/// spaces and dots are dropped, the name is shortened to a bounded length, and a Windows device
/// name gets an underscore after its stem. A name with nothing usable left becomes `Project`.
/// The result always passes [`validate_managed_folder_name`].
pub fn portable_folder_name(project_name: &str) -> String {
    let spaced: String = project_name
        .chars()
        .filter(|character| !INVISIBLE_FOLDER_CHARACTERS.contains(character))
        .map(|character| {
            if is_refused_character(character) {
                ' '
            } else {
                character
            }
        })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = trim_folder_name(truncate_to_bytes(
        trim_folder_name(&collapsed),
        GENERATED_FOLDER_NAME_BYTES,
    ));
    if name.is_empty() {
        return FALLBACK_FOLDER_NAME.to_string();
    }
    unreserve(name)
}

/// Returns the key two managed folder names collide under: Windows and macOS file systems ignore
/// case by default, so names that differ only in case would name one folder there.
pub fn managed_folder_key(name: &str) -> String {
    name.to_lowercase()
}

/// Returns `preferred`, or `preferred` with the lowest numeric suffix (` 2`, ` 3`, and so on)
/// that `is_taken` does not report, shortened so the result stays a generated folder name.
/// `preferred` must already be a [`portable_folder_name`].
///
/// # Errors
///
/// Returns a message when every suffix up to the bound is taken.
pub fn unique_folder_name(
    preferred: &str,
    is_taken: impl Fn(&str) -> bool,
) -> Result<String, String> {
    if !is_taken(preferred) {
        return Ok(preferred.to_string());
    }
    for number in 2..=MAX_FOLDER_NAME_SUFFIX {
        let suffix = format!(" {number}");
        let base = trim_folder_name(truncate_to_bytes(
            preferred,
            GENERATED_FOLDER_NAME_BYTES.saturating_sub(suffix.len()),
        ));
        let base = if base.is_empty() {
            FALLBACK_FOLDER_NAME
        } else {
            base
        };
        let candidate = unreserve(&format!("{base}{suffix}"));
        if !is_taken(&candidate) {
            return Ok(candidate);
        }
    }
    Err("no free project folder name is left for this project name".to_string())
}

/// Returns the vault-relative path of the managed folder named `folder_name`.
///
/// # Errors
///
/// Returns a message when `folder_name` cannot name a managed folder.
pub fn managed_relative_path(folder_name: &str) -> Result<String, String> {
    validate_managed_folder_name(folder_name)?;
    Ok(format!("{MANAGED_PROJECTS_DIRECTORY}/{folder_name}"))
}

/// Returns the folder name a stored managed relative path names, or `None` when the path is not
/// one managed folder name directly inside `projects/`.
pub fn managed_folder_name(relative_path: &str) -> Option<&str> {
    let name = relative_path
        .strip_prefix(MANAGED_PROJECTS_DIRECTORY)?
        .strip_prefix('/')?;
    validate_managed_folder_name(name).ok().map(|()| name)
}

/// Returns the collision keys of every entry directly inside the vault `projects/` directory,
/// whatever its type, so a new managed folder never takes over a folder or file already there.
///
/// # Errors
///
/// Returns an error when the directory exists but cannot be listed.
pub fn existing_folder_keys(vault_root: &Path) -> io::Result<Vec<String>> {
    let projects = vault_root.join(MANAGED_PROJECTS_DIRECTORY);
    let entries = match fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    entries
        .map(|entry| entry.map(|entry| managed_folder_key(&entry.file_name().to_string_lossy())))
        .collect()
}

/// Id prefix of the managed folder created with a project.
const MANAGED_FOLDER_ID_PREFIX: &str = "working-folder:";

/// Id prefix of the routine projects the baseline schema seeds.
const SEEDED_PROJECT_ID_PREFIX: &str = "project-routine-";

/// Id prefix the baseline schema gives the managed folders of its seeded routine projects.
const SEEDED_FOLDER_ID_PREFIX: &str = "working-folder-routine-";

/// Returns the one id a project's managed folder has: `working-folder:{project id}`, or the
/// seeded key for a routine project the baseline schema seeds.
///
/// Every device keys a project's managed folder by this id, so sync admits a managed folder only
/// under it and an external folder never under an id it returns.
///
/// # Errors
///
/// Returns a message when the project identifier is not one plain path segment, or when the id
/// would be too long for a working-folder id.
pub fn managed_folder_id(project_id: &str) -> Result<ProjectWorkingFolderId, String> {
    validate_project_identifier(project_id)?;
    ProjectWorkingFolderId::new(match project_id.strip_prefix(SEEDED_PROJECT_ID_PREFIX) {
        Some(routine) => format!("{SEEDED_FOLDER_ID_PREFIX}{routine}"),
        None => format!("{MANAGED_FOLDER_ID_PREFIX}{project_id}"),
    })
}

/// Returns the project whose managed folder id is `folder_id`, or `None` when `folder_id` is
/// not the [`managed_folder_id`] of any project.
pub fn managed_folder_project(folder_id: &str) -> Option<String> {
    let project = match folder_id.strip_prefix(SEEDED_FOLDER_ID_PREFIX) {
        Some(routine) => format!("{SEEDED_PROJECT_ID_PREFIX}{routine}"),
        None => folder_id.strip_prefix(MANAGED_FOLDER_ID_PREFIX)?.to_owned(),
    };
    managed_folder_id(&project)
        .is_ok_and(|id| id.as_str() == folder_id)
        .then_some(project)
}

/// Creates the managed folder named `folder_name` when missing and returns its canonical path,
/// which is always a real directory directly inside the vault `projects/` directory. The vault
/// root may itself be reached through a link the user chose; links below it are refused.
///
/// # Errors
///
/// Returns an error when `folder_name` cannot name a managed folder, when `projects/` or the
/// project folder is a link or not a directory, or when the canonical folder is not a direct
/// child of the canonical `projects/` directory.
pub fn resolve_managed_project_folder(vault_root: &Path, folder_name: &str) -> io::Result<PathBuf> {
    validate_managed_folder_name(folder_name)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let projects = vault_root.join(MANAGED_PROJECTS_DIRECTORY);
    let folder = projects.join(folder_name);
    let mut components = folder
        .strip_prefix(&projects)
        .unwrap_or(&folder)
        .components();
    if !matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) {
        return Err(outside_projects_error(&folder));
    }
    ganbaru_atomic_files::create_directories_within(vault_root, &folder)?;
    ganbaru_atomic_files::reject_symlink(&folder)?;
    let canonical_projects = fs::canonicalize(&projects)?;
    let canonical_folder = fs::canonicalize(&folder)?;
    if canonical_folder.parent() != Some(canonical_projects.as_path())
        || !fs::metadata(&canonical_folder)?.is_dir()
    {
        return Err(outside_projects_error(&folder));
    }
    Ok(canonical_folder)
}

/// Checks that a project identifier is one plain path segment, so the managed folder id built
/// from it stays a plain key.
fn validate_project_identifier(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.starts_with('.')
        || value.len() > MAX_MANAGED_FOLDER_NAME_BYTES
        || value.chars().any(is_refused_character)
    {
        return Err("id cannot name a project working folder".to_string());
    }
    Ok(())
}

fn is_refused_character(character: char) -> bool {
    character.is_control()
        || FORBIDDEN_FOLDER_CHARACTERS.contains(&character)
        || INVISIBLE_FOLDER_CHARACTERS.contains(&character)
}

/// Whether Windows reserves the part of `name` before its first dot, ignoring trailing spaces.
fn has_reserved_stem(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    WINDOWS_RESERVED_STEMS
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

/// Adds an underscore after a reserved stem, so `CON` becomes `CON_` and `nul.txt` `nul_.txt`.
fn unreserve(name: &str) -> String {
    if !has_reserved_stem(name) {
        return name.to_string();
    }
    match name.split_once('.') {
        Some((stem, rest)) => format!("{stem}_.{rest}"),
        None => format!("{name}_"),
    }
}

fn trim_folder_name(name: &str) -> &str {
    name.trim_matches(['.', ' '])
}

/// Returns the longest prefix of `value` within `max_bytes` that ends on a character boundary.
fn truncate_to_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn outside_projects_error(folder: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "{} is not a managed project folder inside the vault",
            folder.display()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_VAULT: AtomicUsize = AtomicUsize::new(0);

    fn test_vault(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-managed-folder-{label}-{}-{}",
            std::process::id(),
            NEXT_VAULT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test vault");
        path
    }

    #[test]
    fn resolves_a_folder_name_to_a_direct_child_of_projects() {
        let vault = test_vault("plain");
        let folder = resolve_managed_project_folder(&vault, "Thesis").unwrap();
        assert_eq!(
            folder,
            fs::canonicalize(&vault)
                .unwrap()
                .join("projects")
                .join("Thesis")
        );
        assert!(folder.is_dir());
        assert_eq!(
            resolve_managed_project_folder(&vault, "Thesis").unwrap(),
            folder
        );
        assert_eq!(managed_relative_path("Thesis").unwrap(), "projects/Thesis");
        fs::remove_dir_all(vault).unwrap();
    }

    #[test]
    fn rejects_names_that_are_not_one_portable_folder_name() {
        let vault = test_vault("crafted");
        for crafted in [
            "",
            " ",
            ".",
            "..",
            "../outside",
            "a/b",
            "a\\b",
            "C:",
            "/etc",
            "a\0b",
            "trailing.",
            "trailing ",
            " leading",
            "CON",
            "nul.txt",
            "Com1 .md",
            "lpt¹",
            "right\u{202E}left",
            "zero\u{200B}width",
        ] {
            assert!(
                resolve_managed_project_folder(&vault, crafted).is_err(),
                "{crafted:?} must be refused"
            );
            assert!(managed_relative_path(crafted).is_err(), "{crafted:?}");
        }
        assert!(
            validate_managed_folder_name(&"a".repeat(MAX_MANAGED_FOLDER_NAME_BYTES + 1)).is_err()
        );
        for accepted in [
            "CONSOLE",
            "con_",
            "Año 2026",
            "a..b",
            "project.opencode",
            "👩\u{200D}💻",
        ] {
            assert!(
                validate_managed_folder_name(accepted).is_ok(),
                "{accepted:?} must be accepted"
            );
        }
        assert_eq!(
            fs::read_dir(vault.parent().unwrap())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name() == "outside")
                .count(),
            0
        );
        fs::remove_dir_all(vault).unwrap();
    }

    #[test]
    fn rejects_hidden_folder_names_that_tools_load_from_parent_folders() {
        let vault = test_vault("hidden");
        for hidden in [".opencode", ".grok", ".git", ".claude", ". ", ".a..b"] {
            assert!(
                validate_managed_folder_name(hidden).is_err(),
                "{hidden:?} must be refused"
            );
            assert!(managed_relative_path(hidden).is_err());
            assert!(resolve_managed_project_folder(&vault, hidden).is_err());
        }
        assert_eq!(
            fs::read_dir(vault.join("projects"))
                .map(|entries| entries.count())
                .unwrap_or(0),
            0
        );
        fs::remove_dir_all(vault).unwrap();
    }

    #[test]
    fn portable_names_keep_readable_project_names() {
        for (project, folder) in [
            ("Thesis", "Thesis"),
            ("  Año   nuevo  ", "Año nuevo"),
            ("Work: Q3/Q4 plan?", "Work Q3 Q4 plan"),
            ("...hidden", "hidden"),
            ("Release v1.", "Release v1"),
            ("CON", "CON_"),
            ("nul.notes", "nul_.notes"),
            ("Line\nbreak\ttab", "Line break tab"),
            ("right\u{202E}left", "rightleft"),
            ("👩\u{200D}💻 Code", "👩\u{200D}💻 Code"),
            ("", "Project"),
            ("..", "Project"),
            ("<>|", "Project"),
        ] {
            assert_eq!(portable_folder_name(project), folder, "{project:?}");
            assert!(validate_managed_folder_name(folder).is_ok(), "{folder:?}");
        }
    }

    #[test]
    fn portable_names_are_bounded_on_a_character_boundary() {
        let long = "é".repeat(100);
        let folder = portable_folder_name(&long);
        assert_eq!(folder.len(), GENERATED_FOLDER_NAME_BYTES);
        assert!(validate_managed_folder_name(&folder).is_ok());
        let spaced = format!("{} tail", "a".repeat(GENERATED_FOLDER_NAME_BYTES - 1));
        assert_eq!(
            portable_folder_name(&spaced),
            "a".repeat(GENERATED_FOLDER_NAME_BYTES - 1)
        );
    }

    #[test]
    fn unique_names_add_the_lowest_free_suffix_ignoring_case() {
        let taken: HashSet<String> = ["thesis", "thesis 2", "con_"]
            .into_iter()
            .map(String::from)
            .collect();
        let is_taken = |name: &str| taken.contains(&managed_folder_key(name));
        assert_eq!(unique_folder_name("Notes", is_taken).unwrap(), "Notes");
        assert_eq!(unique_folder_name("Thesis", is_taken).unwrap(), "Thesis 3");
        assert_eq!(unique_folder_name("CON_", is_taken).unwrap(), "CON_ 2");

        let long = "b".repeat(GENERATED_FOLDER_NAME_BYTES);
        let suffixed = unique_folder_name(&long, |name| name == long).unwrap();
        assert_eq!(suffixed.len(), GENERATED_FOLDER_NAME_BYTES);
        assert!(suffixed.ends_with(" 2"));
        assert!(validate_managed_folder_name(&suffixed).is_ok());

        assert!(unique_folder_name("Full", |_| true).is_err());
    }

    #[test]
    fn stored_paths_name_one_managed_folder() {
        assert_eq!(managed_folder_name("projects/Thesis"), Some("Thesis"));
        assert_eq!(
            managed_folder_name("projects/project-routine-learning"),
            Some("project-routine-learning")
        );
        for other in [
            "projects",
            "projects/",
            "projects/a/b",
            "projects/.git",
            "projects/CON",
            "assets/Thesis",
            "projectsThesis",
            "/projects/Thesis",
        ] {
            assert_eq!(managed_folder_name(other), None, "{other}");
        }
    }

    #[test]
    fn existing_keys_cover_every_entry_inside_projects() {
        let vault = test_vault("existing");
        assert!(existing_folder_keys(&vault).unwrap().is_empty());
        fs::create_dir_all(vault.join("projects").join("Thesis")).unwrap();
        fs::write(vault.join("projects").join("Notes.md"), "").unwrap();
        let mut keys = existing_folder_keys(&vault).unwrap();
        keys.sort();
        assert_eq!(keys, ["notes.md", "thesis"]);
        fs::remove_dir_all(vault).unwrap();
    }

    #[test]
    fn managed_folder_ids_name_exactly_one_project() {
        for (project, folder) in [
            ("project-1", "working-folder:project-1"),
            (
                "0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f0a63",
                "working-folder:0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f0a63",
            ),
            (
                "project-routine-learning",
                "working-folder-routine-learning",
            ),
        ] {
            assert_eq!(managed_folder_id(project).unwrap().as_str(), folder);
            assert_eq!(managed_folder_project(folder).as_deref(), Some(project));
        }
        for other in [
            "working-folder:project-routine-learning",
            "working-folder:external:0f8e2c4a-6b1d-4e3f-9a7c-5d2b8e1f0a63",
            "working-folder:.git",
            "working-folder:",
            "working-folder-1",
            "folder-external",
        ] {
            assert_eq!(managed_folder_project(other), None, "{other}");
        }
        assert!(managed_folder_id("a/b").is_err());
        assert!(managed_folder_id(&"a".repeat(60)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_links_planted_inside_the_vault() {
        let vault = test_vault("links");
        let outside = test_vault("links-target");
        fs::create_dir_all(vault.join("projects")).unwrap();
        std::os::unix::fs::symlink(&outside, vault.join("projects").join("linked")).unwrap();
        assert!(resolve_managed_project_folder(&vault, "linked").is_err());

        let relinked = test_vault("relinked");
        std::os::unix::fs::symlink(&outside, relinked.join("projects")).unwrap();
        assert!(resolve_managed_project_folder(&relinked, "Thesis").is_err());
        assert!(!outside.join("Thesis").exists());

        for path in [vault, outside, relinked] {
            fs::remove_dir_all(path).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn accepts_a_vault_root_reached_through_a_user_chosen_link() {
        let target = test_vault("root-target");
        let parent = test_vault("root-link");
        let link = parent.join("Ganbaru");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let folder = resolve_managed_project_folder(&link, "Thesis").unwrap();
        assert_eq!(
            folder,
            fs::canonicalize(&target)
                .unwrap()
                .join("projects")
                .join("Thesis")
        );
        fs::remove_dir_all(parent).unwrap();
        fs::remove_dir_all(target).unwrap();
    }
}
