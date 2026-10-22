//! Bounded image header parsing without pixel decoding.
//!
//! Managed still images (Notes icons and covers, profile and project images) accept PNG,
//! JPEG, and WebP. Displayed images (Quick note images) also accept GIF and report the
//! dimensions a browser renders, after JPEG EXIF orientation.

use std::{error::Error, fmt};

/// Maximum width or height accepted by managed image assets.
pub const MANAGED_IMAGE_MAX_DIMENSION_PIXELS: u32 = 8192;
/// Maximum decoded pixel count accepted by managed image assets.
pub const MANAGED_IMAGE_MAX_TOTAL_PIXELS: u64 = 16_000_000;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const EXIF_PREFIX: &[u8; 6] = b"Exif\0\0";
const EXIF_ORIENTATION_TAG: u16 = 0x0112;
const EXIF_SHORT_TYPE: u16 = 3;

/// Image encoding identified from its binary header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedImageKind {
    Png,
    Jpeg,
    Webp,
    /// Only [`parse_display_image_metadata`] accepts GIF.
    Gif,
}

impl ManagedImageKind {
    /// Every encoding, in a stable order.
    pub const ALL: [Self; 4] = [Self::Png, Self::Jpeg, Self::Webp, Self::Gif];

    /// Canonical MIME type for the image encoding.
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
        }
    }

    /// Canonical file extension for the image encoding.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }

    /// Resolve a supported MIME type, ignoring MIME parameters such as `charset`.
    pub fn from_mime_type(value: &str) -> Option<Self> {
        let value = value.split(';').next()?.trim();
        Self::ALL
            .into_iter()
            .find(|kind| value.eq_ignore_ascii_case(kind.mime_type()))
    }

    /// Resolve a canonical file extension.
    pub fn from_extension(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.extension() == value)
    }

    /// Return whether a declared MIME type matches the sniffed encoding.
    pub fn matches_mime_type(self, value: &str) -> bool {
        Self::from_mime_type(value) == Some(self)
    }

    /// Return whether a file extension matches the sniffed encoding.
    pub fn matches_extension(self, value: &str) -> bool {
        match self {
            Self::Png => value.eq_ignore_ascii_case("png"),
            Self::Jpeg => value.eq_ignore_ascii_case("jpg") || value.eq_ignore_ascii_case("jpeg"),
            Self::Webp => value.eq_ignore_ascii_case("webp"),
            Self::Gif => value.eq_ignore_ascii_case("gif"),
        }
    }
}

/// Dimensions and encoding read from an image header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManagedImageMetadata {
    pub kind: ManagedImageKind,
    pub width: u32,
    pub height: u32,
}

/// Failure to identify a complete supported image header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedImageMetadataError {
    UnsupportedFormat,
    MalformedHeader(&'static str),
}

impl fmt::Display for ManagedImageMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFormat => formatter.write_str("format is not a supported image type"),
            Self::MalformedHeader(reason) => write!(formatter, "header is malformed: {reason}"),
        }
    }
}

impl Error for ManagedImageMetadataError {}

/// Decoded image geometry that exceeds the managed-asset policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedImageDimensionError {
    DimensionTooLarge {
        width: u32,
        height: u32,
        maximum: u32,
    },
    TooManyPixels {
        width: u32,
        height: u32,
        maximum: u64,
    },
}

impl fmt::Display for ManagedImageDimensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DimensionTooLarge {
                width,
                height,
                maximum,
            } => write!(
                formatter,
                "dimensions {width}x{height} exceed {maximum} pixels per side"
            ),
            Self::TooManyPixels {
                width,
                height,
                maximum,
            } => write!(
                formatter,
                "dimensions {width}x{height} exceed the {maximum} pixel limit"
            ),
        }
    }
}

impl Error for ManagedImageDimensionError {}

fn read_u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u24_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(3)?)?;
    Some(u32::from(value[0]) | (u32::from(value[1]) << 8) | (u32::from(value[2]) << 16))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn metadata(
    kind: ManagedImageKind,
    width: u32,
    height: u32,
) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    if width == 0 || height == 0 {
        return Err(ManagedImageMetadataError::MalformedHeader(
            "dimensions must be positive",
        ));
    }
    Ok(ManagedImageMetadata {
        kind,
        width,
        height,
    })
}

