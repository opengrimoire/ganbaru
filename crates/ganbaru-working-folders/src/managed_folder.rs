//! Location of the managed working folder each project owns inside the vault.

use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

/// Vault directory that holds one managed working folder per project.
pub const MANAGED_PROJECTS_DIRECTORY: &str = "projects";

const MAX_MANAGED_FOLDER_NAME_BYTES: usize = 240;

/// Checks that a project identifier can name its managed folder as exactly one path segment on
/// every supported platform.
///
/// # Errors
///
/// Returns a message when the identifier is empty, too long, a relative marker, or contains a
/// separator, drive marker, wildcard, or control character.
pub fn validate_managed_folder_name(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value == "."
        || value == ".."
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
