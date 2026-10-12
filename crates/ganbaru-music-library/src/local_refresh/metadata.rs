use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const MAX_TAG_BYTES: usize = 16 * 1024 * 1024;
const MAX_COMMENT_SCAN_BYTES: usize = 2 * 1024 * 1024;
const MAX_MP4_METADATA_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct LocalTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i64>,
}

pub(super) fn read_tags(path: &Path, extension: &str) -> Result<LocalTags, String> {
    let mut header = [0_u8; 12];
    let mut file = File::open(path).map_err(|error| format!("tag open failed: {error}"))?;
    let header_len = file
        .read(&mut header)
        .map_err(|error| format!("tag header read failed: {error}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("tag seek failed: {error}"))?;
    if header_len >= 10 && &header[..3] == b"ID3" {
        return read_id3v2(&mut file);
    }
    if header_len >= 4 && &header[..4] == b"fLaC" {
        return read_flac(&mut file);
    }
    if header_len >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE" {
        return read_wav_info(&mut file);
    }
    if header_len >= 8 && &header[4..8] == b"ftyp" {
        return read_mp4(&mut file);
    }
    if matches!(extension, "ogg" | "opus") || (header_len >= 4 && &header[..4] == b"OggS") {
        return read_ogg_comments(&mut file);
    }
    Ok(LocalTags::default())
}

pub(super) fn read_container_duration_ms(
    path: &Path,
    extension: &str,
) -> Result<Option<i64>, String> {
    if !matches!(extension, "m4a" | "m4v" | "mov" | "mp4") {
        return Ok(None);
    }
    let mut file =
        File::open(path).map_err(|error| format!("MP4 duration open failed: {error}"))?;
    let length = file
        .metadata()
        .map_err(|error| format!("MP4 duration inspection failed: {error}"))?
        .len()
        .min(MAX_MP4_METADATA_BYTES as u64) as usize;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("MP4 duration read failed: {error}"))?;
    find_mp4_duration(&bytes, 0, bytes.len(), 0)
}

fn read_id3v2(file: &mut File) -> Result<LocalTags, String> {
    let mut header = [0_u8; 10];
    file.read_exact(&mut header)
        .map_err(|error| format!("ID3 header is malformed: {error}"))?;
    let version = header[3];
    if !(2..=4).contains(&version) {
        return Err(format!("ID3 version {version} is unsupported"));
    }
    let tag_size = synchsafe_u32(&header[6..10])? as usize;
    if tag_size > MAX_TAG_BYTES {
        return Err("ID3 metadata exceeds the safe read limit".to_string());
    }
    let mut bytes = vec![0_u8; tag_size];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("ID3 metadata is truncated: {error}"))?;
    let mut tags = LocalTags::default();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let (name, size, header_size) = if version == 2 {
            if offset + 6 > bytes.len() || bytes[offset..offset + 3] == [0, 0, 0] {
                break;
            }
            let name = &bytes[offset..offset + 3];
            let size = ((bytes[offset + 3] as usize) << 16)
                | ((bytes[offset + 4] as usize) << 8)
                | bytes[offset + 5] as usize;
            (name, size, 6)
        } else {
            if offset + 10 > bytes.len() || bytes[offset..offset + 4] == [0, 0, 0, 0] {
                break;
            }
            let name = &bytes[offset..offset + 4];
            let size = if version == 4 {
                synchsafe_u32(&bytes[offset + 4..offset + 8])? as usize
            } else {
                u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize
            };
            (name, size, 10)
        };
        let content_start = offset + header_size;
        let content_end = content_start.saturating_add(size);
        if content_end > bytes.len() {
            return Err("ID3 frame extends past the metadata boundary".to_string());
        }
        let content = &bytes[content_start..content_end];
        let text = || decode_id3_text(content);
        match name {
            b"TIT2" | b"TT2" => set_if_text(&mut tags.title, text()?),
            b"TPE1" | b"TP1" => set_if_text(&mut tags.artist, text()?),
            b"TALB" | b"TAL" => set_if_text(&mut tags.album, text()?),
            b"TRCK" | b"TRK" => tags.track_number = text()?.as_deref().and_then(parse_track),
            _ => {}
        }
        offset = content_end;
    }
    Ok(tags)
}

