//! Track artwork discovery from sidecar images and embedded ID3, FLAC, and MP4 cover art.

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    fs::{self, File},
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_EMBEDDED_ARTWORK_BYTES: usize = 24 * 1024 * 1024;
const MAX_TAG_BYTES: usize = 64 * 1024 * 1024;
const IMAGE_EXTENSIONS: &[&str] = &["avif", "bmp", "gif", "jpeg", "jpg", "png", "webp"];

/// Validated cover art bytes extracted from a media file.
pub struct EmbeddedArtwork {
    /// Sniffed image media type.
    pub content_type: String,
    /// Raw image bytes within the embedded artwork size limit.
    pub bytes: Vec<u8>,
}

type ArtworkScore = (u8, u8, u8, String);
type ArtworkCandidate = (ArtworkScore, PathBuf);

fn is_supported_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

/// Finds the best sidecar image for a track in its folder, common artwork subfolders, or parent
/// folders up to `root`.
pub fn find_track_artwork(
    track_path: &Path,
    root: &Path,
    artwork_cache: &mut HashMap<PathBuf, Vec<PathBuf>>,
) -> Option<PathBuf> {
    let mut dir = track_path.parent()?;
    let mut distance = 0_u8;
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut best: Option<ArtworkCandidate> = None;
    let track_stem = normalized_artwork_stem(track_path);

    loop {
        consider_artwork_dir(dir, distance, 0, &track_stem, artwork_cache, &mut best);
        for (subdir_distance, name) in [
            "cover", "covers", "artwork", "art", "scans", "scan", "booklet",
        ]
        .iter()
        .enumerate()
        {
            let candidate_dir = dir.join(name);
            if candidate_dir.is_dir() {
                consider_artwork_dir(
                    &candidate_dir,
                    distance,
                    (subdir_distance + 1) as u8,
                    &track_stem,
                    artwork_cache,
                    &mut best,
                );
            }
        }

        let current = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if current == root {
            break;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent;
        distance = distance.saturating_add(1);
    }

    best.map(|(_, path)| path)
}

fn consider_artwork_dir(
    dir: &Path,
    distance: u8,
    subdir_distance: u8,
    track_stem: &str,
    artwork_cache: &mut HashMap<PathBuf, Vec<PathBuf>>,
    best: &mut Option<ArtworkCandidate>,
) {
    let candidates = artwork_cache
        .entry(dir.to_path_buf())
        .or_insert_with(|| find_directory_artwork_candidates(dir));
    let Some((score, path)) = candidates
        .iter()
        .map(|path| {
            let (rank, name) = artwork_rank_for_track(path, track_stem);
            ((rank, distance, subdir_distance, name), path)
        })
        .min_by(|(left, _), (right, _)| left.cmp(right))
    else {
        return;
    };
    if best
        .as_ref()
        .is_none_or(|(best_score, _)| score < *best_score)
    {
        *best = Some((score, path.clone()));
    }
}

fn find_directory_artwork_candidates(dir: &Path) -> Vec<PathBuf> {
    let Some(entries) = fs::read_dir(dir).ok() else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_file() || !is_supported_image_path(&path) {
            continue;
        }
        candidates.push(path);
    }
    candidates
}

fn artwork_rank_for_track(path: &Path, track_stem: &str) -> (u8, String) {
    let stem = normalized_artwork_stem(path);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let priority = if !track_stem.is_empty() && stem == track_stem {
        0
    } else if stem == "cover" {
        1
    } else if stem == "folder" {
        2
    } else if stem == "front" {
        3
    } else if stem == "albumart" || stem == "album" {
        4
    } else if !track_stem.is_empty() && stem.contains(track_stem) {
        5
    } else if stem.contains("cover") || stem.contains("folder") || stem.contains("front") {
        6
    } else if stem.contains("albumart") || stem.contains("album") {
        7
    } else {
        8
    };
    (priority, file_name)
}

fn normalized_artwork_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace([' ', '_', '-', '.'], "")
}