fn parse_png(bytes: &[u8]) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    if bytes.len() < 33 {
        return Err(ManagedImageMetadataError::MalformedHeader(
            "PNG IHDR is truncated",
        ));
    }
    if read_u32_be(bytes, 8) != Some(13) || bytes.get(12..16) != Some(b"IHDR") {
        return Err(ManagedImageMetadataError::MalformedHeader(
            "PNG must begin with a 13-byte IHDR chunk",
        ));
    }
    metadata(
        ManagedImageKind::Png,
        read_u32_be(bytes, 16).unwrap_or(0),
        read_u32_be(bytes, 20).unwrap_or(0),
    )
}

fn is_jpeg_sof_marker(marker: u8) -> bool {
    matches!(
        marker,
        0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
    )
}

/// Orientation (1 to 8) from a JPEG APP1 payload, or `None` when the payload is not EXIF or
/// carries no well-formed IFD0 orientation entry.
fn exif_orientation(payload: &[u8]) -> Option<u16> {
    let tiff = payload.strip_prefix(EXIF_PREFIX)?;
    let little_endian = match tiff.get(..4)? {
        b"II*\0" => true,
        b"MM\0*" => false,
        _ => return None,
    };
    let u16_at = |offset| {
        if little_endian {
            read_u16_le(tiff, offset)
        } else {
            read_u16_be(tiff, offset)
        }
    };
    let u32_at = |offset| {
        if little_endian {
            read_u32_le(tiff, offset)
        } else {
            read_u32_be(tiff, offset)
        }
    };
    let directory = usize::try_from(u32_at(4)?).ok()?;
    let entries = usize::from(u16_at(directory)?);
    for index in 0..entries {
        let entry = directory
            .checked_add(2)?
            .checked_add(index.checked_mul(12)?)?;
        if u16_at(entry)? != EXIF_ORIENTATION_TAG {
            continue;
        }
        if u16_at(entry + 2)? != EXIF_SHORT_TYPE || u32_at(entry + 4)? != 1 {
            return None;
        }
        let orientation = u16_at(entry + 8)?;
        return (1..=8).contains(&orientation).then_some(orientation);
    }
    None
}

/// JPEG frame dimensions and the EXIF orientation (1 when absent) found before the frame.
fn parse_jpeg_frame(
    bytes: &[u8],
) -> Result<(ManagedImageMetadata, u16), ManagedImageMetadataError> {
    let mut orientation = None;
    let mut offset = 2usize;
    while offset < bytes.len() {
        if bytes[offset] != 0xff {
            return Err(ManagedImageMetadataError::MalformedHeader(
                "JPEG marker prefix is missing",
            ));
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let Some(&marker) = bytes.get(offset) else {
            return Err(ManagedImageMetadataError::MalformedHeader(
                "JPEG marker is truncated",
            ));
        };
        offset += 1;

        if matches!(marker, 0x00 | 0xd8 | 0xd9 | 0xda) {
            return Err(ManagedImageMetadataError::MalformedHeader(
                "JPEG reached image data before a SOF marker",
            ));
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        let segment_length = usize::from(read_u16_be(bytes, offset).ok_or(
            ManagedImageMetadataError::MalformedHeader("JPEG segment length is truncated"),
        )?);
        if segment_length < 2
            || offset
                .checked_add(segment_length)
                .is_none_or(|end| end > bytes.len())
        {
            return Err(ManagedImageMetadataError::MalformedHeader(
                "JPEG segment exceeds the inspected header",
            ));
        }
        if is_jpeg_sof_marker(marker) {
            if segment_length < 8 {
                return Err(ManagedImageMetadataError::MalformedHeader(
                    "JPEG SOF segment is too short",
                ));
            }
            let frame = metadata(
                ManagedImageKind::Jpeg,
                u32::from(read_u16_be(bytes, offset + 5).unwrap_or(0)),
                u32::from(read_u16_be(bytes, offset + 3).unwrap_or(0)),
            )?;
            return Ok((frame, orientation.unwrap_or(1)));
        }
        if marker == 0xe1 && orientation.is_none() {
            orientation = exif_orientation(&bytes[offset + 2..offset + segment_length]);
        }
        offset += segment_length;
    }
    Err(ManagedImageMetadataError::MalformedHeader(
        "JPEG SOF marker was not found",
    ))
}

fn parse_jpeg(bytes: &[u8]) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    parse_jpeg_frame(bytes).map(|(frame, _)| frame)
}

fn parse_gif(bytes: &[u8]) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    metadata(
        ManagedImageKind::Gif,
        u32::from(
            read_u16_le(bytes, 6).ok_or(ManagedImageMetadataError::MalformedHeader(
                "GIF logical screen descriptor is truncated",
            ))?,
        ),
        u32::from(
            read_u16_le(bytes, 8).ok_or(ManagedImageMetadataError::MalformedHeader(
                "GIF logical screen descriptor is truncated",
            ))?,
        ),
    )
}

