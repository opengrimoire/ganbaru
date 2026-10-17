use rodio::{Decoder, Source};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, ReadDir};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::artwork::{extract_embedded_artwork, find_track_artwork};

use super::super::MusicMediaKind;
use super::metadata::{LocalTags, read_container_duration_ms, read_tags};

pub(super) const DISCOVERY_BATCH_SIZE: usize = 128;
pub(super) const RECONCILE_BATCH_SIZE: i64 = 256;
const FINGERPRINT_SAMPLE_BYTES: usize = 64 * 1024;

pub(crate) type ArtworkCache = HashMap<PathBuf, Vec<PathBuf>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DiscoveredEntryKind {
    Directory,
    Media,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiscoveredEntry {
    pub relative_path: String,
    pub kind: DiscoveredEntryKind,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LocalMediaEvidence {
    pub relative_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: Option<i64>,
    pub duration_ms: Option<i64>,
    pub media_kind: MusicMediaKind,
    pub original_artwork_identity: Option<String>,
    pub file_size_bytes: i64,
    pub modified_at_ms: Option<i64>,
    pub lightweight_fingerprint: String,
    pub metadata_issue: Option<String>,
}

pub(super) fn open_directory(root: &Path, relative_path: &str) -> Result<ReadDir, String> {
    let directory = join_relative(root, relative_path)?;
    fs::read_dir(&directory)
        .map_err(|error| format!("cannot read '{}': {error}", display_relative(relative_path)))
}

pub(super) fn next_discovery_batch(
    root: &Path,
    reader: &mut ReadDir,
) -> Result<Vec<DiscoveredEntry>, String> {
    let mut entries = Vec::with_capacity(DISCOVERY_BATCH_SIZE);
    while entries.len() < DISCOVERY_BATCH_SIZE {
        let Some(entry) = reader.next() else {
            break;
        };
        let entry = entry.map_err(|error| format!("cannot read a folder entry: {error}"))?;
        let path = entry.path();
        let relative_path = normalized_relative_path(root, &path)?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect '{relative_path}': {error}"))?;
        let kind = if file_type.is_symlink() {
            DiscoveredEntryKind::Skipped
        } else if file_type.is_dir() {
            DiscoveredEntryKind::Directory
        } else if file_type.is_file() && crate::media::is_supported_media_path(&path) {
            DiscoveredEntryKind::Media
        } else {
            DiscoveredEntryKind::Skipped
        };
        entries.push(DiscoveredEntry {
            relative_path,
            kind,
        });
    }
    entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(entries)
}

pub(crate) fn inspect_media(
    root: &Path,
    relative_path: &str,
    artwork_cache: &mut ArtworkCache,
) -> Result<LocalMediaEvidence, String> {
    let path = join_relative(root, relative_path)?;
    let metadata = fs::metadata(&path)
        .map_err(|error| format!("cannot inspect '{relative_path}': {error}"))?;
    if !metadata.is_file() {
        return Err(format!("'{relative_path}' is no longer a file"));
    }
    let file_size_bytes = i64::try_from(metadata.len())
        .map_err(|_| format!("'{relative_path}' is too large to catalog"))?;
    let modified_at_ms = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_millis()).ok());
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let media_kind = media_kind(&extension);
    let mut metadata_issues = Vec::new();
    let duration_ms = match duration_ms(&path, media_kind, &extension) {
        Ok(duration) => duration,
        Err(error) => {
            metadata_issues.push(error);
            None
        }
    };
    let tags = match read_tags(&path, &extension) {
        Ok(tags) => tags,
        Err(error) => {
            metadata_issues.push(error);
            LocalTags::default()
        }
    };
    let embedded_artwork = match extract_embedded_artwork(&path) {
        Ok(artwork) => artwork,
        Err(error) => {
            metadata_issues.push(error);
            None
        }
    };
    let original_artwork_identity = match embedded_artwork {
        Some(artwork) => {
            let mut hasher = Sha256::new();
            hasher.update(artwork.content_type.as_bytes());
            hasher.update(&artwork.bytes);
            Some(format!("embedded:sha256:{:x}", hasher.finalize()))
        }
        None => find_track_artwork(&path, root, artwork_cache)
            .map(|artwork_path| normalized_relative_path(root, &artwork_path))
            .transpose()?
            .map(|relative| format!("sidecar:{relative}")),
    };
    Ok(LocalMediaEvidence {
        relative_path: relative_path.to_string(),
        title: tags.title.unwrap_or_else(|| fallback_title(&path)),
        artist: tags.artist.unwrap_or_default(),
        album: tags.album.unwrap_or_default(),
        track_number: tags.track_number,
        duration_ms,
        media_kind,
        original_artwork_identity,
        file_size_bytes,
        modified_at_ms,
        lightweight_fingerprint: lightweight_fingerprint(&path, metadata.len())?,
        metadata_issue: (!metadata_issues.is_empty()).then(|| metadata_issues.join(" ")),
    })
}