/// Returns a stable identifier for embedded artwork from its path and bytes.
pub fn embedded_artwork_id(path: &Path, artwork: &EmbeddedArtwork) -> String {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    artwork.content_type.hash(&mut hasher);
    artwork.bytes.len().hash(&mut hasher);
    artwork.bytes.hash(&mut hasher);
    format!("artwork-{:x}", hasher.finish())
}

/// Extracts the preferred embedded cover from an ID3, FLAC, or MP4 file, if any.
pub fn extract_embedded_artwork(path: &Path) -> Result<Option<EmbeddedArtwork>, String> {
    let mut file =
        File::open(path).map_err(|e| format!("failed to open media file for artwork: {e}"))?;
    let mut header = [0_u8; 12];
    let header_len = file
        .read(&mut header)
        .map_err(|e| format!("failed to read media header for artwork: {e}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("failed to seek media file for artwork: {e}"))?;

    if header_len >= 10 && &header[..3] == b"ID3" {
        return extract_id3_artwork(&mut file);
    }
    if header_len >= 4 && &header[..4] == b"fLaC" {
        return extract_flac_artwork(&mut file);
    }
    if header_len >= 8 && &header[4..8] == b"ftyp" {
        return extract_mp4_artwork(&mut file);
    }
    Ok(None)
}

fn extract_id3_artwork(file: &mut File) -> Result<Option<EmbeddedArtwork>, String> {
    let mut header = [0_u8; 10];
    file.read_exact(&mut header)
        .map_err(|e| format!("failed to read ID3 header: {e}"))?;
    if &header[..3] != b"ID3" {
        return Ok(None);
    }
    let version = header[3];
    let tag_size =
        synchsafe_u32(&header[6..10]).ok_or_else(|| "invalid ID3 tag size".to_string())? as usize;
    if tag_size > MAX_TAG_BYTES {
        return Ok(None);
    }
    let mut tag = vec![0_u8; tag_size];
    file.read_exact(&mut tag)
        .map_err(|e| format!("failed to read ID3 tag: {e}"))?;
    if header[5] & 0x80 != 0 {
        tag = remove_id3_unsynchronization(&tag);
    }

    let frame_start = id3_frame_start(version, header[5], &tag);
    parse_id3_frames(version, &tag[frame_start..])
}

fn remove_id3_unsynchronization(bytes: &[u8]) -> Vec<u8> {
    let mut clean = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0xff && bytes.get(index + 1) == Some(&0) {
            clean.push(0xff);
            index += 2;
            continue;
        }
        clean.push(bytes[index]);
        index += 1;
    }
    clean
}

fn id3_frame_start(version: u8, flags: u8, tag: &[u8]) -> usize {
    if flags & 0x40 == 0 || tag.len() < 4 {
        return 0;
    }
    if version == 4 {
        let Some(size) = synchsafe_u32(&tag[..4]) else {
            return 0;
        };
        usize::try_from(size).unwrap_or(0).min(tag.len())
    } else {
        let size = u32::from_be_bytes([tag[0], tag[1], tag[2], tag[3]]) as usize;
        size.saturating_add(4).min(tag.len())
    }
}

fn parse_id3_frames(version: u8, frames: &[u8]) -> Result<Option<EmbeddedArtwork>, String> {
    if version == 2 {
        return parse_id3v22_frames(frames);
    }

    let mut offset = 0;
    let mut fallback = None;
    while offset + 10 <= frames.len() {
        let id = &frames[offset..offset + 4];
        if id.iter().all(|byte| *byte == 0) {
            break;
        }
        let size = if version == 4 {
            synchsafe_u32(&frames[offset + 4..offset + 8]).unwrap_or(0)
        } else {
            u32::from_be_bytes([
                frames[offset + 4],
                frames[offset + 5],
                frames[offset + 6],
                frames[offset + 7],
            ])
        } as usize;
        offset += 10;
        if size == 0 || offset + size > frames.len() {
            break;
        }
        if id == b"APIC" {
            let Some((picture_type, artwork)) = parse_apic_frame(&frames[offset..offset + size])
            else {
                offset += size;
                continue;
            };
            if picture_type == 3 {
                return Ok(Some(artwork));
            }
            fallback.get_or_insert(artwork);
        }
        offset += size;
    }
    Ok(fallback)
}

fn parse_id3v22_frames(frames: &[u8]) -> Result<Option<EmbeddedArtwork>, String> {
    let mut offset = 0;
    let mut fallback = None;
    while offset + 6 <= frames.len() {
        let id = &frames[offset..offset + 3];
        if id.iter().all(|byte| *byte == 0) {
            break;
        }
        let size = ((frames[offset + 3] as usize) << 16)
            | ((frames[offset + 4] as usize) << 8)
            | frames[offset + 5] as usize;
        offset += 6;
        if size == 0 || offset + size > frames.len() {
            break;
        }
        if id == b"PIC" {
            let Some((picture_type, artwork)) = parse_pic_frame(&frames[offset..offset + size])
            else {
                offset += size;
                continue;
            };
            if picture_type == 3 {
                return Ok(Some(artwork));
            }
            fallback.get_or_insert(artwork);
        }
        offset += size;
    }
    Ok(fallback)
}

fn parse_apic_frame(frame: &[u8]) -> Option<(u8, EmbeddedArtwork)> {
    if frame.len() < 5 {
        return None;
    }
    let encoding = frame[0];
    let mime_end = frame[1..].iter().position(|byte| *byte == 0)? + 1;
    let mime = std::str::from_utf8(&frame[1..mime_end])
        .ok()?
        .to_ascii_lowercase();
    let picture_type = *frame.get(mime_end + 1)?;
    let description_start = mime_end + 2;
    let data_start =
        description_start + encoded_terminator_offset(encoding, &frame[description_start..])?;
    let content_type = normalize_artwork_content_type(&mime, &frame[data_start..])?;
    let bytes = frame[data_start..].to_vec();
    validate_artwork_bytes(&bytes)?;
    Some((
        picture_type,
        EmbeddedArtwork {
            content_type,
            bytes,
        },
    ))
}

fn parse_pic_frame(frame: &[u8]) -> Option<(u8, EmbeddedArtwork)> {
    if frame.len() < 6 {
        return None;
    }
    let encoding = frame[0];
    let format = std::str::from_utf8(&frame[1..4]).ok()?.to_ascii_lowercase();
    let picture_type = frame[4];
    let description_start = 5;
    let data_start =
        description_start + encoded_terminator_offset(encoding, &frame[description_start..])?;
    let mime = match format.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => format.as_str(),
    };
    let content_type = normalize_artwork_content_type(mime, &frame[data_start..])?;
    let bytes = frame[data_start..].to_vec();
    validate_artwork_bytes(&bytes)?;
    Some((
        picture_type,
        EmbeddedArtwork {
            content_type,
            bytes,
        },
    ))
}