fn parse_webp(bytes: &[u8]) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    if bytes.len() < 20 {
        return Err(ManagedImageMetadataError::MalformedHeader(
            "WebP chunk header is truncated",
        ));
    }
    let riff_size = read_u32_le(bytes, 4).unwrap_or(0);
    let chunk_size = read_u32_le(bytes, 16).unwrap_or(0);
    let minimum_riff_size =
        12u32
            .checked_add(chunk_size)
            .ok_or(ManagedImageMetadataError::MalformedHeader(
                "WebP chunk length overflows the container",
            ))?;
    if riff_size < minimum_riff_size {
        return Err(ManagedImageMetadataError::MalformedHeader(
            "WebP RIFF length is smaller than its first chunk",
        ));
    }

    match bytes.get(12..16) {
        Some(b"VP8X") => {
            if chunk_size < 10 || bytes.len() < 30 {
                return Err(ManagedImageMetadataError::MalformedHeader(
                    "WebP VP8X header is truncated",
                ));
            }
            metadata(
                ManagedImageKind::Webp,
                read_u24_le(bytes, 24).unwrap_or(0) + 1,
                read_u24_le(bytes, 27).unwrap_or(0) + 1,
            )
        }
        Some(b"VP8L") => {
            if chunk_size < 5 || bytes.len() < 25 || bytes[20] != 0x2f {
                return Err(ManagedImageMetadataError::MalformedHeader(
                    "WebP VP8L header is invalid",
                ));
            }
            let width = 1 + u32::from(bytes[21]) + (u32::from(bytes[22] & 0x3f) << 8);
            let height = 1
                + u32::from(bytes[22] >> 6)
                + (u32::from(bytes[23]) << 2)
                + (u32::from(bytes[24] & 0x0f) << 10);
            metadata(ManagedImageKind::Webp, width, height)
        }
        Some(b"VP8 ") => {
            if chunk_size < 10 || bytes.len() < 30 || bytes.get(23..26) != Some(&[0x9d, 0x01, 0x2a])
            {
                return Err(ManagedImageMetadataError::MalformedHeader(
                    "WebP VP8 frame header is invalid",
                ));
            }
            metadata(
                ManagedImageKind::Webp,
                u32::from(read_u16_le(bytes, 26).unwrap_or(0) & 0x3fff),
                u32::from(read_u16_le(bytes, 28).unwrap_or(0) & 0x3fff),
            )
        }
        _ => Err(ManagedImageMetadataError::MalformedHeader(
            "WebP first chunk must be VP8, VP8L, or VP8X",
        )),
    }
}

/// Parse PNG, JPEG SOF, or WebP dimensions from bounded header bytes.
///
/// This validates the metadata needed for managed-asset policy. It does not
/// replace complete image decoding or prove that the entire file is valid.
pub fn parse_managed_image_metadata(
    bytes: &[u8],
) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    if bytes.starts_with(PNG_SIGNATURE) {
        parse_png(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        parse_jpeg(bytes)
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        parse_webp(bytes)
    } else {
        Err(ManagedImageMetadataError::UnsupportedFormat)
    }
}

