//! Bounded Music JSON and M3U codecs used by native preview, commit, and export.

use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Write;

pub(super) const FORMAT: &str = "ganbaru-ai/music-playlists";
pub(super) const MAX_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_MEMBERSHIPS: usize = 10_000;
pub(super) const MAX_PLAYLISTS: usize = 500;
pub(super) const MAX_CHILD_ROWS: usize = 100_000;
pub(super) const MAX_DIAGNOSTICS: usize = 200;
pub(super) const MAX_DIAGNOSTIC_CHARS: usize = 1_024;
pub(super) const M3U_WARNING: &str = "M3U8 does not preserve weights, snoozes, assignments, logical-root identity, signals, or focus guidance.";

#[derive(Clone, Debug, PartialEq)]
pub(super) struct M3uEntry {
    pub value: String,
    pub title: Option<String>,
    pub video_id: Option<String>,
    pub unsupported: bool,
}

pub(super) fn validation(message: impl Into<String>) -> MusicLibraryError {
    MusicLibraryError::validation("transfer", message)
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn check_contents(contents: &str) -> MusicLibraryResult<()> {
    if contents.len() > MAX_BYTES {
        return Err(validation(
            "The music transfer exceeds the 8 MB safety limit",
        ));
    }
    Ok(())
}

/// Rejects output before an allocator can grow beyond the document byte ceiling.
struct BoundedOutput(Vec<u8>);

impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(std::io::Error::other(
                "Music transfer exceeds the 8 MB safety limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn document_bytes<T: serde::Serialize>(value: &T) -> MusicLibraryResult<Vec<u8>> {
    let mut output = BoundedOutput(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|error| validation(error.to_string()))?;
    Ok(output.0)
}

/// Parses the current JSON format while retaining diagnostics for unknown membership kinds.
pub(super) fn parse_json(contents: &str) -> MusicLibraryResult<MusicInterchangeDocument> {
    check_contents(contents)?;
    let mut value: Value = serde_json::from_str(contents.trim_start_matches(|character: char| {
        character.is_whitespace() || character == '\u{feff}'
    }))
    .map_err(|error| validation(format!("The selected file is not valid JSON: {error}")))?;
    validate_shape(&value, 0)?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(validation(
            "The selected JSON file is not a Ganbaru AI music export",
        ));
    }
    if value.get("version").and_then(Value::as_i64) != Some(1) {
        return Err(validation("The music export version is not supported"));
    }
    let playlists = value
        .get_mut("playlists")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| validation("Music import playlists must be an array"))?;
    if playlists.len() > MAX_PLAYLISTS {
        return Err(validation("Music import exceeds its playlist limit"));
    }
    let mut warnings = Vec::new();
    let mut membership_count = 0;
    for (playlist_index, playlist) in playlists.iter_mut().enumerate() {
        let memberships = playlist
            .get_mut("memberships")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| validation("Music playlist memberships must be an array"))?;
        membership_count += memberships.len();
        if membership_count > MAX_MEMBERSHIPS {
            return Err(validation("Music import exceeds its membership limit"));
        }
        let mut supported = Vec::with_capacity(memberships.len());
        for (index, membership) in memberships.drain(..).enumerate() {
            match serde_json::from_value::<MusicInterchangeMembership>(membership.clone()) {
                Ok(_) => supported.push(membership),
                Err(error) if error.to_string().starts_with("unknown variant") => {
                    warnings.push(Value::String(format!(
                        "Unsupported record skipped: playlists[{playlist_index}].memberships[{index}]: {error}"
                    )));
                }
                Err(error) => return Err(validation(format!("Invalid music membership: {error}"))),
            }
        }
        *memberships = supported;
    }
    let stored_warnings = value
        .get_mut("warnings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| validation("Music import warnings must be an array"))?;
    stored_warnings.extend(warnings);
    let document: MusicInterchangeDocument = serde_json::from_value(value)
        .map_err(|error| validation(format!("Invalid music import: {error}")))?;
    validate_document(&document)?;
    Ok(document)
}