pub(crate) fn strong_fingerprint(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("cannot open media: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash media: {error}"))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub(super) fn join_relative(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    if relative_path.is_empty() {
        return Ok(root.to_path_buf());
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("refresh path escapes its logical root".to_string());
    }
    Ok(root.join(path))
}

fn normalized_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "discovered path escaped its logical root".to_string())?;
    let components = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| "a media path is not valid UTF-8".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(components.join("/"))
}

fn lightweight_fingerprint(path: &Path, file_size: u64) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("cannot open media: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(file_size.to_le_bytes());
    let sample_size = usize::try_from(file_size.min(FINGERPRINT_SAMPLE_BYTES as u64))
        .map_err(|_| "media sample size is unsupported".to_string())?;
    let mut sample = vec![0_u8; sample_size];
    if sample_size > 0 {
        file.read_exact(&mut sample)
            .map_err(|error| format!("cannot read media fingerprint: {error}"))?;
        hasher.update(&sample);
    }
    if file_size > FINGERPRINT_SAMPLE_BYTES as u64 {
        file.seek(SeekFrom::End(-(FINGERPRINT_SAMPLE_BYTES as i64)))
            .map_err(|error| format!("cannot seek media fingerprint: {error}"))?;
        let mut tail = vec![0_u8; FINGERPRINT_SAMPLE_BYTES];
        file.read_exact(&mut tail)
            .map_err(|error| format!("cannot read media fingerprint tail: {error}"))?;
        hasher.update(&tail);
    }
    Ok(format!("sample-sha256:{:x}", hasher.finalize()))
}

fn duration_ms(path: &Path, kind: MusicMediaKind, extension: &str) -> Result<Option<i64>, String> {
    let file = File::open(path).map_err(|error| format!("metadata open failed: {error}"))?;
    let decoder = match Decoder::try_from(file) {
        Ok(decoder) => decoder,
        Err(_) if kind == MusicMediaKind::Video => {
            return read_container_duration_ms(path, extension);
        }
        Err(error) => {
            return Err(format!("audio metadata could not be decoded: {error}"));
        }
    };
    let decoded = decoder
        .total_duration()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok());
    if decoded.is_none() && kind == MusicMediaKind::Video {
        return read_container_duration_ms(path, extension);
    }
    Ok(decoded)
}

fn media_kind(extension: &str) -> MusicMediaKind {
    match extension {
        "avi" | "flv" | "m4v" | "mkv" | "mov" | "mp4" | "mpeg" | "mpg" | "ogv" | "webm" | "wmv" => {
            MusicMediaKind::Video
        }
        "aac" | "aif" | "aiff" | "alac" | "ape" | "flac" | "m4a" | "mp3" | "ogg" | "opus"
        | "wav" | "wma" => MusicMediaKind::Audio,
        _ => MusicMediaKind::Unknown,
    }
}

fn fallback_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Untitled media")
        .to_string()
}

fn display_relative(relative_path: &str) -> &str {
    if relative_path.is_empty() {
        "the selected root"
    } else {
        relative_path
    }
}