/// Parse PNG, JPEG, WebP, or GIF dimensions as a browser displays them.
///
/// JPEG dimensions swap when the first EXIF orientation rotates by a quarter turn
/// (orientations 5 to 8). Orientation in WebP and PNG metadata is ignored, so those images
/// report their stored dimensions. Like [`parse_managed_image_metadata`], this inspects headers
/// only and does not prove that the whole file decodes.
pub fn parse_display_image_metadata(
    bytes: &[u8],
) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        let (frame, orientation) = parse_jpeg_frame(bytes)?;
        return Ok(if orientation >= 5 {
            ManagedImageMetadata {
                width: frame.height,
                height: frame.width,
                ..frame
            }
        } else {
            frame
        });
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return parse_gif(bytes);
    }
    parse_managed_image_metadata(bytes)
}

/// Enforce width, height, and total decoded-pixel limits.
pub fn validate_image_dimensions(
    metadata: ManagedImageMetadata,
    max_dimension: u32,
    max_pixels: u64,
) -> Result<(), ManagedImageDimensionError> {
    if metadata.width > max_dimension || metadata.height > max_dimension {
        return Err(ManagedImageDimensionError::DimensionTooLarge {
            width: metadata.width,
            height: metadata.height,
            maximum: max_dimension,
        });
    }
    let pixels = u64::from(metadata.width) * u64::from(metadata.height);
    if pixels > max_pixels {
        return Err(ManagedImageDimensionError::TooManyPixels {
            width: metadata.width,
            height: metadata.height,
            maximum: max_pixels,
        });
    }
    Ok(())
}

