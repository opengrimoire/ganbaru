//! Quick notes adapter: the canonical text run and image list codecs, the plain text projection,
//! and lifecycle priorities. The app normalizes and projects bodies with these functions, so
//! local writes and merged values agree byte for byte.

use std::fmt;

use ganbaru_sync_contracts::codec::{Reader, Writer};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::quick_notes::{ADAPTER, NOTES_TABLE, note_group};

/// Text runs in one body.
pub const MAX_RUNS: usize = 4_096;
/// Characters in one body.
pub const MAX_BODY_CHARS: usize = 65_536;
/// Characters of body text in a recovery preview.
pub const PREVIEW_CHARS: usize = 240;

const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDERLINE: u8 = 4;
const STYLE_BITS: u8 = BOLD | ITALIC | UNDERLINE;
/// Largest UTF-8 length of one run: every character of a full body in four bytes.
const MAX_RUN_BYTES: usize = MAX_BODY_CHARS * 4;

/// One styled run of note body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    /// Run text, never empty in a canonical list.
    pub content: String,
    /// Bold style.
    pub bold: bool,
    /// Italic style.
    pub italic: bool,
    /// Underline style.
    pub underline: bool,
}

impl TextRun {
    fn style(&self) -> u8 {
        (u8::from(self.bold) * BOLD)
            | (u8::from(self.italic) * ITALIC)
            | (u8::from(self.underline) * UNDERLINE)
    }

    fn from_style(content: String, style: u8) -> Self {
        Self {
            content,
            bold: style & BOLD != 0,
            italic: style & ITALIC != 0,
            underline: style & UNDERLINE != 0,
        }
    }
}

/// Why a run list cannot become a note body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError {
    /// More than [`MAX_RUNS`] runs were given.
    TooManyRuns,
    /// The text exceeds [`MAX_BODY_CHARS`].
    TooLong,
    /// The text contains a NUL character.
    Nul,
}

impl fmt::Display for RunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRuns => write!(
                formatter,
                "quick note body cannot contain more than {MAX_RUNS} text runs"
            ),
            Self::TooLong => write!(
                formatter,
                "quick note body cannot exceed {MAX_BODY_CHARS} characters"
            ),
            Self::Nul => formatter.write_str("quick note body cannot contain NUL characters"),
        }
    }
}

impl std::error::Error for RunError {}

/// Canonical form of a run list: empty runs dropped and adjacent runs of equal style merged.
pub fn normalize_runs(runs: &[TextRun]) -> Result<Vec<TextRun>, RunError> {
    if runs.len() > MAX_RUNS {
        return Err(RunError::TooManyRuns);
    }
    let mut output: Vec<TextRun> = Vec::new();
    let mut chars = 0;
    for run in runs.iter().filter(|run| !run.content.is_empty()) {
        if run.content.contains('\0') {
            return Err(RunError::Nul);
        }
        chars += run.content.chars().count();
        if chars > MAX_BODY_CHARS {
            return Err(RunError::TooLong);
        }
        match output.last_mut() {
            Some(previous) if previous.style() == run.style() => {
                previous.content.push_str(&run.content);
            }
            _ => output.push(run.clone()),
        }
    }
    Ok(output)
}

/// Plain text of a body: its runs concatenated.
pub fn plain_text(runs: &[TextRun]) -> String {
    runs.iter().map(|run| run.content.as_str()).collect()
}

/// Encodes a run list: a u32 count, then per run a style byte and u32-length UTF-8 text.
pub fn encode_runs(runs: &[TextRun]) -> Vec<u8> {
    let mut writer =
        Writer::with_capacity(4 + runs.iter().map(|run| 5 + run.content.len()).sum::<usize>());
    writer.u32(runs.len() as u32);
    for run in runs {
        writer.u8(run.style());
        writer.bytes_u32(run.content.as_bytes());
    }
    writer.into_bytes()
}

