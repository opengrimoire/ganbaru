//! Hands authorized workspace paths to the operating system's default handler.
//!
//! Workspace files can come from a cloned repository or an agent, and the default handler for
//! some types runs the file as a program. Documents open directly; anything else opens only
//! with an [`ExternalOpenConfirmation`] from a native dialog that names the file.

use super::AuthorizedWorkingFolder;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

/// Lowercase file extensions that open without confirmation. No supported platform's default
/// handler runs these as programs. Script and source types that some platforms associate with an
/// interpreter (`js`, `py`, `sh`, `ps1`, `rb`, `pl`, and similar) are deliberately absent.
pub const EXTERNAL_OPEN_DOCUMENT_EXTENSIONS: &[&str] = &[
    // Text, data, and configuration.
    "txt", "text", "log", "md", "markdown", "mdx", "rst", "adoc", "tex", "csv", "tsv", "json",
    "jsonc", "json5", "yaml", "yml", "toml", "ini", "cfg", "conf", "env", "lock", "xml", "diff",
    "patch", "sql", "proto", "graphql", "gql",
    // Source files without an executing default handler.
    "rs", "ts", "tsx", "mts", "cts", "jsx", "svelte", "vue", "css", "scss", "sass", "less", "html",
    "htm", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "go", "java", "kt", "swift",
    // Images.
    "png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "svg", "tif", "tiff", "heic",
    // Audio and video.
    "mp3", "wav", "flac", "ogg", "opus", "m4a", "aac", "mp4", "m4v", "webm", "mkv", "mov", "avi",
    // Documents without macros.
    "pdf", "rtf", "epub", "odt", "ods", "odp", "docx", "xlsx", "pptx",
];

/// How the default handler may receive a workspace path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalOpenKind {
    /// A plain folder or a document type that opens directly.
    Document,
    /// A type that could run as a program, or whose handler is unknown, so it opens only after
    /// the user confirms it in a native dialog.
    NeedsConfirmation,
}

/// Proof that the user approved opening one path in a native dialog that named it.
#[derive(Debug)]
pub struct ExternalOpenConfirmation {
    _private: (),
}

impl ExternalOpenConfirmation {
    /// Records the approval returned by the native confirmation dialog. Only that dialog's
    /// result handler calls this.
    pub fn from_native_dialog() -> Self {
        Self { _private: () }
    }
}

/// Classifies a path by its final name and file type.
///
/// Trailing dots and spaces are ignored because Windows drops them when it resolves a name,
/// so `run.exe.` would run `run.exe`. A folder with an extension needs confirmation because
/// macOS opens bundles such as `.app` as programs and Windows routes some suffixes to shell
/// handlers. A file without an extension opens directly unless it is executable.
pub fn classify_external_open(
    file_name: &OsStr,
    is_directory: bool,
    executable: bool,
) -> ExternalOpenKind {
    let name = file_name.to_string_lossy();
    let name = name.trim_end_matches(['.', ' ']);
    let extension = Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    let direct = match extension {
        None => is_directory || !executable,
        Some(_) if is_directory => false,
        Some(extension) => EXTERNAL_OPEN_DOCUMENT_EXTENSIONS.contains(&extension.as_str()),
    };
    if direct {
        ExternalOpenKind::Document
    } else {
        ExternalOpenKind::NeedsConfirmation
    }
}

/// Classifies an existing path for [`open_authorized_document`].
pub fn external_open_kind(path: &Path) -> ChatResult<ExternalOpenKind> {
    let metadata = fs::metadata(path).map_err(|_| open_error())?;
    let file_name = path.file_name().ok_or_else(open_error)?;
    Ok(classify_external_open(
        file_name,
        metadata.is_dir(),
        is_executable(&metadata),
    ))
}

/// Opens the authorized folder root itself, which the user bound through a native picker.
pub fn open_authorized_workspace(authorized: &AuthorizedWorkingFolder) -> ChatResult<()> {
    spawn_default_handler(&authorized.canonical_path)
}

/// Opens a path inside the authorized folder when it is a document, and refuses anything that
/// needs confirmation before the default handler runs.
pub fn open_authorized_document(
    authorized: &AuthorizedWorkingFolder,
    path: &Path,
) -> ChatResult<()> {
    require_inside(authorized, path)?;
    if external_open_kind(path)? != ExternalOpenKind::Document {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "This file type opens only after confirmation",
            true,
        ));
    }
    spawn_default_handler(path)
}

/// Opens any path inside the authorized folder after the user confirmed it natively.
pub fn open_authorized_path_confirmed(
    authorized: &AuthorizedWorkingFolder,
    path: &Path,
    _confirmation: ExternalOpenConfirmation,
) -> ChatResult<()> {
    require_inside(authorized, path)?;
    spawn_default_handler(path)
}

fn require_inside(authorized: &AuthorizedWorkingFolder, path: &Path) -> ChatResult<()> {
    if path.starts_with(&authorized.canonical_path) {
        return Ok(());
    }
    Err(ChatError::new(
        ChatErrorCode::Permission,
        "Workspace path resolves outside the bound folder",
        false,
    ))
}

fn spawn_default_handler(path: &Path) -> ChatResult<()> {
    spawn_file_manager(path).map_err(|_| open_error())
}

fn open_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "The project working-folder path could not be opened",
        true,
    )
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("xdg-open", [path.as_os_str()])
}

