//! Reading a whole-vault archive back into a folder, with its folder marker.

use super::{
    BACKUP_MAX_BYTES, BACKUP_MAX_DEPTH, BACKUP_MAX_ENTRIES, COPY_BUFFER_BYTES, DATABASE_FILE,
    VAULT_MANIFEST_FILE, is_bundle_layout_entry,
};
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

/// The app name a vault folder marker carries.
pub const VAULT_APP_MARKER: &str = "ganbaru";
/// The folder marker schema this build reads and writes.
pub const VAULT_SCHEMA_VERSION: u32 = 1;
/// Largest folder marker any reader accepts.
pub const MAX_VAULT_MANIFEST_BYTES: u64 = 64 * 1024;
/// Most skipped bundle entries named in one log line.
const MAX_REPORTED_SKIPPED_ENTRIES: usize = 8;
/// Refusal for an archive of another vault than the one expected.
pub const DIFFERENT_VAULT_ERROR: &str = "received bundle belongs to a different vault";

/// The vault folder marker.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultManifest {
    pub app: String,
    pub schema_version: u32,
    pub vault_id: String,
    pub display_name: String,
    pub created_at: String,
}

/// Parses and checks a folder marker. The identity names key store accounts and key files, so
/// it keeps to the wire identifier alphabet before anything uses it.
pub fn parse_vault_manifest(contents: &[u8]) -> Result<VaultManifest, String> {
    let manifest: VaultManifest = serde_json::from_slice(contents)
        .map_err(|error| format!("parse Ganbaru folder marker: {error}"))?;
    if manifest.app != VAULT_APP_MARKER {
        return Err("selected folder is not a Ganbaru folder".to_string());
    }
    if manifest.schema_version != VAULT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported Ganbaru folder schema version {}",
            manifest.schema_version
        ));
    }
    ganbaru_handoff::protocol::validate_identifier("Ganbaru folder id", &manifest.vault_id)?;
    Ok(manifest)
}

/// Entries left out of an extracted bundle or a vault replacement.
#[derive(Debug, Default)]
pub struct SkippedEntries {
    count: usize,
    named: Vec<PathBuf>,
}

impl SkippedEntries {
    /// Counts one skipped entry, keeping the first few names.
    pub fn push(&mut self, path: PathBuf) {
        self.count += 1;
        if self.named.len() < MAX_REPORTED_SKIPPED_ENTRIES {
            self.named.push(path);
        }
    }

    /// Whether nothing was skipped.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Logs the skipped entries, naming the first few, so nothing is left out silently.
    pub fn report(&self, operation: &str, reason: &str) {
        if self.count == 0 {
            return;
        }
        let named = self
            .named
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "{operation} skipped {} entries {reason}: {named}",
            self.count
        );
    }
}

/// Checks an archive entry path: relative, plain components, within the depth bound.
pub fn safe_archive_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().count() > BACKUP_MAX_DEPTH
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("backup contains an unsafe entry path".to_string());
    }
    Ok(path.to_path_buf())
}

/// Checks the central directory before anything is written: the archive must hold a vault
/// marker and database, its in-layout entries must declare at most `max_bytes` in total, and
/// the marker must parse. Returns the archived vault identity.
fn admit_backup_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    max_bytes: u64,
) -> Result<String, String> {
    if archive.len() > BACKUP_MAX_ENTRIES {
        return Err("backup contains too many entries".to_string());
    }
    let mut declared_bytes = 0_u64;
    let mut has_database = false;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| format!("read backup entry {index}: {error}"))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        if entry.is_dir() || !is_bundle_layout_entry(&relative, false) {
            continue;
        }
        has_database |= relative == Path::new(DATABASE_FILE);
        declared_bytes = declared_bytes.saturating_add(entry.size());
        if declared_bytes > max_bytes {
            return Err("backup exceeds the restore size limit".to_string());
        }
    }
    if !has_database {
        return Err("backup is missing its database".to_string());
    }
    let manifest = archive
        .by_name(VAULT_MANIFEST_FILE)
        .map_err(|error| format!("backup is missing its Ganbaru folder marker: {error}"))?;
    if !manifest.is_file() || manifest.size() > MAX_VAULT_MANIFEST_BYTES {
        return Err("backup has an invalid Ganbaru folder marker".to_string());
    }
    let mut contents = Vec::new();
    manifest
        .take(MAX_VAULT_MANIFEST_BYTES + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("read backup Ganbaru folder marker: {error}"))?;
    if contents.len() as u64 > MAX_VAULT_MANIFEST_BYTES {
        return Err("backup has an invalid Ganbaru folder marker".to_string());
    }
    Ok(parse_vault_manifest(&contents)?.vault_id)
}