/// Decodes a canonical run list, refusing any other encoding.
pub fn decode_runs(bytes: &[u8]) -> Result<Vec<TextRun>, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note body");
    let mut reader = Reader::new(bytes);
    let count = reader.u32().map_err(|_| INVALID)? as usize;
    if count > MAX_RUNS {
        return Err(INVALID);
    }
    let mut runs: Vec<TextRun> = Vec::with_capacity(count.min(reader.remaining() / 6));
    let mut chars = 0;
    for _ in 0..count {
        let style = reader.u8().map_err(|_| INVALID)?;
        if style & !STYLE_BITS != 0 {
            return Err(INVALID);
        }
        let text = reader
            .bytes_u32(MAX_RUN_BYTES, "run")
            .map_err(|_| INVALID)?;
        let text = std::str::from_utf8(text).map_err(|_| INVALID)?;
        if text.is_empty() || text.contains('\0') {
            return Err(INVALID);
        }
        chars += text.chars().count();
        if chars > MAX_BODY_CHARS {
            return Err(INVALID);
        }
        if runs
            .last()
            .is_some_and(|previous| previous.style() == style)
        {
            return Err(INVALID);
        }
        runs.push(TextRun::from_style(text.to_owned(), style));
    }
    reader.finish().map_err(|_| INVALID)?;
    Ok(runs)
}

/// Images attached to one note.
pub const MAX_IMAGES: usize = 24;
/// Bytes in one image file.
pub const MAX_IMAGE_BYTES: u32 = 25 * 1024 * 1024;
/// Displayed width or height of one image.
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;
/// Displayed pixels of one image.
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;
/// Content types an image may have, indexed by their codec byte.
pub const IMAGE_CONTENT_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const SHA256_BYTES: usize = 32;

/// A reference from a note to a content-addressed image file in the vault.
///
/// The file is named by the lowercase hex SHA-256 of its bytes. Width and height are the
/// displayed dimensions, after orientation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteImage {
    /// Lowercase hex SHA-256 of the file bytes.
    pub sha256: String,
    /// One of [`IMAGE_CONTENT_TYPES`].
    pub content_type: String,
    /// File length in bytes.
    pub byte_size: u32,
    /// Displayed width in pixels.
    pub width: u32,
    /// Displayed height in pixels.
    pub height: u32,
}

/// Why an image list cannot belong to a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageError {
    /// More than [`MAX_IMAGES`] images were given.
    TooManyImages,
    /// The same file appears twice.
    Duplicate,
    /// The digest is not 64 lowercase hex characters.
    Digest,
    /// The content type is not one of [`IMAGE_CONTENT_TYPES`].
    ContentType,
    /// The byte size is zero or exceeds [`MAX_IMAGE_BYTES`].
    Size,
    /// A dimension is zero, or the geometry exceeds [`MAX_IMAGE_DIMENSION`] or
    /// [`MAX_IMAGE_PIXELS`].
    Dimensions,
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyImages => write!(
                formatter,
                "a quick note cannot contain more than {MAX_IMAGES} images"
            ),
            Self::Duplicate => formatter.write_str("a quick note cannot contain an image twice"),
            Self::Digest => formatter.write_str("quick note image digest is invalid"),
            Self::ContentType => formatter.write_str("quick note image type is unsupported"),
            Self::Size => write!(
                formatter,
                "quick note images must be between 1 and {MAX_IMAGE_BYTES} bytes"
            ),
            Self::Dimensions => formatter.write_str("quick note image dimensions are too large"),
        }
    }
}

impl std::error::Error for ImageError {}

fn content_type_code(content_type: &str) -> Option<u8> {
    IMAGE_CONTENT_TYPES
        .iter()
        .position(|candidate| *candidate == content_type)
        .and_then(|index| u8::try_from(index).ok())
}

fn digest_bytes(sha256: &str) -> Option<[u8; SHA256_BYTES]> {
    let text = sha256.as_bytes();
    if text.len() != SHA256_BYTES * 2 {
        return None;
    }
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let mut digest = [0; SHA256_BYTES];
    for (index, [high, low]) in text.as_chunks::<2>().0.iter().enumerate() {
        digest[index] = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(digest)
}

fn digest_hex(digest: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    digest
        .iter()
        .flat_map(|byte| [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]])
        .map(char::from)
        .collect()
}