#[cfg(target_os = "macos")]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("open", [path.as_os_str()])
}

/// Canonical Windows paths carry the `\\?\` verbatim prefix, which Explorer does not accept,
/// so the path is simplified first when that is lossless.
#[cfg(windows)]
fn spawn_file_manager(path: &Path) -> std::io::Result<()> {
    spawn_file_manager_command("explorer.exe", [dunce::simplified(path).as_os_str()])
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn spawn_file_manager(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "platform is unsupported",
    ))
}

fn spawn_file_manager_command<I, S>(program: &str, arguments: I) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str, is_directory: bool, executable: bool) -> ExternalOpenKind {
        classify_external_open(OsStr::new(name), is_directory, executable)
    }

    #[test]
    fn documents_open_directly_regardless_of_case_or_executable_bit() {
        for name in [
            "report.pdf",
            "notes.MD",
            "photo.JPeg",
            "Cargo.lock",
            "index.html",
        ] {
            assert_eq!(
                kind(name, false, false),
                ExternalOpenKind::Document,
                "{name}"
            );
        }
        assert_eq!(
            kind("copied-from-ntfs.txt", false, true),
            ExternalOpenKind::Document
        );
        assert_eq!(kind("src", true, true), ExternalOpenKind::Document);
    }

    #[test]
    fn programs_scripts_and_shell_handlers_need_confirmation() {
        for name in [
            "evil.sh",
            "run.exe",
            "setup.bat",
            "tool.cmd",
            "macro.js",
            "task.vbs",
            "page.hta",
            "shortcut.lnk",
            "link.url",
            "launcher.desktop",
            "start.command",
            "build.py",
            "script.ps1",
            "package.jar",
            "installer.msi",
            "legacy.xls",
            "x.PDF.exe",
            "unknown.weird",
        ] {
            assert_eq!(
                kind(name, false, false),
                ExternalOpenKind::NeedsConfirmation,
                "{name}"
            );
        }
    }

    #[test]
    fn trailing_dots_and_spaces_do_not_hide_an_executable_extension() {
        for name in ["run.exe.", "run.exe . ", "setup.bat  "] {
            assert_eq!(
                kind(name, false, false),
                ExternalOpenKind::NeedsConfirmation,
                "{name}"
            );
        }
        assert_eq!(kind("notes.txt.", false, false), ExternalOpenKind::Document);
    }

    #[test]
    fn extensionless_files_need_confirmation_only_when_executable() {
        assert_eq!(kind("README", false, false), ExternalOpenKind::Document);
        assert_eq!(kind("Makefile", false, false), ExternalOpenKind::Document);
        assert_eq!(
            kind("payload", false, true),
            ExternalOpenKind::NeedsConfirmation
        );
    }

    #[test]
    fn folders_with_an_extension_need_confirmation() {
        assert_eq!(
            kind("Calculator.app", true, true),
            ExternalOpenKind::NeedsConfirmation
        );
        assert_eq!(
            kind("x.{645FF040-5081-101B-9F08-00AA002F954E}", true, false),
            ExternalOpenKind::NeedsConfirmation
        );
    }

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = ganbaru_atomic_files::create_private_temporary_directory(
                &std::env::temp_dir(),
                "ganbaru-external-open-test-",
            )
            .expect("create test directory");
            Self(fs::canonicalize(path).expect("canonicalize test directory"))
        }

        fn authorized(&self) -> AuthorizedWorkingFolder {
            AuthorizedWorkingFolder {
                working_folder_id: crate::models::ProjectWorkingFolderId::new(
                    "workspace:external-open-test",
                )
                .expect("workspace ID should be valid"),
                canonical_path: self.0.clone(),
                repository_kind: crate::models::RepositoryKind::None,
                repository_identity: None,
                repository_storage_identity: None,
                git_trust: crate::git::hardened::GitFolderTrust::Untrusted,
                project_configuration: crate::models::ProjectConfigurationAccess::Restricted,
            }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn document_open_refuses_types_that_need_confirmation_before_spawning() {
        let root = TestDirectory::new();
        let script = root.0.join("evil.sh");
        fs::write(&script, "#!/bin/sh\n").expect("write script");
        let authorized = root.authorized();

        let error = open_authorized_document(&authorized, &script).expect_err("script refused");
        assert_eq!(error.code, ChatErrorCode::Permission);

        let outside = TestDirectory::new();
        let outside_file = outside.0.join("notes.txt");
        fs::write(&outside_file, "text").expect("write outside file");
        let error = open_authorized_path_confirmed(
            &authorized,
            &outside_file,
            ExternalOpenConfirmation::from_native_dialog(),
        )
        .expect_err("outside path refused");
        assert_eq!(error.code, ChatErrorCode::Permission);
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_extensionless_file_on_disk_needs_confirmation() {
        use std::os::unix::fs::PermissionsExt;
        let root = TestDirectory::new();
        let payload = root.0.join("payload");
        fs::write(&payload, "#!/bin/sh\n").expect("write payload");
        assert_eq!(
            external_open_kind(&payload).unwrap(),
            ExternalOpenKind::Document
        );
        fs::set_permissions(&payload, fs::Permissions::from_mode(0o755)).expect("mark executable");
        assert_eq!(
            external_open_kind(&payload).unwrap(),
            ExternalOpenKind::NeedsConfirmation
        );
    }
}
