//! Music value rules shared by the library and the engine: the managed artwork reference.
//!
//! An artwork override is stored as `music/artwork/{sha256}.{ext}`, the class name of
//! [`ARTWORK_CLASS`] followed by the engine's asset file name, so the reference names exactly
//! one content-addressed file and never an arbitrary path.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::assets::{self, AssetRef};
use crate::manifest::vault::music::ARTWORK_CLASS;

/// Bytes in one artwork file.
pub const MAX_ARTWORK_BYTES: u32 = 10 * 1024 * 1024;

/// Content types artwork may have.
pub const ARTWORK_CONTENT_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// A value that is not a managed artwork reference, or a reference that cannot name an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidArtworkReference;

impl fmt::Display for InvalidArtworkReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "artwork must be a managed {} reference of a supported image",
            ARTWORK_CLASS.name
        )
    }
}

impl std::error::Error for InvalidArtworkReference {}

/// The file a managed artwork reference names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtworkFile {
    /// Lowercase hex SHA-256 of the file bytes.
    pub sha256: String,
    /// One of [`ARTWORK_CONTENT_TYPES`].
    pub content_type: String,
}

impl ArtworkFile {
    /// The asset this file is, given its exact size.
    pub fn asset(&self, byte_size: u32) -> Result<AssetRef, InvalidArtworkReference> {
        AssetRef::new(
            &ARTWORK_CLASS,
            self.sha256.clone(),
            self.content_type.clone(),
            byte_size,
        )
        .map_err(|_| InvalidArtworkReference)
    }

    /// The stored reference, `music/artwork/{sha256}.{ext}`.
    pub fn reference(&self) -> Result<String, InvalidArtworkReference> {
        artwork_reference(&self.sha256, &self.content_type)
    }

    /// The path of the file in a vault, the engine path of [`ARTWORK_CLASS`].
    pub fn path(&self, vault_root: &Path) -> Result<PathBuf, InvalidArtworkReference> {
        let name = assets::file_name(&self.sha256, &self.content_type)
            .map_err(|_| InvalidArtworkReference)?;
        Ok(ARTWORK_CLASS.directory(vault_root).join(name))
    }
}

/// The reference stored for an artwork file, after checking the digest and content type.
pub fn artwork_reference(
    sha256: &str,
    content_type: &str,
) -> Result<String, InvalidArtworkReference> {
    if !ARTWORK_CONTENT_TYPES.contains(&content_type) {
        return Err(InvalidArtworkReference);
    }
    let name = assets::file_name(sha256, content_type).map_err(|_| InvalidArtworkReference)?;
    Ok(format!("{}/{name}", ARTWORK_CLASS.name))
}

/// The file a stored reference names. Accepts only the exact form [`artwork_reference`]
/// produces, so no other spelling, directory, or extension can reach the filesystem.
pub fn parse_artwork_reference(reference: &str) -> Result<ArtworkFile, InvalidArtworkReference> {
    let name = reference
        .strip_prefix(ARTWORK_CLASS.name)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or(InvalidArtworkReference)?;
    let (sha256, extension) = name.split_once('.').ok_or(InvalidArtworkReference)?;
    let content_type = ARTWORK_CONTENT_TYPES
        .iter()
        .find(|content_type| assets::extension(content_type) == extension)
        .ok_or(InvalidArtworkReference)?;
    let file = ArtworkFile {
        sha256: sha256.to_string(),
        content_type: (*content_type).to_string(),
    };
    if file.reference()? != reference {
        return Err(InvalidArtworkReference);
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn artwork_references_round_trip_every_content_type() {
        for content_type in ARTWORK_CONTENT_TYPES {
            let reference = artwork_reference(DIGEST, content_type).unwrap();
            let file = parse_artwork_reference(&reference).unwrap();
            assert_eq!(file.sha256, DIGEST);
            assert_eq!(file.content_type, content_type);
        }
        assert_eq!(
            artwork_reference(DIGEST, "image/jpeg").unwrap(),
            format!("music/artwork/{DIGEST}.jpg")
        );
    }

    #[test]
    fn artwork_references_name_the_engine_asset_path() {
        let reference = format!("music/artwork/{DIGEST}.webp");
        let asset = parse_artwork_reference(&reference)
            .unwrap()
            .asset(1_024)
            .unwrap();
        assert_eq!(
            asset.path(Path::new("vault")),
            Path::new("vault")
                .join("assets")
                .join("music")
                .join("artwork")
                .join(format!("{DIGEST}.webp"))
        );
    }

    #[test]
    fn artwork_references_refuse_paths_and_other_spellings() {
        for value in [
            String::new(),
            "/custom/artwork.png".to_string(),
            "C:\\Music\\cover.jpg".to_string(),
            "content://media/external/images/1".to_string(),
            format!("music/artwork/{DIGEST}"),
            format!("music/artwork/{DIGEST}.jpeg"),
            format!("music/artwork/{DIGEST}.PNG"),
            format!("music/artwork/{DIGEST}.bmp"),
            format!("music/artwork/{DIGEST}.bin"),
            format!("music/artwork/{}.png", DIGEST.to_uppercase()),
            format!("music/artwork/{}.png", &DIGEST[1..]),
            format!("music/artwork/../{DIGEST}.png"),
            format!("music/artwork/sub/{DIGEST}.png"),
            format!("quick-notes/images/{DIGEST}.png"),
            format!("music/artwork{DIGEST}.png"),
            format!(" music/artwork/{DIGEST}.png"),
            format!("music/artwork/{DIGEST}.png.png"),
        ] {
            assert_eq!(
                parse_artwork_reference(&value),
                Err(InvalidArtworkReference),
                "{value}"
            );
        }
        assert_eq!(
            artwork_reference(DIGEST, "image/bmp"),
            Err(InvalidArtworkReference)
        );
    }

    #[test]
    fn artwork_assets_respect_the_class_bound() {
        let file = parse_artwork_reference(&format!("music/artwork/{DIGEST}.gif")).unwrap();
        assert!(file.asset(MAX_ARTWORK_BYTES).is_ok());
        assert_eq!(file.asset(0), Err(InvalidArtworkReference));
        assert_eq!(
            file.asset(MAX_ARTWORK_BYTES + 1),
            Err(InvalidArtworkReference)
        );
    }
}
