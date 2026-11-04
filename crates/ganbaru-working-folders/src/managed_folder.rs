//! Location of the managed working folder each project owns inside the vault.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::ProjectWorkingFolderId;

/// Vault directory that holds one managed working folder per project.
pub const MANAGED_PROJECTS_DIRECTORY: &str = "projects";

const MAX_MANAGED_FOLDER_NAME_BYTES: usize = 240;

/// Checks that a project identifier can name its managed folder as exactly one path segment on
/// every supported platform.
///
/// A leading dot is refused as well. Project identifiers are UUIDs, while Git and several coding
/// agents read configuration from hidden folders in the parents of the folder they run in, so a
/// project folder named `.opencode` or `.git` would configure tools launched in every other
/// project folder.
///
/// # Errors
///
/// Returns a message when the identifier is empty, too long, starts with a dot, or contains a
/// separator, drive marker, wildcard, or control character.
pub fn validate_managed_folder_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.starts_with('.')
        || value.len() > MAX_MANAGED_FOLDER_NAME_BYTES
        || value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        })
    {
        return Err("id cannot be used as a project folder name".to_string());
    }
    Ok(())
}

/// Returns the vault-relative path stored for a project's managed folder.
///
/// # Errors
///
/// Returns a message when the project identifier is not a single folder name.
pub fn managed_project_relative_path(project_id: &str) -> Result<String, String> {
    validate_managed_folder_name(project_id)?;
    Ok(format!("{MANAGED_PROJECTS_DIRECTORY}/{project_id}"))
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
/// Returns a message when the project identifier cannot name a managed folder, or when the id
/// would be too long for a working-folder id.
pub fn managed_folder_id(project_id: &str) -> Result<ProjectWorkingFolderId, String> {
    validate_managed_folder_name(project_id)?;
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

/// Creates a project's managed folder when missing and returns its canonical path, which is
/// always a real directory directly inside the vault `projects/` directory. The vault root may
/// itself be reached through a link the user chose; links below it are refused.
///
/// # Errors
///
/// Returns an error when the identifier is not a single folder name, when `projects/` or the
/// project folder is a link or not a directory, or when the canonical folder is not a direct
/// child of the canonical `projects/` directory.
pub fn resolve_managed_project_folder(vault_root: &Path, project_id: &str) -> io::Result<PathBuf> {
    validate_managed_folder_name(project_id)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let projects = vault_root.join(MANAGED_PROJECTS_DIRECTORY);
    let folder = projects.join(project_id);
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
    fn resolves_a_plain_project_id_to_a_direct_child_of_projects() {
        let vault = test_vault("plain");
        let folder = resolve_managed_project_folder(&vault, "project-1").unwrap();
        assert_eq!(
            folder,
            fs::canonicalize(&vault)
                .unwrap()
                .join("projects")
                .join("project-1")
        );
        assert!(folder.is_dir());
        assert_eq!(
            resolve_managed_project_folder(&vault, "project-1").unwrap(),
            folder
        );
        assert_eq!(
            managed_project_relative_path("project-1").unwrap(),
            "projects/project-1"
        );
        fs::remove_dir_all(vault).unwrap();
    }

    #[test]
    fn rejects_identifiers_that_are_not_one_folder_name() {
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
        ] {
            assert!(
                resolve_managed_project_folder(&vault, crafted).is_err(),
                "{crafted:?} must be refused"
            );
            assert!(managed_project_relative_path(crafted).is_err());
        }
        assert!(
            validate_managed_folder_name(&"a".repeat(MAX_MANAGED_FOLDER_NAME_BYTES + 1)).is_err()
        );
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
            assert!(managed_project_relative_path(hidden).is_err());
            assert!(resolve_managed_project_folder(&vault, hidden).is_err());
        }
        assert!(validate_managed_folder_name("project.opencode").is_ok());
        assert!(validate_managed_folder_name("a..b").is_ok());
        assert_eq!(
            fs::read_dir(vault.join("projects"))
                .map(|entries| entries.count())
                .unwrap_or(0),
            0
        );
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
        assert!(resolve_managed_project_folder(&relinked, "project-1").is_err());
        assert!(!outside.join("project-1").exists());

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
        let folder = resolve_managed_project_folder(&link, "project-1").unwrap();
        assert_eq!(
            folder,
            fs::canonicalize(&target)
                .unwrap()
                .join("projects")
                .join("project-1")
        );
        fs::remove_dir_all(parent).unwrap();
        fs::remove_dir_all(target).unwrap();
    }
}
