//! Patch paging, spooling, and temporary Git object storage.

use super::super::models::{ChatError, ChatErrorCode, ChatResult};
use super::contracts::{ReviewDiffSource, ReviewFilePatchRead, ReviewHunkRead, ReviewPatchState};
use super::material::deterministic_diff_arguments;
use super::patch_parser::{parse_hunk_header, parse_patch};
use super::registry::{ReviewFileInternal, ReviewSnapshot};
use super::{MAX_PATCH_PAGE_BYTES, corrupt_data_error, git_text, review_error};
use crate::git;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_PATCH_HUNKS_PER_RESPONSE: usize = 2_048;
const MAX_PATCH_SPOOL_BYTES: usize = 512 * 1024 * 1024;
const MAX_REVIEW_OBJECT_BYTES: u64 = 512 * 1024 * 1024;
static REVIEW_TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
pub struct ParsedHunk {
    pub read: ReviewHunkRead,
    pub text: String,
}

pub struct ParsedPatch {
    pub preamble: String,
    pub hunks: Vec<ParsedHunk>,
}

pub enum PatchIndex {
    Memory(ParsedPatch),
    Spool(SpoolPatchIndex),
}

#[derive(Clone)]
pub struct SpoolPatchIndex {
    file: Arc<ReviewTemporaryFile>,
    byte_size: u64,
    preamble_end: u64,
    hunks: Vec<IndexedHunk>,
}

#[derive(Clone)]
struct IndexedHunk {
    read: ReviewHunkRead,
    start: u64,
    end: u64,
}

pub struct ReviewObjectStore {
    root: PathBuf,
    pub objects: PathBuf,
    pub index: PathBuf,
    pub alternate_objects: PathBuf,
}

struct ReviewTemporaryFile {
    root: PathBuf,
    path: PathBuf,
}

pub async fn read_patch_page(
    snapshot: &Arc<ReviewSnapshot>,
    file_id: &str,
    start_hunk: usize,
    maximum: usize,
) -> ChatResult<ReviewFilePatchRead> {
    let file = snapshot
        .files
        .iter()
        .find(|file| file.read.file_id == file_id)
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Review file was not found", true)
        })?;
    if file.read.flags.binary {
        return Ok(ReviewFilePatchRead {
            file_id: file_id.to_string(),
            patch: None,
            hunks: Vec::new(),
            continuation_cursor: None,
            state: ReviewPatchState::Binary,
        });
    }
    if matches!(&snapshot.source, ReviewDiffSource::ProviderTurn { .. })
        && !snapshot.provider_patches.contains_key(file_id)
    {
        return Ok(ReviewFilePatchRead {
            file_id: file_id.to_string(),
            patch: None,
            hunks: Vec::new(),
            continuation_cursor: None,
            state: ReviewPatchState::Unavailable,
        });
    }
    let index = load_patch_index(snapshot, file).await?;
    read_indexed_patch_page(index, file_id, start_hunk, maximum).await
}

async fn load_patch_index(
    snapshot: &Arc<ReviewSnapshot>,
    file: &ReviewFileInternal,
) -> ChatResult<Arc<PatchIndex>> {
    if let Some(index) = snapshot
        .patch_cache
        .lock()
        .await
        .get(&file.read.file_id)
        .cloned()
    {
        return Ok(index);
    }
    let index = if let Some(patch) = snapshot.provider_patches.get(&file.read.file_id) {
        Arc::new(PatchIndex::Memory(parse_patch(patch, &file.read.file_id)?))
    } else {
        let before = snapshot.before_oid.as_deref().ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Review patch is unavailable", true)
        })?;
        let after = snapshot.after_oid.as_deref().ok_or_else(|| {
            ChatError::new(ChatErrorCode::NotFound, "Review patch is unavailable", true)
        })?;
        let context = format!("--unified={}", snapshot.context_lines);
        let mut arguments =
            deterministic_diff_arguments(snapshot.ignore_whitespace, Some(&context));
        arguments.extend([before, after, "--", &file.read.relative_path]);
        if let Some(previous) = file.read.previous_relative_path.as_deref() {
            arguments.push(previous);
        }
        let spool = Arc::new(ReviewTemporaryFile::new("patch")?);
        git::review_output_to_file_in_storage(
            &snapshot.root,
            &arguments,
            spool.path(),
            MAX_PATCH_SPOOL_BYTES,
            snapshot
                .object_store
                .as_ref()
                .map(|store| store.objects.as_path()),
            snapshot
                .object_store
                .as_ref()
                .map(|store| store.alternate_objects.as_path()),
        )
        .await?;
        let spool_for_index = spool.clone();
        let file_id = file.read.file_id.clone();
        let indexed =
            tokio::task::spawn_blocking(move || index_spooled_patch(spool_for_index, &file_id))
                .await
                .map_err(|_| review_error("Review patch index worker stopped"))??;
        Arc::new(PatchIndex::Spool(indexed))
    };
    let mut cache = snapshot.patch_cache.lock().await;
    Ok(cache
        .entry(file.read.file_id.clone())
        .or_insert_with(|| index.clone())
        .clone())
}

