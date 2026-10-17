use crate::models::{NoteHtmlArchiveSaveDto, NoteHtmlExportDiagnosticDto};
use crate::transfers::html_export::HtmlArchive;
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

pub fn write_archive(
    asset_root: &Path,
    path: &Path,
    archive: &mut HtmlArchive,
) -> Result<NoteHtmlArchiveSaveDto, String> {
    require_zip_extension(path)?;
    write_zip_archive(asset_root, path, archive)?;
    Ok(NoteHtmlArchiveSaveDto::saved(archive.dto.clone()))
}

fn write_zip_archive(
    asset_root: &Path,
    path: &Path,
    archive: &mut HtmlArchive,
) -> Result<(), String> {
    let temporary_path = temporary_zip_path(path)?;
    let result = write_zip_archive_inner(asset_root, &temporary_path, archive).and_then(|()| {
        fs::rename(&temporary_path, path).map_err(|e| format!("save HTML archive: {e}"))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn write_zip_archive_inner(
    asset_root: &Path,
    path: &Path,
    archive: &mut HtmlArchive,
) -> Result<(), String> {
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let file = fs::File::create(path).map_err(|e| format!("create HTML archive: {e}"))?;
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
        let source = asset_source_path(asset_root, &asset.source_path)?;
        match fs::read(&source) {
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
                    format!("Managed asset could not be read: {error}"),
                )),
        }
    }
    let file = writer
        .finish()
        .map_err(|e| format!("finish HTML archive: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("sync HTML archive: {e}"))?;
    Ok(())
}

fn asset_source_path(root: &Path, source_path: &str) -> Result<PathBuf, String> {
    if source_path.starts_with('/') || source_path.contains('\\') || source_path.contains("..") {
        return Err("asset path is not safe for export".to_string());
    }
    let path = Path::new(source_path);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("asset path is not safe for export".to_string());
    }
    Ok(root.join(path))
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

fn temporary_zip_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "HTML archive path has no file name".to_string())?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}
