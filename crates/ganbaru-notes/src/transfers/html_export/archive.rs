use crate::assets::files::{NOTES_FILE_MAX_BYTES, hex_hash};
use crate::assets::validate_managed_asset_path;
use crate::models::{NoteHtmlArchiveSaveDto, NoteHtmlExportAssetDto, NoteHtmlExportDiagnosticDto};
use crate::transfers::html_export::HtmlArchive;
use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped, require_directories_within};
use std::{
    fs,
    io::Write,
    path::{Component, Path},
};

/// Vault folder that holds managed assets.
const ASSETS_DIR: &str = "assets";

/// Writes `archive` to the zip file at `path`, reading exported managed assets from below
/// `vault_root`.
///
/// # Errors
///
/// Returns an error when the target is not a `.zip` file, an entry path is unsafe, or the
/// archive cannot be written. An asset that cannot be read or no longer matches its recorded
/// size and digest is left out with a warning diagnostic instead.
pub fn write_archive(
    vault_root: &Path,
    path: &Path,
    archive: &mut HtmlArchive,
) -> Result<NoteHtmlArchiveSaveDto, String> {
    require_zip_extension(path)?;
    write_zip_archive(vault_root, path, archive)?;
    Ok(NoteHtmlArchiveSaveDto::saved(archive.dto.clone()))
}

fn write_zip_archive(
    vault_root: &Path,
    path: &Path,
    archive: &mut HtmlArchive,
) -> Result<(), String> {
    ganbaru_atomic_files::write_file_atomically_with(
        path,
        ganbaru_atomic_files::FileAccess::Default,
        |file| write_zip_archive_inner(vault_root, file, archive).map_err(std::io::Error::other),
    )
    .map_err(|e| format!("save HTML archive: {e}"))
}

fn write_zip_archive_inner(
    vault_root: &Path,
    file: &mut fs::File,
    archive: &mut HtmlArchive,
) -> Result<(), String> {
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(file);
    for file in &archive.dto.files {
        validate_archive_entry_path(&file.path)?;
        writer
            .start_file(&file.path, options)
            .map_err(|e| format!("write HTML archive entry: {e}"))?;
        writer
            .write_all(file.contents.as_bytes())
            .map_err(|e| format!("write HTML archive entry: {e}"))?;
    }
    for asset in &archive.dto.assets {
        if !asset.exported {
            continue;
        }
        validate_archive_entry_path(&asset.archive_path)?;
        match read_exported_asset(vault_root, asset) {
            Ok(bytes) => {
                writer
                    .start_file(&asset.archive_path, options)
                    .map_err(|e| format!("write HTML archive asset: {e}"))?;
                writer
                    .write_all(&bytes)
                    .map_err(|e| format!("write HTML archive asset: {e}"))?;
            }
            Err(error) => archive
                .dto
                .diagnostics
                .push(NoteHtmlExportDiagnosticDto::new(
                    "html_export_asset_read_failed",
                    "warning",
                    None::<String>,
                    None::<String>,
                    Some(asset.id.clone()),
                    None::<String>,
                    error,
                )),
        }
    }
    writer
        .finish()
        .map_err(|e| format!("finish HTML archive: {e}"))?;
    Ok(())
}

/// Reads one managed asset for export. The path must name a managed Notes asset directory,
/// no folder between the vault and the file may be a symlink, the file itself must be a regular
/// file, and its bytes must match the recorded size and SHA-256, so a planted link or swapped
/// file cannot place other local data in the archive.
fn read_exported_asset(
    vault_root: &Path,
    asset: &NoteHtmlExportAssetDto,
) -> Result<Vec<u8>, String> {
    validate_managed_asset_path(&asset.source_path)
        .map_err(|error| format!("Managed asset path is not exportable: {error}"))?;
    let source = vault_root.join(ASSETS_DIR).join(&asset.source_path);
    let folder = source
        .parent()
        .ok_or_else(|| "Managed asset path is not exportable".to_string())?;
    require_directories_within(vault_root, folder).map_err(|error| {
        format!("Managed asset folder is not a folder inside the vault: {error}")
    })?;
    let recorded_size = u64::try_from(asset.byte_size)
        .ok()
        .filter(|size| *size <= NOTES_FILE_MAX_BYTES as u64)
        .ok_or_else(|| "Managed asset has an invalid recorded size".to_string())?;
    let bytes = read_regular_file_capped(&source, recorded_size, FinalSymlink::Refuse)
        .map_err(|error| format!("Managed asset could not be read: {error}"))?;
    if bytes.len() as u64 != recorded_size || hex_hash(&bytes) != asset.sha256 {
        return Err("Managed asset no longer matches its recorded size and digest".to_string());
    }
    Ok(bytes)
}