async fn read_indexed_patch_page(
    index: Arc<PatchIndex>,
    file_id: &str,
    start_hunk: usize,
    maximum: usize,
) -> ChatResult<ReviewFilePatchRead> {
    match index.as_ref() {
        PatchIndex::Memory(parsed) => read_memory_patch_page(parsed, file_id, start_hunk, maximum),
        PatchIndex::Spool(spool) => {
            let file_id = file_id.to_string();
            let spool = spool.clone();
            tokio::task::spawn_blocking(move || {
                read_spooled_patch_page(&spool, &file_id, start_hunk, maximum)
            })
            .await
            .map_err(|_| review_error("Review patch page worker stopped"))?
        }
    }
}

fn read_memory_patch_page(
    parsed: &ParsedPatch,
    file_id: &str,
    start_hunk: usize,
    maximum: usize,
) -> ChatResult<ReviewFilePatchRead> {
    if parsed.hunks.is_empty() {
        let complete = parsed.preamble.len() <= maximum;
        return Ok(ReviewFilePatchRead {
            file_id: file_id.to_string(),
            patch: complete.then(|| parsed.preamble.clone()),
            hunks: Vec::new(),
            continuation_cursor: None,
            state: if complete {
                ReviewPatchState::Complete
            } else {
                ReviewPatchState::Unavailable
            },
        });
    }
    if parsed.preamble.len() > maximum {
        return Ok(unavailable_patch(file_id));
    }
    let mut page = parsed.preamble.clone();
    let mut reads = Vec::new();
    let mut index = start_hunk.min(parsed.hunks.len());
    while let Some(hunk) = parsed.hunks.get(index) {
        let metadata_bytes = hunk.read.hunk_id.len().saturating_add(256);
        if page
            .len()
            .saturating_add(hunk.text.len())
            .saturating_add(metadata_bytes)
            > maximum
            || reads.len() >= MAX_PATCH_HUNKS_PER_RESPONSE
        {
            if reads.is_empty() {
                let mut read = hunk.read.clone();
                read.state = ReviewPatchState::OversizedHunk;
                reads.push(read);
                index += 1;
            }
            break;
        }
        page.push_str(&hunk.text);
        reads.push(hunk.read.clone());
        index += 1;
    }
    let continuation_cursor = (index < parsed.hunks.len()).then(|| format!("{file_id}/{index}"));
    let oversized = reads
        .iter()
        .any(|hunk| hunk.state == ReviewPatchState::OversizedHunk);
    Ok(ReviewFilePatchRead {
        file_id: file_id.to_string(),
        patch: (!page.is_empty()
            && reads
                .iter()
                .any(|hunk| hunk.state == ReviewPatchState::Complete))
        .then_some(page),
        hunks: reads,
        continuation_cursor: continuation_cursor.clone(),
        state: if oversized {
            ReviewPatchState::OversizedHunk
        } else if continuation_cursor.is_some() || start_hunk > 0 {
            ReviewPatchState::Partial
        } else {
            ReviewPatchState::Complete
        },
    })
}

pub fn unavailable_patch(file_id: &str) -> ReviewFilePatchRead {
    ReviewFilePatchRead {
        file_id: file_id.to_string(),
        patch: None,
        hunks: Vec::new(),
        continuation_cursor: None,
        state: ReviewPatchState::Unavailable,
    }
}