fn validate_shape(value: &Value, depth: usize) -> MusicLibraryResult<()> {
    if depth > 16 {
        return Err(validation("Music import nesting is too deep"));
    }
    match value {
        Value::String(text) if text.len() > 8_192 => {
            return Err(validation("Music import text exceeds its field limit"));
        }
        Value::Array(values) => {
            if values.len() > MAX_MEMBERSHIPS {
                return Err(validation("Music import array exceeds its record limit"));
            }
            for entry in values {
                validate_shape(entry, depth + 1)?;
            }
        }
        Value::Object(values) => {
            if values.len() > 64 {
                return Err(validation("Music import object exceeds its field limit"));
            }
            for entry in values.values() {
                validate_shape(entry, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn validate_document(document: &MusicInterchangeDocument) -> MusicLibraryResult<()> {
    if document.roots.len() > MAX_PLAYLISTS || document.context_assignments.len() > MAX_MEMBERSHIPS
    {
        return Err(validation("Music import exceeds its record limit"));
    }
    for playlist in &document.playlists {
        let mut intended_uses = HashSet::new();
        if playlist
            .intended_uses
            .iter()
            .any(|entry| !intended_uses.insert(entry))
        {
            return Err(validation("Music playlist intended uses must be unique"));
        }
        for membership in &playlist.memberships {
            if membership.item.locations.len() > 32
                || membership.skip_ranges.len() > 100
                || membership.snoozes.len() > 100
                || membership.item.signals.len() > 6
            {
                return Err(validation(
                    "Music membership exceeds its child record limit",
                ));
            }
            if membership
                .start_ms
                .zip(membership.end_ms)
                .is_some_and(|(start, end)| end < start)
            {
                return Err(validation(
                    "Music membership end must not precede its start",
                ));
            }
            for snooze in &membership.snoozes {
                if snooze.starts_at_ms <= 0
                    || snooze
                        .ends_at_ms
                        .is_some_and(|end| end <= snooze.starts_at_ms)
                {
                    return Err(validation(
                        "Music snooze must have a positive, ordered time range",
                    ));
                }
            }
        }
    }
    let mut assignment_keys = HashSet::new();
    for assignment in &document.context_assignments {
        if !assignment_keys.insert((
            assignment.owner_kind,
            &assignment.owner_id,
            assignment.phase,
        )) {
            return Err(validation("Music assignment owner phases must be unique"));
        }
        if assignment.version <= 0 {
            return Err(validation("Invalid Music assignment revision"));
        }
        ganbaru_music::assignments::validate_set(&MusicContextAssignmentSet {
            owner_kind: assignment.owner_kind,
            owner_id: assignment.owner_id.clone(),
            updated_at_ms: assignment.updated_at_ms,
            assignments: vec![ganbaru_music::assignments::MusicContextAssignmentDraft {
                phase: assignment.phase,
                behavior: assignment.behavior,
                playlist_id: assignment.playlist_id.clone(),
                soundscape_id: assignment.soundscape_id.clone(),
                soundscape_behavior: assignment.soundscape_behavior,
                provenance_kind: assignment.provenance_kind,
                provenance_id: assignment.provenance_id.clone(),
            }],
        })?;
    }
    super::interchange::validate_import(&MusicInterchangeImportRequest {
        document: document.clone(),
        playlist_conflict: MusicImportPlaylistConflict::ImportCopy,
        replace_item_descriptions: false,
        import_context_assignments: false,
        imported_at: document.exported_at,
    })
}

/// Keeps portable local identity case-sensitive, independent of device path matching.
pub(super) fn local_identity(root_id: &str, relative_path: &str) -> String {
    format!(
        "local-import:{}",
        hash(format!("{root_id}\0{}", relative_path.replace('\\', "/")).as_bytes())
    )
}

fn youtube_id(value: &str) -> Option<String> {
    let value = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let url = url::Url::parse(&value).ok()?;
    let candidate = match url.host_str()? {
        "youtu.be" | "www.youtu.be" => url.path_segments()?.next()?.to_string(),
        "youtube.com" | "www.youtube.com" if url.path() == "/watch" => url
            .query_pairs()
            .find(|(key, _)| key == "v")?
            .1
            .into_owned(),
        _ => return None,
    };
    (candidate.len() >= 6
        && candidate.len() <= 64
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')))
    .then_some(candidate)
}

pub(super) fn parse_m3u(contents: &str) -> MusicLibraryResult<Vec<M3uEntry>> {
    check_contents(contents)?;
    let mut entries = Vec::new();
    let mut title = None;
    for line in contents
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
    {
        if line.is_empty() {
            continue;
        }
        if line.len() > 8_192 {
            return Err(validation("M3U8 entry is too long"));
        }
        if line.starts_with("#EXTINF:") {
            title = line
                .split_once(',')
                .map(|(_, title)| title.trim().to_string())
                .filter(|title| !title.is_empty());
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let video_id = youtube_id(line);
        entries.push(M3uEntry {
            value: line.to_string(),
            title: title.take(),
            unsupported: video_id.is_none() && line.contains("://"),
            video_id,
        });
        if entries.len() > MAX_MEMBERSHIPS {
            return Err(validation("M3U8 import exceeds its entry limit"));
        }
    }
    Ok(entries)
}

pub(super) fn serialize_json(mut document: MusicInterchangeDocument) -> MusicLibraryResult<String> {
    document.roots.sort_by(|left, right| left.id.cmp(&right.id));
    document
        .playlists
        .sort_by(|left, right| left.id.cmp(&right.id));
    for playlist in &mut document.playlists {
        playlist
            .intended_uses
            .sort_by_key(|entry| entry.as_ref().to_string());
        playlist.memberships.sort_by(|left, right| {
            left.position
                .cmp(&right.position)
                .then_with(|| left.item.identity_key.cmp(&right.item.identity_key))
        });
    }
    document.context_assignments.sort_by_key(|entry| {
        format!(
            "{}:{}:{}",
            entry.owner_kind.as_ref(),
            entry.owner_id,
            entry.phase.as_ref()
        )
    });
    document.warnings.sort();
    let mut output = BoundedOutput(Vec::new());
    serde_json::to_writer_pretty(&mut output, &document)
        .map_err(|error| validation(error.to_string()))?;
    output
        .write_all(b"\n")
        .map_err(|error| validation(error.to_string()))?;
    String::from_utf8(output.0).map_err(|error| validation(error.to_string()))
}