fn encoded_terminator_offset(encoding: u8, value: &[u8]) -> Option<usize> {
    if encoding == 1 || encoding == 2 {
        let position = value
            .windows(2)
            .position(|window| window == [0, 0])
            .map(|index| index + 2)?;
        return Some(position);
    }
    value
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| index + 1)
}

fn extract_flac_artwork(file: &mut File) -> Result<Option<EmbeddedArtwork>, String> {
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature)
        .map_err(|e| format!("failed to read FLAC signature: {e}"))?;
    if &signature != b"fLaC" {
        return Ok(None);
    }

    let mut fallback = None;
    loop {
        let mut header = [0_u8; 4];
        if file.read_exact(&mut header).is_err() {
            break;
        }
        let is_last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let size = ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
        if size > MAX_TAG_BYTES {
            return Ok(None);
        }
        if block_type == 6 {
            let mut block = vec![0_u8; size];
            file.read_exact(&mut block)
                .map_err(|e| format!("failed to read FLAC picture block: {e}"))?;
            if let Some((picture_type, artwork)) = parse_flac_picture_block(&block) {
                if picture_type == 3 {
                    return Ok(Some(artwork));
                }
                fallback.get_or_insert(artwork);
            }
        } else {
            file.seek(SeekFrom::Current(size as i64))
                .map_err(|e| format!("failed to skip FLAC metadata block: {e}"))?;
        }
        if is_last {
            break;
        }
    }
    Ok(fallback)
}