/// Checks one image reference against the codec bounds.
pub fn validate_image(image: &NoteImage) -> Result<(), ImageError> {
    if digest_bytes(&image.sha256).is_none() {
        return Err(ImageError::Digest);
    }
    if content_type_code(&image.content_type).is_none() {
        return Err(ImageError::ContentType);
    }
    if image.byte_size == 0 || image.byte_size > MAX_IMAGE_BYTES {
        return Err(ImageError::Size);
    }
    if image.width == 0
        || image.height == 0
        || image.width > MAX_IMAGE_DIMENSION
        || image.height > MAX_IMAGE_DIMENSION
        || u64::from(image.width) * u64::from(image.height) > MAX_IMAGE_PIXELS
    {
        return Err(ImageError::Dimensions);
    }
    Ok(())
}

/// Checks a whole image list: count, every reference, and distinct files.
pub fn validate_images(images: &[NoteImage]) -> Result<(), ImageError> {
    if images.len() > MAX_IMAGES {
        return Err(ImageError::TooManyImages);
    }
    for (index, image) in images.iter().enumerate() {
        validate_image(image)?;
        if images[..index]
            .iter()
            .any(|previous| previous.sha256 == image.sha256)
        {
            return Err(ImageError::Duplicate);
        }
    }
    Ok(())
}

/// Encodes an image list: a u32 count, then per image the 32 digest bytes, a content type
/// byte, and u32 byte size, width, and height. Fails on a reference the codec cannot carry.
pub fn encode_images(images: &[NoteImage]) -> Result<Vec<u8>, ImageError> {
    let count = u32::try_from(images.len()).map_err(|_| ImageError::TooManyImages)?;
    let mut writer = Writer::with_capacity(4 + images.len() * (SHA256_BYTES + 13));
    writer.u32(count);
    for image in images {
        writer.raw(&digest_bytes(&image.sha256).ok_or(ImageError::Digest)?);
        writer.u8(content_type_code(&image.content_type).ok_or(ImageError::ContentType)?);
        writer.u32(image.byte_size);
        writer.u32(image.width);
        writer.u32(image.height);
    }
    Ok(writer.into_bytes())
}

/// Decodes a canonical image list, refusing any list [`validate_images`] refuses.
pub fn decode_images(bytes: &[u8]) -> Result<Vec<NoteImage>, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note images");
    let mut reader = Reader::new(bytes);
    let count = reader.u32().map_err(|_| INVALID)? as usize;
    if count > MAX_IMAGES {
        return Err(INVALID);
    }
    let mut images = Vec::with_capacity(count);
    for _ in 0..count {
        let digest = reader.array::<SHA256_BYTES>().map_err(|_| INVALID)?;
        let content_type = IMAGE_CONTENT_TYPES
            .get(usize::from(reader.u8().map_err(|_| INVALID)?))
            .ok_or(INVALID)?;
        images.push(NoteImage {
            sha256: digest_hex(&digest),
            content_type: (*content_type).to_owned(),
            byte_size: reader.u32().map_err(|_| INVALID)?,
            width: reader.u32().map_err(|_| INVALID)?,
            height: reader.u32().map_err(|_| INVALID)?,
        });
    }
    reader.finish().map_err(|_| INVALID)?;
    validate_images(&images).map_err(|_| INVALID)?;
    Ok(images)
}

/// The Quick notes adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickNotesAdapter;

fn body_blob(value: &Value) -> Result<&[u8], AdapterError> {
    match value.fields() {
        [Field::Blob(bytes)] => Ok(bytes),
        _ => Err(AdapterError("quick note body")),
    }
}

fn run_from_fields(fields: Vec<Field>) -> Result<TextRun, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note text run");
    let flag = |field: &Field| match field {
        Field::Integer(0) => Ok(false),
        Field::Integer(1) => Ok(true),
        _ => Err(INVALID),
    };
    match fields.as_slice() {
        [Field::Text(_), bold, italic, underline] => {
            let (bold, italic, underline) = (flag(bold)?, flag(italic)?, flag(underline)?);
            let Some(Field::Text(content)) = fields.into_iter().next() else {
                return Err(INVALID);
            };
            Ok(TextRun {
                content,
                bold,
                italic,
                underline,
            })
        }
        _ => Err(INVALID),
    }
}

/// The single blob field of an image list value.
pub fn images_blob(value: &Value) -> Result<&[u8], AdapterError> {
    match value.fields() {
        [Field::Blob(bytes)] => Ok(bytes),
        _ => Err(AdapterError("quick note images")),
    }
}

