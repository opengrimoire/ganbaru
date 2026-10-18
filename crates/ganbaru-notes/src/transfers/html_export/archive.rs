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
    ganbaru_atomic_files::write_file_atomically_with(
        path,
        ganbaru_atomic_files::FileAccess::Default,
        |file| write_zip_archive_inner(asset_root, file, archive).map_err(std::io::Error::other),
    )
    .map_err(|e| format!("save HTML archive: {e}"))
}

fn write_zip_archive_inner(
    asset_root: &Path,
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
    writer
        .finish()
        .map_err(|e| format!("finish HTML archive: {e}"))?;
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