fn read_flac(file: &mut File) -> Result<LocalTags, String> {
    file.seek(SeekFrom::Start(4))
        .map_err(|error| format!("FLAC metadata seek failed: {error}"))?;
    let mut total = 0_usize;
    loop {
        let mut header = [0_u8; 4];
        file.read_exact(&mut header)
            .map_err(|error| format!("FLAC metadata is truncated: {error}"))?;
        let last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let length =
            ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
        total = total.saturating_add(length);
        if total > MAX_TAG_BYTES {
            return Err("FLAC metadata exceeds the safe read limit".to_string());
        }
        if block_type == 4 {
            let mut block = vec![0_u8; length];
            file.read_exact(&mut block)
                .map_err(|error| format!("FLAC comments are truncated: {error}"))?;
            return parse_vorbis_comments(&block);
        }
        file.seek(SeekFrom::Current(length as i64))
            .map_err(|error| format!("FLAC metadata seek failed: {error}"))?;
        if last {
            return Ok(LocalTags::default());
        }
    }
}

fn read_ogg_comments(file: &mut File) -> Result<LocalTags, String> {
    let length = file
        .metadata()
        .map_err(|error| format!("Ogg metadata inspection failed: {error}"))?
        .len()
        .min(MAX_COMMENT_SCAN_BYTES as u64) as usize;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("Ogg metadata read failed: {error}"))?;
    for marker in [b"\x03vorbis".as_slice(), b"OpusTags".as_slice()] {
        if let Some(position) = find_bytes(&bytes, marker) {
            return parse_vorbis_comments(&bytes[position + marker.len()..]);
        }
    }
    Ok(LocalTags::default())
}

fn parse_vorbis_comments(bytes: &[u8]) -> Result<LocalTags, String> {
    let mut cursor = 0_usize;
    let vendor_length = read_le_u32(bytes, &mut cursor)? as usize;
    cursor = cursor
        .checked_add(vendor_length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| "Vorbis vendor string is truncated".to_string())?;
    let comment_count = read_le_u32(bytes, &mut cursor)? as usize;
    if comment_count > 100_000 {
        return Err("Vorbis comment count exceeds the safe limit".to_string());
    }
    let mut tags = LocalTags::default();
    for _ in 0..comment_count {
        let length = read_le_u32(bytes, &mut cursor)? as usize;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "Vorbis comment is truncated".to_string())?;
        let comment = std::str::from_utf8(&bytes[cursor..end])
            .map_err(|_| "Vorbis comment is not valid UTF-8".to_string())?;
        cursor = end;
        if let Some((key, value)) = comment.split_once('=') {
            apply_tag(&mut tags, key, value);
        }
    }
    Ok(tags)
}

fn read_wav_info(file: &mut File) -> Result<LocalTags, String> {
    file.seek(SeekFrom::Start(12))
        .map_err(|error| format!("WAV metadata seek failed: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("WAV metadata inspection failed: {error}"))?
        .len();
    let mut tags = LocalTags::default();
    while file.stream_position().unwrap_or(file_len) + 8 <= file_len {
        let mut header = [0_u8; 8];
        file.read_exact(&mut header)
            .map_err(|error| format!("WAV chunk is truncated: {error}"))?;
        let length = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64;
        if &header[..4] == b"LIST" && length >= 4 && length <= MAX_TAG_BYTES as u64 {
            let mut bytes = vec![0_u8; length as usize];
            file.read_exact(&mut bytes)
                .map_err(|error| format!("WAV INFO list is truncated: {error}"))?;
            if &bytes[..4] == b"INFO" {
                parse_wav_info_entries(&bytes[4..], &mut tags)?;
            }
        } else {
            file.seek(SeekFrom::Current(length as i64))
                .map_err(|error| format!("WAV chunk seek failed: {error}"))?;
        }
        if length % 2 == 1 {
            file.seek(SeekFrom::Current(1))
                .map_err(|error| format!("WAV padding seek failed: {error}"))?;
        }
    }
    Ok(tags)
}

fn parse_wav_info_entries(bytes: &[u8], tags: &mut LocalTags) -> Result<(), String> {
    let mut cursor = 0_usize;
    while cursor + 8 <= bytes.len() {
        let name = &bytes[cursor..cursor + 4];
        let length = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;
        let end = cursor
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "WAV INFO entry is truncated".to_string())?;
        let value = String::from_utf8_lossy(&bytes[cursor..end])
            .trim_matches(['\0', ' '])
            .to_string();
        match name {
            b"INAM" => set_if_text(&mut tags.title, Some(value)),
            b"IART" => set_if_text(&mut tags.artist, Some(value)),
            b"IPRD" => set_if_text(&mut tags.album, Some(value)),
            b"ITRK" => tags.track_number = parse_track(&value),
            _ => {}
        }
        cursor = end + (length % 2);
    }
    Ok(())
}

fn read_mp4(file: &mut File) -> Result<LocalTags, String> {
    let length = file
        .metadata()
        .map_err(|error| format!("MP4 metadata inspection failed: {error}"))?
        .len()
        .min(MAX_MP4_METADATA_BYTES as u64) as usize;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("MP4 metadata read failed: {error}"))?;
    let mut tags = LocalTags::default();
    parse_mp4_atoms(&bytes, 0, bytes.len(), 0, &mut tags)?;
    Ok(tags)
}