fn index_spooled_patch(
    file: Arc<ReviewTemporaryFile>,
    file_id: &str,
) -> ChatResult<SpoolPatchIndex> {
    let input = fs::File::open(file.path()).map_err(|_| review_error("Open review patch spool"))?;
    let byte_size = input
        .metadata()
        .map_err(|_| review_error("Read review patch spool metadata"))?
        .len();
    let mut reader = BufReader::new(input);
    let mut position = 0_u64;
    let mut preamble_end = byte_size;
    let mut hunks = Vec::new();
    let mut active: Option<SpoolHunkBuilder> = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| review_error("Read review patch spool"))?;
        if read == 0 {
            break;
        }
        let line_start = position;
        position = position
            .checked_add(u64::try_from(read).map_err(|_| corrupt_data_error())?)
            .ok_or_else(corrupt_data_error)?;
        let text = std::str::from_utf8(&line)
            .map_err(|_| review_error("Git diff output is not valid UTF-8"))?;
        if text.starts_with("@@ ") {
            if let Some(builder) = active.take() {
                hunks.push(builder.finish(line_start));
            } else {
                preamble_end = line_start;
            }
            active = Some(SpoolHunkBuilder::new(file_id, text, line_start)?);
        } else if let Some(builder) = active.as_mut() {
            builder.push_line(text);
        }
    }
    if let Some(builder) = active {
        hunks.push(builder.finish(byte_size));
    }
    Ok(SpoolPatchIndex {
        file,
        byte_size,
        preamble_end,
        hunks,
    })
}

struct SpoolHunkBuilder {
    start: u64,
    old_start: u64,
    old_count: u64,
    new_start: u64,
    new_count: u64,
    old_line: u64,
    new_line: u64,
    hasher: Sha256,
}

impl SpoolHunkBuilder {
    fn new(file_id: &str, header: &str, start: u64) -> ChatResult<Self> {
        let (old_start, old_count, new_start, new_count) = parse_hunk_header(header.trim_end())?;
        let mut hasher = Sha256::new();
        hasher.update(file_id);
        hasher.update([0]);
        Ok(Self {
            start,
            old_start,
            old_count,
            new_start,
            new_count,
            old_line: old_start,
            new_line: new_start,
            hasher,
        })
    }

    fn push_line(&mut self, line: &str) {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with(' ') {
            self.old_line = self.old_line.saturating_add(1);
            self.new_line = self.new_line.saturating_add(1);
        } else if let Some(content) = line.strip_prefix('-') {
            self.hasher.update(b"-");
            self.hasher.update(self.old_line.to_le_bytes());
            self.hasher.update(content);
            self.hasher.update([0]);
            self.old_line = self.old_line.saturating_add(1);
        } else if let Some(content) = line.strip_prefix('+') {
            self.hasher.update(b"+");
            self.hasher.update(self.new_line.to_le_bytes());
            self.hasher.update(content);
            self.hasher.update([0]);
            self.new_line = self.new_line.saturating_add(1);
        }
    }

    fn finish(self, end: u64) -> IndexedHunk {
        IndexedHunk {
            read: ReviewHunkRead {
                hunk_id: format!("review-hunk:{:x}", self.hasher.finalize()),
                old_start: self.old_start,
                old_count: self.old_count,
                new_start: self.new_start,
                new_count: self.new_count,
                state: ReviewPatchState::Complete,
            },
            start: self.start,
            end,
        }
    }
}