fn image_from_fields(fields: Vec<Field>) -> Result<NoteImage, AdapterError> {
    const INVALID: AdapterError = AdapterError("quick note image");
    let dimension = |field: &Field| match field {
        Field::Integer(value) => u32::try_from(*value).map_err(|_| INVALID),
        _ => Err(INVALID),
    };
    let mut fields = fields.into_iter();
    match (
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
        fields.next(),
    ) {
        (
            Some(Field::Text(sha256)),
            Some(Field::Text(content_type)),
            Some(byte_size),
            Some(width),
            Some(height),
            None,
        ) => Ok(NoteImage {
            sha256,
            content_type,
            byte_size: dimension(&byte_size)?,
            width: dimension(&width)?,
            height: dimension(&height)?,
        }),
        _ => Err(INVALID),
    }
}

fn is_body(table: TableId, group: GroupId) -> bool {
    table == NOTES_TABLE && group == note_group::BODY
}

fn is_images(table: TableId, group: GroupId) -> bool {
    table == NOTES_TABLE && group == note_group::IMAGES
}

impl DomainAdapter for QuickNotesAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn encode_owned(
        &self,
        table: TableId,
        group: GroupId,
        rows: Vec<Vec<Field>>,
    ) -> Result<Value, AdapterError> {
        if is_images(table, group) {
            let images = rows
                .into_iter()
                .map(image_from_fields)
                .collect::<Result<Vec<_>, _>>()?;
            let bytes = encode_images(&images).map_err(|_| AdapterError("quick note images"))?;
            return Value::new(vec![Field::Blob(bytes)])
                .map_err(|_| AdapterError("quick note images"));
        }
        if !is_body(table, group) {
            return Err(AdapterError("owned group"));
        }
        let runs = rows
            .into_iter()
            .map(run_from_fields)
            .collect::<Result<Vec<_>, _>>()?;
        Value::new(vec![Field::Blob(encode_runs(&runs))])
            .map_err(|_| AdapterError("quick note body"))
    }

    fn decode_owned(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        if is_images(table, group) {
            return Ok(decode_images(images_blob(value)?)?
                .into_iter()
                .map(|image| {
                    vec![
                        Field::Text(image.sha256),
                        Field::Text(image.content_type),
                        Field::Integer(i64::from(image.byte_size)),
                        Field::Integer(i64::from(image.width)),
                        Field::Integer(i64::from(image.height)),
                    ]
                })
                .collect());
        }
        if !is_body(table, group) {
            return Err(AdapterError("owned group"));
        }
        Ok(decode_runs(body_blob(value)?)?
            .into_iter()
            .map(|run| {
                vec![
                    Field::Text(run.content),
                    Field::Integer(i64::from(run.bold)),
                    Field::Integer(i64::from(run.italic)),
                    Field::Integer(i64::from(run.underline)),
                ]
            })
            .collect())
    }

    fn derived(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        if !is_body(table, group) {
            return Ok(Vec::new());
        }
        let runs = decode_runs(body_blob(value)?)?;
        Ok(vec![("body_plain_text", Field::Text(plain_text(&runs)))])
    }

    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        if table != NOTES_TABLE || group != note_group::LIFECYCLE {
            return 0;
        }
        match value.fields() {
            [_, _, Field::Text(_)] => 2,
            [_, Field::Integer(1), _] => 1,
            _ => 0,
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        if table != NOTES_TABLE {
            return Ok(());
        }
        if group == note_group::BODY {
            decode_runs(body_blob(value)?)?;
        } else if group == note_group::IMAGES {
            decode_images(images_blob(value)?)?;
        } else if group == note_group::LIFECYCLE {
            if let [Field::Integer(1), archived, trashed_at] = value.fields() {
                if *archived != Field::Integer(0) || *trashed_at != Field::Null {
                    return Err(AdapterError("quick note lifecycle"));
                }
            }
        }
        Ok(())
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        if table != NOTES_TABLE {
            return presentation;
        }
        for (group, value) in values {
            if *group == note_group::TITLE {
                if let [Field::Text(title)] = value.fields() {
                    presentation.title = title.clone();
                }
            } else if *group == note_group::BODY {
                if let Ok(runs) = body_blob(value).and_then(decode_runs) {
                    presentation.preview = plain_text(&runs).chars().take(PREVIEW_CHARS).collect();
                }
            }
        }
        presentation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(seed: u8, content_type: &str) -> NoteImage {
        NoteImage {
            sha256: digest_hex(&[seed; SHA256_BYTES]),
            content_type: content_type.to_owned(),
            byte_size: 1_024,
            width: 4_000,
            height: 3_000,
        }
    }

    fn owned(images: &[NoteImage]) -> Vec<Vec<Field>> {
        images
            .iter()
            .map(|image| {
                vec![
                    Field::Text(image.sha256.clone()),
                    Field::Text(image.content_type.clone()),
                    Field::Integer(i64::from(image.byte_size)),
                    Field::Integer(i64::from(image.width)),
                    Field::Integer(i64::from(image.height)),
                ]
            })
            .collect()
    }

    #[test]
    fn image_lists_round_trip_through_owned_rows_in_order() {
        let images = vec![image(0xab, "image/gif"), image(0x01, "image/png")];
        let value = QuickNotesAdapter
            .encode_owned(NOTES_TABLE, note_group::IMAGES, owned(&images))
            .unwrap();
        assert_eq!(
            QuickNotesAdapter.validate(NOTES_TABLE, note_group::IMAGES, &value),
            Ok(())
        );
        assert_eq!(
            QuickNotesAdapter
                .decode_owned(NOTES_TABLE, note_group::IMAGES, &value)
                .unwrap(),
            owned(&images)
        );
        assert_eq!(decode_images(images_blob(&value).unwrap()).unwrap(), images);
        assert_eq!(
            QuickNotesAdapter
                .derived(NOTES_TABLE, note_group::IMAGES, &value)
                .unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn empty_image_list_is_canonical() {
        let value = QuickNotesAdapter
            .encode_owned(NOTES_TABLE, note_group::IMAGES, Vec::new())
            .unwrap();
        assert_eq!(images_blob(&value).unwrap(), &[0, 0, 0, 0]);
        assert_eq!(
            QuickNotesAdapter.validate(NOTES_TABLE, note_group::IMAGES, &value),
            Ok(())
        );
    }

    #[test]
    fn encoding_refuses_references_it_cannot_carry_unchanged() {
        let mut uppercase = image(0xab, "image/png");
        uppercase.sha256 = uppercase.sha256.to_ascii_uppercase();
        assert_eq!(encode_images(&[uppercase]), Err(ImageError::Digest));
        assert_eq!(
            encode_images(&[image(1, "image/bmp")]),
            Err(ImageError::ContentType)
        );
        let mut rows = owned(&[image(1, "image/png")]);
        rows[0][2] = Field::Integer(-1);
        assert!(
            QuickNotesAdapter
                .encode_owned(NOTES_TABLE, note_group::IMAGES, rows)
                .is_err()
        );
    }

    #[test]
    fn validation_refuses_duplicates_bounds_and_overlong_lists() {
        let duplicate = encode_images(&[image(1, "image/png"), image(1, "image/jpeg")]).unwrap();
        assert!(decode_images(&duplicate).is_err());

        let mut empty_file = image(1, "image/png");
        empty_file.byte_size = 0;
        assert_eq!(validate_image(&empty_file), Err(ImageError::Size));
        let mut huge = image(1, "image/png");
        huge.width = MAX_IMAGE_DIMENSION;
        huge.height = MAX_IMAGE_DIMENSION;
        assert_eq!(validate_image(&huge), Err(ImageError::Dimensions));
        assert!(decode_images(&encode_images(&[huge]).unwrap()).is_err());

        let many = (0..=MAX_IMAGES as u8)
            .map(|seed| image(seed, "image/webp"))
            .collect::<Vec<_>>();
        assert_eq!(validate_images(&many), Err(ImageError::TooManyImages));
        assert!(decode_images(&encode_images(&many).unwrap()).is_err());
    }

    #[test]
    fn decoding_refuses_unknown_types_and_trailing_bytes() {
        let mut bytes = encode_images(&[image(1, "image/png")]).unwrap();
        bytes[4 + SHA256_BYTES] = IMAGE_CONTENT_TYPES.len() as u8;
        assert!(decode_images(&bytes).is_err());
        let mut trailing = encode_images(&[image(1, "image/png")]).unwrap();
        trailing.push(0);
        assert!(decode_images(&trailing).is_err());
    }
}
