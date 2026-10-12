//! Unified patch parsing and stable hunk identity.

use super::super::models::ChatResult;
use super::contracts::{ReviewHunkRead, ReviewPatchState};
use super::corrupt_data_error;
use super::patch_store::{ParsedHunk, ParsedPatch};
use sha2::{Digest, Sha256};

pub fn parse_patch(patch: &str, file_id: &str) -> ChatResult<ParsedPatch> {
    let mut starts = patch
        .match_indices("\n@@ ")
        .map(|(index, _)| index + 1)
        .collect::<Vec<_>>();
    if patch.starts_with("@@ ") {
        starts.insert(0, 0);
    }
    let preamble_end = starts.first().copied().unwrap_or(patch.len());
    let mut hunks = Vec::new();
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(patch.len());
        let text = patch[start..end].to_string();
        let header = text.lines().next().ok_or_else(corrupt_data_error)?;
        let (old_start, old_count, new_start, new_count) = parse_hunk_header(header)?;
        hunks.push(ParsedHunk {
            read: ReviewHunkRead {
                hunk_id: hunk_id(file_id, &text, old_start, new_start),
                old_start,
                old_count,
                new_start,
                new_count,
                state: ReviewPatchState::Complete,
            },
            text,
        });
    }
    Ok(ParsedPatch {
        preamble: patch[..preamble_end].to_string(),
        hunks,
    })
}

pub fn parse_hunk_header(header: &str) -> ChatResult<(u64, u64, u64, u64)> {
    let body = header
        .strip_prefix("@@ -")
        .and_then(|value| value.split_once(" @@").map(|(range, _)| range))
        .ok_or_else(corrupt_data_error)?;
    let (old, new) = body.split_once(" +").ok_or_else(corrupt_data_error)?;
    let old = parse_hunk_range(old)?;
    let new = parse_hunk_range(new)?;
    Ok((old.0, old.1, new.0, new.1))
}

fn parse_hunk_range(value: &str) -> ChatResult<(u64, u64)> {
    let (start, lines) = value
        .split_once(',')
        .map_or((value, "1"), |(start, lines)| (start, lines));
    Ok((
        start.parse().map_err(|_| corrupt_data_error())?,
        lines.parse().map_err(|_| corrupt_data_error())?,
    ))
}

pub fn hunk_id(file_id: &str, text: &str, old_start: u64, new_start: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(file_id);
    hasher.update([0]);
    let mut old_line = old_start;
    let mut new_line = new_start;
    for line in text.lines().skip(1) {
        if line.starts_with(' ') {
            old_line = old_line.saturating_add(1);
            new_line = new_line.saturating_add(1);
        } else if let Some(content) = line.strip_prefix('-') {
            hasher.update(b"-");
            hasher.update(old_line.to_le_bytes());
            hasher.update(content);
            hasher.update([0]);
            old_line = old_line.saturating_add(1);
        } else if let Some(content) = line.strip_prefix('+') {
            hasher.update(b"+");
            hasher.update(new_line.to_le_bytes());
            hasher.update(content);
            hasher.update([0]);
            new_line = new_line.saturating_add(1);
        }
    }
    format!("review-hunk:{:x}", hasher.finalize())
}