fn parse_flac_picture_block(block: &[u8]) -> Option<(u32, EmbeddedArtwork)> {
    let mut offset = 0;
    let picture_type = read_be_u32(block, &mut offset)?;
    let mime_len = read_be_u32(block, &mut offset)? as usize;
    let mime = read_bytes(block, &mut offset, mime_len)?;
    let mime = std::str::from_utf8(mime).ok()?.to_ascii_lowercase();
    let description_len = read_be_u32(block, &mut offset)? as usize;
    read_bytes(block, &mut offset, description_len)?;
    for _ in 0..4 {
        read_be_u32(block, &mut offset)?;
    }
    let data_len = read_be_u32(block, &mut offset)? as usize;
    let data = read_bytes(block, &mut offset, data_len)?;
    let content_type = normalize_artwork_content_type(&mime, data)?;
    let bytes = data.to_vec();
    validate_artwork_bytes(&bytes)?;
    Some((
        picture_type,
        EmbeddedArtwork {
            content_type,
            bytes,
        },
    ))
}

fn extract_mp4_artwork(file: &mut File) -> Result<Option<EmbeddedArtwork>, String> {
    let len = file
        .metadata()
        .map_err(|e| format!("failed to inspect MP4 file for artwork: {e}"))?
        .len();
    parse_mp4_atoms(file, 0, len, 0)
}

fn parse_mp4_atoms(
    file: &mut File,
    start: u64,
    end: u64,
    depth: u8,
) -> Result<Option<EmbeddedArtwork>, String> {
    if depth > 8 {
        return Ok(None);
    }

    let mut offset = start;
    while offset.saturating_add(8) <= end {
        let header = read_file_bytes(file, offset, 8)?;
        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
        let atom_type = [header[4], header[5], header[6], header[7]];
        let (atom_size, header_size) = if size32 == 1 {
            let large_size = read_file_bytes(file, offset + 8, 8)?;
            (
                u64::from_be_bytes([
                    large_size[0],
                    large_size[1],
                    large_size[2],
                    large_size[3],
                    large_size[4],
                    large_size[5],
                    large_size[6],
                    large_size[7],
                ]),
                16,
            )
        } else if size32 == 0 {
            (end.saturating_sub(offset), 8)
        } else {
            (u64::from(size32), 8)
        };
        if atom_size < header_size || offset.saturating_add(atom_size) > end {
            break;
        }

        let content_start = offset + header_size;
        let content_end = offset + atom_size;
        match &atom_type {
            b"moov" | b"udta" | b"ilst" => {
                if let Some(artwork) = parse_mp4_atoms(file, content_start, content_end, depth + 1)?
                {
                    return Ok(Some(artwork));
                }
            }
            b"meta" => {
                if content_start.saturating_add(4) <= content_end {
                    if let Some(artwork) =
                        parse_mp4_atoms(file, content_start + 4, content_end, depth + 1)?
                    {
                        return Ok(Some(artwork));
                    }
                }
            }
            b"covr" => {
                if let Some(artwork) = parse_mp4_cover_atom(file, content_start, content_end)? {
                    return Ok(Some(artwork));
                }
            }
            _ => {}
        }

        offset += atom_size;
    }
    Ok(None)
}

fn parse_mp4_cover_atom(
    file: &mut File,
    start: u64,
    end: u64,
) -> Result<Option<EmbeddedArtwork>, String> {
    let mut offset = start;
    while offset.saturating_add(16) <= end {
        let header = read_file_bytes(file, offset, 8)?;
        let atom_size = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let atom_type = [header[4], header[5], header[6], header[7]];
        if atom_size < 16 || offset.saturating_add(atom_size) > end {
            break;
        }
        if &atom_type == b"data" {
            let data_header = read_file_bytes(file, offset + 8, 8)?;
            let data_type = u32::from_be_bytes([
                data_header[0],
                data_header[1],
                data_header[2],
                data_header[3],
            ]);
            let payload_start = offset + 16;
            let payload_len = atom_size - 16;
            if payload_len > MAX_EMBEDDED_ARTWORK_BYTES as u64 {
                return Ok(None);
            }
            let bytes = read_file_bytes(file, payload_start, payload_len as usize)?;
            if validate_artwork_bytes(&bytes).is_none() {
                return Ok(None);
            }
            let content_type = match data_type {
                13 => "image/jpeg".to_string(),
                14 => "image/png".to_string(),
                _ => sniff_image_content_type(&bytes)
                    .unwrap_or("application/octet-stream")
                    .to_string(),
            };
            if content_type == "application/octet-stream" {
                return Ok(None);
            }
            return Ok(Some(EmbeddedArtwork {
                content_type,
                bytes,
            }));
        }
        offset += atom_size;
    }
    Ok(None)
}