fn parse_mp4_atoms(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    depth: usize,
    tags: &mut LocalTags,
) -> Result<(), String> {
    if depth > 8 {
        return Err("MP4 metadata nesting exceeds the safe limit".to_string());
    }
    while cursor + 8 <= end {
        let size = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let name = &bytes[cursor + 4..cursor + 8];
        if size < 8 || cursor + size > end {
            break;
        }
        let payload_start = cursor + 8;
        let atom_end = cursor + size;
        if matches!(name, b"moov" | b"udta" | b"ilst") {
            parse_mp4_atoms(bytes, payload_start, atom_end, depth + 1, tags)?;
        } else if name == b"meta" && payload_start + 4 <= atom_end {
            parse_mp4_atoms(bytes, payload_start + 4, atom_end, depth + 1, tags)?;
        } else if matches!(
            name,
            b"\xa9nam" | b"\xa9ART" | b"aART" | b"\xa9alb" | b"trkn"
        ) {
            let value = mp4_data_payload(bytes, payload_start, atom_end);
            match name {
                b"\xa9nam" => set_if_text(&mut tags.title, value.and_then(mp4_text)),
                b"\xa9ART" | b"aART" => {
                    set_if_text(&mut tags.artist, value.and_then(mp4_text));
                }
                b"\xa9alb" => set_if_text(&mut tags.album, value.and_then(mp4_text)),
                b"trkn" => tags.track_number = value.and_then(mp4_track),
                _ => {}
            }
        }
        cursor = atom_end;
    }
    Ok(())
}

fn find_mp4_duration(
    bytes: &[u8],
    mut cursor: usize,
    end: usize,
    depth: usize,
) -> Result<Option<i64>, String> {
    if depth > 4 {
        return Err("MP4 duration nesting exceeds the safe limit".to_string());
    }
    while cursor + 8 <= end {
        let size = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        let name = &bytes[cursor + 4..cursor + 8];
        if size < 8 || cursor + size > end {
            break;
        }
        let payload_start = cursor + 8;
        let atom_end = cursor + size;
        if name == b"mvhd" {
            return parse_mvhd_duration(&bytes[payload_start..atom_end]).map(Some);
        }
        if name == b"moov" {
            if let Some(duration) = find_mp4_duration(bytes, payload_start, atom_end, depth + 1)? {
                return Ok(Some(duration));
            }
        }
        cursor = atom_end;
    }
    Ok(None)
}

fn parse_mvhd_duration(payload: &[u8]) -> Result<i64, String> {
    let version = *payload
        .first()
        .ok_or_else(|| "MP4 movie header is empty".to_string())?;
    let (timescale, duration) = match version {
        0 if payload.len() >= 20 => (
            u32::from_be_bytes(payload[12..16].try_into().unwrap()) as u64,
            u32::from_be_bytes(payload[16..20].try_into().unwrap()) as u64,
        ),
        1 if payload.len() >= 32 => (
            u32::from_be_bytes(payload[20..24].try_into().unwrap()) as u64,
            u64::from_be_bytes(payload[24..32].try_into().unwrap()),
        ),
        0 | 1 => return Err("MP4 movie header is truncated".to_string()),
        _ => return Err(format!("MP4 movie header version {version} is unsupported")),
    };
    if timescale == 0 {
        return Err("MP4 movie timescale is zero".to_string());
    }
    i64::try_from(duration.saturating_mul(1_000) / timescale)
        .map_err(|_| "MP4 duration is too large".to_string())
}

fn mp4_data_payload(bytes: &[u8], mut cursor: usize, end: usize) -> Option<&[u8]> {
    while cursor + 8 <= end {
        let size = u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().ok()?) as usize;
        let name = &bytes[cursor + 4..cursor + 8];
        if size < 8 || cursor + size > end {
            return None;
        }
        if name == b"data" && size >= 16 {
            return Some(&bytes[cursor + 16..cursor + size]);
        }
        cursor += size;
    }
    None
}