/// Enforce managed-image width, height, and total decoded-pixel limits.
pub fn validate_managed_image_dimensions(
    metadata: ManagedImageMetadata,
) -> Result<(), ManagedImageDimensionError> {
    validate_image_dimensions(
        metadata,
        MANAGED_IMAGE_MAX_DIMENSION_PIXELS,
        MANAGED_IMAGE_MAX_TOTAL_PIXELS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 33];
        bytes[..8].copy_from_slice(PNG_SIGNATURE);
        bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes[24..29].copy_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg(width: u16, height: u16, sof_marker: u8) -> Vec<u8> {
        let mut bytes = vec![0; 27];
        bytes[..13].copy_from_slice(&[
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0, 0, 0xff, sof_marker, 0x00, 0x11, 8,
        ]);
        bytes[13..15].copy_from_slice(&height.to_be_bytes());
        bytes[15..17].copy_from_slice(&width.to_be_bytes());
        bytes
    }

    fn webp_header(chunk: &[u8; 4], chunk_size: u32, length: usize) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[4..8].copy_from_slice(&(12 + chunk_size).to_le_bytes());
        bytes[8..12].copy_from_slice(b"WEBP");
        bytes[12..16].copy_from_slice(chunk);
        bytes[16..20].copy_from_slice(&chunk_size.to_le_bytes());
        bytes
    }

    fn webp_vp8x(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = webp_header(b"VP8X", 10, 30);
        let width = (width - 1).to_le_bytes();
        let height = (height - 1).to_le_bytes();
        bytes[24..27].copy_from_slice(&width[..3]);
        bytes[27..30].copy_from_slice(&height[..3]);
        bytes
    }

    fn webp_vp8l(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = webp_header(b"VP8L", 5, 25);
        let width = width - 1;
        let height = height - 1;
        bytes[20] = 0x2f;
        bytes[21] = width as u8;
        bytes[22] = ((width >> 8) as u8 & 0x3f) | ((height as u8 & 0x03) << 6);
        bytes[23] = (height >> 2) as u8;
        bytes[24] = ((height >> 10) as u8) & 0x0f;
        bytes
    }

    fn webp_vp8(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = webp_header(b"VP8 ", 10, 30);
        bytes[23..26].copy_from_slice(&[0x9d, 0x01, 0x2a]);
        bytes[26..28].copy_from_slice(&width.to_le_bytes());
        bytes[28..30].copy_from_slice(&height.to_le_bytes());
        bytes
    }

    #[test]
    fn parses_png_and_jpeg_sof_dimensions() {
        assert_eq!(
            parse_managed_image_metadata(&png(4032, 3024)).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Png,
                width: 4032,
                height: 3024,
            }
        );
        assert_eq!(
            parse_managed_image_metadata(&jpeg(4096, 2160, 0xc2)).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Jpeg,
                width: 4096,
                height: 2160,
            }
        );
    }

    #[test]
    fn parses_all_supported_webp_dimension_headers() {
        assert_eq!(
            parse_managed_image_metadata(&webp_vp8(1920, 1080)).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Webp,
                width: 1920,
                height: 1080,
            }
        );
        assert_eq!(
            parse_managed_image_metadata(&webp_vp8l(4000, 3000)).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Webp,
                width: 4000,
                height: 3000,
            }
        );
        assert_eq!(
            parse_managed_image_metadata(&webp_vp8x(8192, 1200)).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Webp,
                width: 8192,
                height: 1200,
            }
        );
    }

    #[test]
    fn rejects_unknown_truncated_and_zero_dimension_headers() {
        assert_eq!(
            parse_managed_image_metadata(b"GIF89a"),
            Err(ManagedImageMetadataError::UnsupportedFormat)
        );
        assert!(matches!(
            parse_managed_image_metadata(&png(1, 1)[..23]),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
        assert!(matches!(
            parse_managed_image_metadata(&png(0, 100)),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
        assert!(matches!(
            parse_managed_image_metadata(&jpeg(100, 0, 0xc0)),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
        assert!(matches!(
            parse_managed_image_metadata(&webp_vp8(0, 100)),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
    }

    #[test]
    fn rejects_malformed_jpeg_segments_and_webp_chunks() {
        assert!(matches!(
            parse_managed_image_metadata(&[0xff, 0xd8, 0xff, 0xe0, 0xff, 0xff]),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
        let mut webp = webp_vp8x(10, 10);
        webp[12..16].copy_from_slice(b"JUNK");
        assert!(matches!(
            parse_managed_image_metadata(&webp),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
    }

    #[test]
    fn accepts_normal_phone_photos_and_enforces_both_geometry_limits() {
        let normal_phone_photo = parse_managed_image_metadata(&jpeg(4032, 3024, 0xc0)).unwrap();
        assert_eq!(
            validate_managed_image_dimensions(normal_phone_photo),
            Ok(())
        );
        assert!(matches!(
            validate_managed_image_dimensions(ManagedImageMetadata {
                kind: ManagedImageKind::Png,
                width: MANAGED_IMAGE_MAX_DIMENSION_PIXELS + 1,
                height: 1,
            }),
            Err(ManagedImageDimensionError::DimensionTooLarge { .. })
        ));
        assert!(matches!(
            validate_managed_image_dimensions(ManagedImageMetadata {
                kind: ManagedImageKind::Png,
                width: 5000,
                height: 4000,
            }),
            Err(ManagedImageDimensionError::TooManyPixels { .. })
        ));
    }

    #[test]
    fn mime_and_extension_matching_use_canonical_image_types() {
        assert!(ManagedImageKind::Jpeg.matches_mime_type(" image/jpeg; charset=binary"));
        assert!(!ManagedImageKind::Jpeg.matches_mime_type("image/png"));
        assert!(ManagedImageKind::Jpeg.matches_extension("JPEG"));
        assert!(!ManagedImageKind::Webp.matches_extension("png"));
        assert_eq!(
            ManagedImageKind::from_mime_type("IMAGE/GIF"),
            Some(ManagedImageKind::Gif)
        );
        assert_eq!(
            ManagedImageKind::from_extension("webp"),
            Some(ManagedImageKind::Webp)
        );
        assert_eq!(ManagedImageKind::from_extension("jpeg"), None);
    }

    fn exif_jpeg(width: u16, height: u16, orientation: u16, little_endian: bool) -> Vec<u8> {
        let u16_bytes = |value: u16| {
            if little_endian {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            }
        };
        let u32_bytes = |value: u32| {
            if little_endian {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            }
        };
        let mut payload = EXIF_PREFIX.to_vec();
        payload.extend_from_slice(if little_endian { b"II*\0" } else { b"MM\0*" });
        payload.extend_from_slice(&u32_bytes(8));
        payload.extend_from_slice(&u16_bytes(1));
        payload.extend_from_slice(&u16_bytes(EXIF_ORIENTATION_TAG));
        payload.extend_from_slice(&u16_bytes(EXIF_SHORT_TYPE));
        payload.extend_from_slice(&u32_bytes(1));
        payload.extend_from_slice(&u16_bytes(orientation));
        payload.extend_from_slice(&[0, 0]);
        payload.extend_from_slice(&u32_bytes(0));

        let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
        bytes.extend_from_slice(&(payload.len() as u16 + 2).to_be_bytes());
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&jpeg(width, height, 0xc0)[2..]);
        bytes
    }

    fn display(bytes: &[u8]) -> (u32, u32) {
        let metadata = parse_display_image_metadata(bytes).unwrap();
        (metadata.width, metadata.height)
    }

    #[test]
    fn display_dimensions_follow_quarter_turn_exif_orientation_in_both_byte_orders() {
        assert_eq!(display(&exif_jpeg(4000, 3000, 6, true)), (3000, 4000));
        assert_eq!(display(&exif_jpeg(4000, 3000, 8, false)), (3000, 4000));
        assert_eq!(display(&exif_jpeg(4000, 3000, 3, true)), (4000, 3000));
        assert_eq!(display(&exif_jpeg(4000, 3000, 1, false)), (4000, 3000));
        assert_eq!(
            parse_managed_image_metadata(&exif_jpeg(4000, 3000, 6, true))
                .unwrap()
                .width,
            4000
        );
    }

    #[test]
    fn malformed_or_out_of_range_exif_keeps_stored_dimensions() {
        assert_eq!(display(&exif_jpeg(40, 30, 9, true)), (40, 30));
        let mut wrong_type = exif_jpeg(40, 30, 6, true);
        let type_offset = 4 + 2 + EXIF_PREFIX.len() + 8 + 2 + 2;
        wrong_type[type_offset] = 4;
        assert_eq!(display(&wrong_type), (40, 30));
        let mut bad_directory = exif_jpeg(40, 30, 6, false);
        let directory_offset = 4 + 2 + EXIF_PREFIX.len() + 4;
        bad_directory[directory_offset..directory_offset + 4]
            .copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(display(&bad_directory), (40, 30));
    }

    #[test]
    fn gif_is_accepted_only_for_displayed_images() {
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&640u16.to_le_bytes());
        gif.extend_from_slice(&480u16.to_le_bytes());
        assert_eq!(
            parse_display_image_metadata(&gif).unwrap(),
            ManagedImageMetadata {
                kind: ManagedImageKind::Gif,
                width: 640,
                height: 480,
            }
        );
        assert_eq!(
            parse_managed_image_metadata(&gif),
            Err(ManagedImageMetadataError::UnsupportedFormat)
        );
        assert!(matches!(
            parse_display_image_metadata(&gif[..9]),
            Err(ManagedImageMetadataError::MalformedHeader(_))
        ));
        let mut empty = gif.clone();
        empty[6..8].copy_from_slice(&0u16.to_le_bytes());
        assert!(parse_display_image_metadata(&empty).is_err());
    }

    #[test]
    fn configurable_dimension_limits_apply_to_both_sides_and_pixel_count() {
        let wide = ManagedImageMetadata {
            kind: ManagedImageKind::Png,
            width: 16_384,
            height: 3_000,
        };
        assert_eq!(validate_image_dimensions(wide, 16_384, 64_000_000), Ok(()));
        assert!(matches!(
            validate_image_dimensions(wide, 16_383, 64_000_000),
            Err(ManagedImageDimensionError::DimensionTooLarge { .. })
        ));
        assert!(matches!(
            validate_image_dimensions(wide, 16_384, 40_000_000),
            Err(ManagedImageDimensionError::TooManyPixels { .. })
        ));
    }
}