fn read_file_bytes(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0_u8; len];
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("failed to seek metadata bytes: {e}"))?;
    file.read_exact(&mut bytes)
        .map_err(|e| format!("failed to read metadata bytes: {e}"))?;
    Ok(bytes)
}

fn read_be_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = (*offset).checked_add(4)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(u32::from_be_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_bytes<'a>(bytes: &'a [u8], offset: &mut usize, len: usize) -> Option<&'a [u8]> {
    let end = (*offset).checked_add(len)?;
    let value = bytes.get(*offset..end)?;
    *offset = end;
    Some(value)
}

fn synchsafe_u32(bytes: &[u8]) -> Option<u32> {
    if bytes.len() != 4 || bytes.iter().any(|byte| byte & 0x80 != 0) {
        return None;
    }
    Some(
        ((bytes[0] as u32) << 21)
            | ((bytes[1] as u32) << 14)
            | ((bytes[2] as u32) << 7)
            | bytes[3] as u32,
    )
}

fn normalize_artwork_content_type(mime: &str, bytes: &[u8]) -> Option<String> {
    let normalized = match mime {
        "image/jpg" | "image/jpeg" => "image/jpeg",
        "image/png" => "image/png",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        "image/bmp" => "image/bmp",
        "image/avif" => "image/avif",
        "" => sniff_image_content_type(bytes)?,
        _ => sniff_image_content_type(bytes)?,
    };
    Some(normalized.to_string())
}

fn sniff_image_content_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