/// Extracts a bundle into a new `destination`, skipping entries outside the vault layout instead
/// of failing the whole restore.
pub fn extract_backup_archive(
    archive_path: &Path,
    destination: &Path,
    expected_vault_id: Option<&str>,
) -> Result<SkippedEntries, String> {
    extract_backup_archive_within(
        archive_path,
        destination,
        expected_vault_id,
        BACKUP_MAX_BYTES,
    )
}

/// [`extract_backup_archive`] with an explicit size bound.
pub fn extract_backup_archive_within(
    archive_path: &Path,
    destination: &Path,
    expected_vault_id: Option<&str>,
    max_bytes: u64,
) -> Result<SkippedEntries, String> {
    let file = fs::File::open(archive_path).map_err(|error| format!("open backup: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("open backup archive: {error}"))?;
    let vault_id = admit_backup_archive(&mut archive, max_bytes)?;
    if expected_vault_id.is_some_and(|expected| expected != vault_id) {
        return Err(DIFFERENT_VAULT_ERROR.to_string());
    }
    ganbaru_atomic_files::create_private_directory(destination)
        .map_err(|error| format!("create restore staging folder: {error}"))?;
    let mut total_bytes = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut skipped = SkippedEntries::default();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("read backup entry {index}: {error}"))?;
        if entry.encrypted() {
            return Err("encrypted backup archives are not supported".to_string());
        }
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type != 0 && file_type != 0o040000 && file_type != 0o100000 {
                return Err("backup contains an unsupported special file".to_string());
            }
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or_else(|| "backup contains an unsafe entry path".to_string())?;
        let relative = safe_archive_path(&enclosed)?;
        if !is_bundle_layout_entry(&relative, entry.is_dir()) {
            skipped.push(relative);
            continue;
        }
        let output = destination.join(&relative);
        if output.exists() {
            return Err(format!(
                "backup contains duplicate entry '{}'",
                relative.display()
            ));
        }
        if entry.is_dir() {
            fs::create_dir(&output).map_err(|error| {
                format!(
                    "create restored directory '{}': {error}",
                    relative.display()
                )
            })?;
            continue;
        }
        if !entry.is_file() {
            return Err(format!(
                "backup entry '{}' is unsupported",
                relative.display()
            ));
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!("create restored parent '{}': {error}", relative.display())
            })?;
        }
        let mut target = fs::File::create(&output)
            .map_err(|error| format!("create restored file '{}': {error}", relative.display()))?;
        let declared_bytes = entry.size();
        let mut entry_bytes = 0_u64;
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|error| format!("read backup file '{}': {error}", relative.display()))?;
            if count == 0 {
                break;
            }
            entry_bytes = entry_bytes.saturating_add(count as u64);
            total_bytes = total_bytes.saturating_add(count as u64);
            if entry_bytes > declared_bytes {
                return Err(format!(
                    "backup file '{}' exceeds its declared size",
                    relative.display()
                ));
            }
            if total_bytes > max_bytes {
                return Err("backup exceeds the restore size limit".to_string());
            }
            target.write_all(&buffer[..count]).map_err(|error| {
                format!("write restored file '{}': {error}", relative.display())
            })?;
        }
        target
            .sync_all()
            .map_err(|error| format!("sync restored file '{}': {error}", relative.display()))?;
    }
    Ok(skipped)
}