fn mp4_text(bytes: &[u8]) -> Option<String> {
    let value = String::from_utf8_lossy(bytes)
        .trim_matches(['\0', ' '])
        .to_string();
    (!value.is_empty()).then_some(value)
}

fn mp4_track(bytes: &[u8]) -> Option<i64> {
    bytes
        .windows(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]) as i64)
        .find(|value| *value > 0)
}

fn decode_id3_text(content: &[u8]) -> Result<Option<String>, String> {
    let Some((&encoding, payload)) = content.split_first() else {
        return Ok(None);
    };
    let value = match encoding {
        0 => payload.iter().map(|byte| char::from(*byte)).collect(),
        1 => decode_utf16(payload, None)?,
        2 => decode_utf16(payload, Some(true))?,
        3 => String::from_utf8_lossy(payload).to_string(),
        _ => return Err(format!("ID3 text encoding {encoding} is unsupported")),
    };
    let value = value.trim_matches(['\0', ' ']).to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn decode_utf16(bytes: &[u8], big_endian: Option<bool>) -> Result<String, String> {
    let (big_endian, payload) = match big_endian {
        Some(value) => (value, bytes),
        None if bytes.starts_with(&[0xfe, 0xff]) => (true, &bytes[2..]),
        None if bytes.starts_with(&[0xff, 0xfe]) => (false, &bytes[2..]),
        None => (false, bytes),
    };
    if payload.len() % 2 != 0 {
        return Err("ID3 UTF-16 text has an incomplete code unit".to_string());
    }
    let units = payload.as_chunks::<2>().0.iter().map(|pair| {
        if big_endian {
            u16::from_be_bytes(*pair)
        } else {
            u16::from_le_bytes(*pair)
        }
    });
    String::from_utf16(&units.collect::<Vec<_>>())
        .map_err(|_| "ID3 UTF-16 text is malformed".to_string())
}

fn apply_tag(tags: &mut LocalTags, key: &str, value: &str) {
    let value = value.trim().to_string();
    match key.trim().to_ascii_uppercase().as_str() {
        "TITLE" => set_if_text(&mut tags.title, Some(value)),
        "ARTIST" | "ALBUMARTIST" => set_if_text(&mut tags.artist, Some(value)),
        "ALBUM" => set_if_text(&mut tags.album, Some(value)),
        "TRACKNUMBER" | "TRACK" => tags.track_number = parse_track(&value),
        _ => {}
    }
}

fn set_if_text(target: &mut Option<String>, value: Option<String>) {
    if target.is_none() {
        *target = value.filter(|text| !text.trim().is_empty());
    }
}

fn parse_track(value: &str) -> Option<i64> {
    value
        .split(['/', '-'])
        .next()
        .and_then(|part| part.trim().parse::<i64>().ok())
        .filter(|number| *number > 0)
}

fn synchsafe_u32(bytes: &[u8]) -> Result<u32, String> {
    if bytes.len() != 4 || bytes.iter().any(|byte| byte & 0x80 != 0) {
        return Err("ID3 synchsafe size is malformed".to_string());
    }
    Ok(((bytes[0] as u32) << 21)
        | ((bytes[1] as u32) << 14)
        | ((bytes[2] as u32) << 7)
        | bytes[3] as u32)
}

fn read_le_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, String> {
    let end = cursor
        .checked_add(4)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| "Vorbis comment header is truncated".to_string())?;
    let value = u32::from_le_bytes(bytes[*cursor..end].try_into().unwrap());
    *cursor = end;
    Ok(value)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_id3_text_frames_and_track_numbers() {
        let mut frames = Vec::new();
        for (name, value) in [
            (b"TIT2", "Flow theme"),
            (b"TPE1", "Composer"),
            (b"TALB", "Game soundtrack"),
            (b"TRCK", "7/24"),
        ] {
            let mut content = vec![3];
            content.extend_from_slice(value.as_bytes());
            frames.extend_from_slice(name);
            frames.extend_from_slice(&(content.len() as u32).to_be_bytes());
            frames.extend_from_slice(&[0, 0]);
            frames.extend_from_slice(&content);
        }
        let mut bytes = b"ID3\x03\x00\x00".to_vec();
        let size = frames.len() as u32;
        bytes.extend_from_slice(&[
            ((size >> 21) & 0x7f) as u8,
            ((size >> 14) & 0x7f) as u8,
            ((size >> 7) & 0x7f) as u8,
            (size & 0x7f) as u8,
        ]);
        bytes.extend(frames);
        let path = fixture_file("id3.mp3", &bytes);
        let tags = read_tags(&path, "mp3").unwrap();
        assert_eq!(tags.title.as_deref(), Some("Flow theme"));
        assert_eq!(tags.artist.as_deref(), Some("Composer"));
        assert_eq!(tags.album.as_deref(), Some("Game soundtrack"));
        assert_eq!(tags.track_number, Some(7));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_vorbis_comments_and_rejects_truncation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(b"test");
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        for comment in [b"TITLE=Rain room".as_slice(), b"TRACKNUMBER=12".as_slice()] {
            bytes.extend_from_slice(&(comment.len() as u32).to_le_bytes());
            bytes.extend_from_slice(comment);
        }
        let tags = parse_vorbis_comments(&bytes).unwrap();
        assert_eq!(tags.title.as_deref(), Some("Rain room"));
        assert_eq!(tags.track_number, Some(12));
        assert!(parse_vorbis_comments(&bytes[..7]).is_err());
    }

    #[test]
    fn missing_or_unknown_tags_produce_an_empty_fallback() {
        let path = fixture_file("untagged.bin", b"ordinary media bytes");
        assert_eq!(read_tags(&path, "bin").unwrap(), LocalTags::default());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_local_video_mp4_title_without_decoding_the_video() {
        let title = mp4_atom(b"\xa9nam", &mp4_data(b"Video opening"));
        let artist = mp4_atom(b"\xa9ART", &mp4_data(b"Studio composer"));
        let ilst = mp4_atom(b"ilst", &[title, artist].concat());
        let mut meta_payload = vec![0, 0, 0, 0];
        meta_payload.extend(ilst);
        let meta = mp4_atom(b"meta", &meta_payload);
        let udta = mp4_atom(b"udta", &meta);
        let mut mvhd_payload = vec![0; 20];
        mvhd_payload[12..16].copy_from_slice(&1_000_u32.to_be_bytes());
        mvhd_payload[16..20].copy_from_slice(&95_000_u32.to_be_bytes());
        let mvhd = mp4_atom(b"mvhd", &mvhd_payload);
        let moov = mp4_atom(b"moov", &[mvhd, udta].concat());
        let ftyp = mp4_atom(b"ftyp", b"isom\0\0\0\0");
        let path = fixture_file("tagged.mp4", &[ftyp, moov].concat());
        let tags = read_tags(&path, "mp4").unwrap();
        assert_eq!(tags.title.as_deref(), Some("Video opening"));
        assert_eq!(tags.artist.as_deref(), Some("Studio composer"));
        assert_eq!(
            read_container_duration_ms(&path, "mp4").unwrap(),
            Some(95_000)
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_wav_info_and_reports_malformed_id3_without_rejecting_fallback_callers() {
        let mut info = b"INFO".to_vec();
        info.extend(wav_info_entry(b"INAM", b"Calm water\0"));
        info.extend(wav_info_entry(b"ITRK", b"4\0"));
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&((4 + 8 + info.len()) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&(info.len() as u32).to_le_bytes());
        wav.extend(info);
        let wav_path = fixture_file("tagged.wav", &wav);
        let tags = read_tags(&wav_path, "wav").unwrap();
        assert_eq!(tags.title.as_deref(), Some("Calm water"));
        assert_eq!(tags.track_number, Some(4));
        std::fs::remove_file(wav_path).unwrap();

        let malformed_path = fixture_file("malformed.mp3", b"ID3\x03\0\0\0\0\0\x7fshort");
        assert!(read_tags(&malformed_path, "mp3").is_err());
        std::fs::remove_file(malformed_path).unwrap();
    }

    fn mp4_atom(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut atom = Vec::with_capacity(payload.len() + 8);
        atom.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
        atom.extend_from_slice(name);
        atom.extend_from_slice(payload);
        atom
    }

    fn mp4_data(value: &[u8]) -> Vec<u8> {
        let mut payload = vec![0; 8];
        payload.extend_from_slice(value);
        mp4_atom(b"data", &payload)
    }

    fn wav_info_entry(name: &[u8; 4], value: &[u8]) -> Vec<u8> {
        let mut entry = Vec::new();
        entry.extend_from_slice(name);
        entry.extend_from_slice(&(value.len() as u32).to_le_bytes());
        entry.extend_from_slice(value);
        if value.len() % 2 == 1 {
            entry.push(0);
        }
        entry
    }

    fn fixture_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("ganbaru-metadata-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