fn validate_artwork_bytes(bytes: &[u8]) -> Option<()> {
    if bytes.is_empty() || bytes.len() > MAX_EMBEDDED_ARTWORK_BYTES {
        None
    } else {
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn artwork_rank_prefers_common_cover_names() {
        assert!(
            artwork_rank_for_track(Path::new("/music/cover.jpg"), "")
                < artwork_rank_for_track(Path::new("/music/random.png"), "")
        );
        assert!(
            artwork_rank_for_track(Path::new("/music/folder.png"), "")
                < artwork_rank_for_track(Path::new("/music/front.png"), "")
        );
    }

    #[test]
    fn artwork_lookup_uses_parent_album_front_image() {
        let root = unique_temporary_dir("ganbaru-ai-artwork-parent");
        let album_dir = root.join("Anime/Made in Abyss/2017 - Made in Abyss OST");
        let disc_dir = album_dir.join("CD 1");
        fs::create_dir_all(&disc_dir).unwrap();
        let track = disc_dir.join("01 - Made in Abyss.mp3");
        let artwork = album_dir.join("01-MIA-FRONT.jpg");
        fs::write(&track, []).unwrap();
        fs::write(disc_dir.join("booklet-page.jpg"), []).unwrap();
        fs::write(&artwork, []).unwrap();

        let mut cache = HashMap::new();
        assert_eq!(find_track_artwork(&track, &root, &mut cache), Some(artwork));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_lookup_prefers_matching_sidecar_image() {
        let root = unique_temporary_dir("ganbaru-ai-artwork-sidecar");
        fs::create_dir_all(&root).unwrap();
        let track = root.join("02 - Focus.mp3");
        let sidecar = root.join("02 - Focus.jpg");
        fs::write(&track, []).unwrap();
        fs::write(root.join("01 - Intro.jpg"), []).unwrap();
        fs::write(&sidecar, []).unwrap();

        let mut cache = HashMap::new();
        assert_eq!(find_track_artwork(&track, &root, &mut cache), Some(sidecar));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apic_frame_parser_extracts_front_cover() {
        let mut frame = Vec::new();
        frame.push(0);
        frame.extend_from_slice(b"image/jpeg\0");
        frame.push(3);
        frame.push(0);
        frame.extend_from_slice(&[0xff, 0xd8, 0xff, 0xdb]);

        let Some((picture_type, artwork)) = parse_apic_frame(&frame) else {
            panic!("expected APIC artwork");
        };

        assert_eq!(picture_type, 3);
        assert_eq!(artwork.content_type, "image/jpeg");
        assert_eq!(artwork.bytes, vec![0xff, 0xd8, 0xff, 0xdb]);
    }

    #[test]
    fn apic_frame_parser_rejects_oversized_artwork_without_rejecting_the_track() {
        let mut frame = Vec::new();
        frame.push(0);
        frame.extend_from_slice(b"image/jpeg\0");
        frame.push(3);
        frame.push(0);
        frame.extend_from_slice(&[0xff, 0xd8, 0xff]);
        frame.resize(24 * 1024 * 1024 + 32, 0);

        assert!(parse_apic_frame(&frame).is_none());
    }

    #[test]
    fn id3_unsynchronization_removes_inserted_zero_bytes() {
        assert_eq!(
            remove_id3_unsynchronization(&[0xff, 0x00, 0xe0, 0x11]),
            vec![0xff, 0xe0, 0x11]
        );
    }

    #[test]
    fn flac_picture_block_parser_extracts_front_cover() {
        let mut block = Vec::new();
        push_be_u32(&mut block, 3);
        push_be_u32(&mut block, 10);
        block.extend_from_slice(b"image/jpeg");
        push_be_u32(&mut block, 0);
        push_be_u32(&mut block, 1);
        push_be_u32(&mut block, 1);
        push_be_u32(&mut block, 24);
        push_be_u32(&mut block, 0);
        push_be_u32(&mut block, 4);
        block.extend_from_slice(&[0xff, 0xd8, 0xff, 0xdb]);

        let Some((picture_type, artwork)) = parse_flac_picture_block(&block) else {
            panic!("expected FLAC artwork");
        };

        assert_eq!(picture_type, 3);
        assert_eq!(artwork.content_type, "image/jpeg");
        assert_eq!(artwork.bytes, vec![0xff, 0xd8, 0xff, 0xdb]);
    }

    #[test]
    fn mp4_cover_atom_parser_extracts_cover_art() {
        let root = unique_temporary_dir("ganbaru-ai-artwork-mp4");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("theme.m4a");
        let mut data_content = Vec::new();
        push_be_u32(&mut data_content, 13);
        push_be_u32(&mut data_content, 0);
        data_content.extend_from_slice(&[0xff, 0xd8, 0xff, 0xdb]);
        let data = mp4_atom(*b"data", &data_content);
        let covr = mp4_atom(*b"covr", &data);
        let ilst = mp4_atom(*b"ilst", &covr);
        let mut meta_content = vec![0, 0, 0, 0];
        meta_content.extend_from_slice(&ilst);
        let meta = mp4_atom(*b"meta", &meta_content);
        let udta = mp4_atom(*b"udta", &meta);
        let moov = mp4_atom(*b"moov", &udta);
        let ftyp = mp4_atom(*b"ftyp", b"M4A \0\0\0\0M4A ");
        let mut file = ftyp;
        file.extend_from_slice(&moov);
        fs::write(&path, file).unwrap();

        let artwork = extract_embedded_artwork(&path).unwrap().unwrap();

        assert_eq!(artwork.content_type, "image/jpeg");
        assert_eq!(artwork.bytes, vec![0xff, 0xd8, 0xff, 0xdb]);
        fs::remove_dir_all(root).unwrap();
    }

    fn push_be_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn mp4_atom(name: [u8; 4], content: &[u8]) -> Vec<u8> {
        let size = u32::try_from(content.len() + 8).unwrap();
        let mut atom = size.to_be_bytes().to_vec();
        atom.extend_from_slice(&name);
        atom.extend_from_slice(content);
        atom
    }

    fn unique_temporary_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nanos}"))
    }
}
