//! Bounded managed-image metadata parsing without pixel decoding.

use std::{error::Error, fmt};

/// Maximum width or height accepted by managed image assets.
pub const MANAGED_IMAGE_MAX_DIMENSION_PIXELS: u32 = 8192;
/// Maximum decoded pixel count accepted by managed image assets.
pub const MANAGED_IMAGE_MAX_TOTAL_PIXELS: u64 = 16_000_000;

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Managed image encoding identified from its binary header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedImageKind {
    Png,
    Jpeg,
    Webp,
}

impl ManagedImageKind {
    /// Canonical MIME type for the image encoding.
    pub const fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    /// Canonical file extension for the image encoding.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    /// Resolve a supported MIME type, ignoring MIME parameters such as `charset`.
    pub fn from_mime_type(value: &str) -> Option<Self> {
        let value = value.split(';').next()?.trim();
        if value.eq_ignore_ascii_case("image/png") {
            Some(Self::Png)
        } else if value.eq_ignore_ascii_case("image/jpeg") {
            Some(Self::Jpeg)
        } else if value.eq_ignore_ascii_case("image/webp") {
            Some(Self::Webp)
        } else {
            None
        }
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
            Self::UnsupportedFormat => formatter.write_str("format must be PNG, JPEG, or WebP"),
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

fn parse_jpeg(bytes: &[u8]) -> Result<ManagedImageMetadata, ManagedImageMetadataError> {
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
            return metadata(
                ManagedImageKind::Jpeg,
                u32::from(read_u16_be(bytes, offset + 5).unwrap_or(0)),
                u32::from(read_u16_be(bytes, offset + 3).unwrap_or(0)),
            );
        }
        offset += segment_length;
    }
    Err(ManagedImageMetadataError::MalformedHeader(
        "JPEG SOF marker was not found",
    ))
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

/// Enforce managed-image width, height, and total decoded-pixel limits.
pub fn validate_managed_image_dimensions(
    metadata: ManagedImageMetadata,
) -> Result<(), ManagedImageDimensionError> {
    if metadata.width > MANAGED_IMAGE_MAX_DIMENSION_PIXELS
        || metadata.height > MANAGED_IMAGE_MAX_DIMENSION_PIXELS
    {
        return Err(ManagedImageDimensionError::DimensionTooLarge {
            width: metadata.width,
            height: metadata.height,
            maximum: MANAGED_IMAGE_MAX_DIMENSION_PIXELS,
        });
    }
    let pixels = u64::from(metadata.width) * u64::from(metadata.height);
    if pixels > MANAGED_IMAGE_MAX_TOTAL_PIXELS {
        return Err(ManagedImageDimensionError::TooManyPixels {
            width: metadata.width,
            height: metadata.height,
            maximum: MANAGED_IMAGE_MAX_TOTAL_PIXELS,
        });
    }
    Ok(())
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
    }
}