fn read_spooled_patch_page(
    spool: &SpoolPatchIndex,
    file_id: &str,
    start_hunk: usize,
    maximum: usize,
) -> ChatResult<ReviewFilePatchRead> {
    if spool.hunks.is_empty() {
        let complete = usize::try_from(spool.byte_size)
            .ok()
            .is_some_and(|size| size <= maximum);
        return Ok(ReviewFilePatchRead {
            file_id: file_id.to_string(),
            patch: complete
                .then(|| read_spool_range(spool.file.path(), 0, spool.byte_size))
                .transpose()?,
            hunks: Vec::new(),
            continuation_cursor: None,
            state: if complete {
                ReviewPatchState::Complete
            } else {
                ReviewPatchState::Unavailable
            },
        });
    }
    let preamble_size = usize::try_from(spool.preamble_end).unwrap_or(usize::MAX);
    if preamble_size > maximum {
        return Ok(unavailable_patch(file_id));
    }
    let mut page = read_spool_range(spool.file.path(), 0, spool.preamble_end)?;
    let mut reads = Vec::new();
    let mut index = start_hunk.min(spool.hunks.len());
    while let Some(hunk) = spool.hunks.get(index) {
        let hunk_size = usize::try_from(hunk.end.saturating_sub(hunk.start)).unwrap_or(usize::MAX);
        let metadata_bytes = hunk.read.hunk_id.len().saturating_add(256);
        if page
            .len()
            .saturating_add(hunk_size)
            .saturating_add(metadata_bytes)
            > maximum
            || reads.len() >= MAX_PATCH_HUNKS_PER_RESPONSE
        {
            if reads.is_empty() {
                let mut read = hunk.read.clone();
                read.state = ReviewPatchState::OversizedHunk;
                reads.push(read);
                index += 1;
            }
            break;
        }
        page.push_str(&read_spool_range(spool.file.path(), hunk.start, hunk.end)?);
        reads.push(hunk.read.clone());
        index += 1;
    }
    let continuation_cursor = (index < spool.hunks.len()).then(|| format!("{file_id}/{index}"));
    let oversized = reads
        .iter()
        .any(|hunk| hunk.state == ReviewPatchState::OversizedHunk);
    Ok(ReviewFilePatchRead {
        file_id: file_id.to_string(),
        patch: (!page.is_empty()
            && reads
                .iter()
                .any(|hunk| hunk.state == ReviewPatchState::Complete))
        .then_some(page),
        hunks: reads,
        continuation_cursor: continuation_cursor.clone(),
        state: if oversized {
            ReviewPatchState::OversizedHunk
        } else if continuation_cursor.is_some() || start_hunk > 0 {
            ReviewPatchState::Partial
        } else {
            ReviewPatchState::Complete
        },
    })
}

fn read_spool_range(path: &Path, start: u64, end: u64) -> ChatResult<String> {
    let length = usize::try_from(end.saturating_sub(start)).map_err(|_| corrupt_data_error())?;
    if length > MAX_PATCH_PAGE_BYTES {
        return Err(review_error(
            "Review patch page exceeds the supported limit",
        ));
    }
    let mut file = fs::File::open(path).map_err(|_| review_error("Open review patch spool"))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|_| review_error("Seek review patch spool"))?;
    let mut bytes = vec![0_u8; length];
    file.read_exact(&mut bytes)
        .map_err(|_| review_error("Read review patch spool"))?;
    String::from_utf8(bytes).map_err(|_| review_error("Git diff output is not valid UTF-8"))
}

impl ReviewObjectStore {
    pub async fn new(repository_root: &Path) -> ChatResult<Self> {
        let alternate_objects = PathBuf::from(
            git_text(
                repository_root,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "objects",
                ],
                None,
            )
            .await?,
        );
        if !alternate_objects.is_absolute() || !alternate_objects.is_dir() {
            return Err(review_error("Git object directory is unavailable"));
        }
        let root = create_review_temporary_directory("objects")?;
        let objects = root.join("objects");
        if fs::create_dir(&objects).is_err() {
            let _ = fs::remove_dir_all(&root);
            return Err(review_error("Create review object directory"));
        }
        Ok(Self {
            index: root.join("index"),
            root,
            objects,
            alternate_objects,
        })
    }

    pub fn verify_size(&self) -> ChatResult<()> {
        let mut total = 0_u64;
        let mut pending = vec![self.objects.clone()];
        while let Some(directory) = pending.pop() {
            let entries = fs::read_dir(directory)
                .map_err(|_| review_error("Read review object directory"))?;
            for entry in entries {
                let entry = entry.map_err(|_| review_error("Read review object entry"))?;
                let metadata = entry
                    .metadata()
                    .map_err(|_| review_error("Read review object metadata"))?;
                if metadata.is_dir() {
                    pending.push(entry.path());
                } else if metadata.is_file() {
                    total = total.saturating_add(metadata.len());
                    if total > MAX_REVIEW_OBJECT_BYTES {
                        return Err(ChatError::new(
                            ChatErrorCode::Protocol,
                            "Review snapshot exceeds the supported temporary storage limit",
                            true,
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for ReviewObjectStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl ReviewTemporaryFile {
    fn new(kind: &str) -> ChatResult<Self> {
        let root = create_review_temporary_directory(kind)?;
        Ok(Self {
            path: root.join("content"),
            root,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ReviewTemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn create_review_temporary_directory(kind: &str) -> ChatResult<PathBuf> {
    for _ in 0..32 {
        let sequence = REVIEW_TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-review-{kind}-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(review_error("Create review temporary directory")),
        }
    }
    Err(review_error("Allocate review temporary directory"))
}