fn validate_archive_entry_path(path: &str) -> Result<(), String> {
    if path.starts_with('/') || path.contains('\\') || path.contains("..") {
        return Err("archive entry path is not safe".to_string());
    }
    if Path::new(path)
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("archive entry path is not safe".to_string());
    }
    Ok(())
}

fn require_zip_extension(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        Ok(())
    } else {
        Err("HTML archive export path must end in .zip".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NoteHtmlExportDto;
    use std::time::{SystemTime, UNIX_EPOCH};

    const ASSET_ID: &str = "11111111-1111-4111-8111-111111111111";

    fn temp_vault() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ganbaru-notes-html-archive-{nonce}"));
        fs::create_dir_all(base.join("vault")).unwrap();
        base
    }

    fn exported_asset(source_path: &str, bytes: &[u8]) -> NoteHtmlExportAssetDto {
        NoteHtmlExportAssetDto {
            id: ASSET_ID.to_string(),
            archive_path: format!("assets/{source_path}"),
            source_path: source_path.to_string(),
            content_type: "application/pdf".to_string(),
            byte_size: bytes.len() as i64,
            sha256: hex_hash(bytes),
            storage_state: "available".to_string(),
            exported: true,
        }
    }

    fn archive_with(assets: Vec<NoteHtmlExportAssetDto>) -> HtmlArchive {
        HtmlArchive {
            dto: NoteHtmlExportDto {
                object: "html_export",
                root_page_id: ASSET_ID.to_string(),
                files: Vec::new(),
                assets,
                diagnostics: Vec::new(),
                manifest_json: "{}".to_string(),
                exported_page_count: 0,
                exported_block_count: 0,
                exported_asset_count: 0,
                exported_comment_count: 0,
                exported_database_view_count: 0,
            },
            default_file_name: "export.zip".to_string(),
        }
    }

    fn archive_entries(path: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().unwrap().to_string())
            .collect()
    }

    #[test]
    fn exported_assets_must_match_their_recorded_size_and_digest() {
        let base = temp_vault();
        let vault = base.join("vault");
        let files = vault.join("assets/notes/files");
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("kept.pdf"), b"%PDF-1.7 kept").unwrap();
        fs::write(files.join("swapped.pdf"), b"%PDF-1.7 swapped later").unwrap();
        let mut swapped = exported_asset("notes/files/swapped.pdf", b"%PDF-1.7 original");
        swapped.byte_size = b"%PDF-1.7 swapped later".len() as i64;
        let mut archive = archive_with(vec![
            exported_asset("notes/files/kept.pdf", b"%PDF-1.7 kept"),
            swapped,
            exported_asset("profile/avatar.png", b"not a Notes asset"),
        ]);
        let target = base.join("export.zip");

        write_archive(&vault, &target, &mut archive).unwrap();

        assert_eq!(archive_entries(&target), ["assets/notes/files/kept.pdf"]);
        let messages = archive
            .dto
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(messages.len(), 2);
        assert_eq!(
            messages[0],
            "Managed asset no longer matches its recorded size and digest"
        );
        assert!(messages[1].starts_with("Managed asset path is not exportable"));
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exported_assets_never_follow_a_symlinked_folder_or_file() {
        let base = temp_vault();
        let vault = base.join("vault");
        let outside = base.join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(vault.join("assets/notes/page-covers")).unwrap();
        let secret = b"private key material";
        fs::write(outside.join("secret.pdf"), secret).unwrap();
        fs::write(outside.join("cover.png"), secret).unwrap();
        std::os::unix::fs::symlink(&outside, vault.join("assets/notes/files")).unwrap();
        std::os::unix::fs::symlink(
            outside.join("cover.png"),
            vault.join("assets/notes/page-covers/cover.png"),
        )
        .unwrap();
        let mut archive = archive_with(vec![
            exported_asset("notes/files/secret.pdf", secret),
            exported_asset("notes/page-covers/cover.png", secret),
        ]);
        let target = base.join("export.zip");

        write_archive(&vault, &target, &mut archive).unwrap();

        assert!(archive_entries(&target).is_empty());
        assert_eq!(archive.dto.diagnostics.len(), 2);
        assert!(
            archive
                .dto
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code == "html_export_asset_read_failed")
        );
        fs::remove_dir_all(base).unwrap();
    }
}
