//! Artwork overrides as managed vault assets of the Music artwork class.
//!
//! A picked image is copied into the class directory under its engine file name and the item
//! stores the reference `music/artwork/{sha256}.{ext}` with the file's exact size. Readers
//! resolve a reference only through the engine path of its class, refuse a final symlink or a
//! symlinked directory, and check that the bytes still match the digest and type the reference
//! names.

use std::io;
use std::path::{Path, PathBuf};

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, is_regular_file, read_regular_file_capped,
    require_directories_within, write_file_atomically_within,
};
use ganbaru_image_metadata::{parse_display_image_metadata, validate_managed_image_dimensions};
use ganbaru_sync::assets::AssetRef;
pub use ganbaru_sync::domains::music::{
    ARTWORK_CONTENT_TYPES, ArtworkFile, InvalidArtworkReference, MAX_ARTWORK_BYTES,
    parse_artwork_reference,
};
pub use ganbaru_sync::manifest::vault::music::{ARTWORK_CLASS, DOMAIN_NAME};
use sha2::{Digest, Sha256};

use crate::{MusicLibraryError, MusicLibraryResult};

/// File extensions an artwork picker offers: the extensions of [`ARTWORK_CONTENT_TYPES`] and
/// the common `jpeg` spelling.
pub const ARTWORK_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

const FIELD: &str = "artworkOverride";

/// A managed artwork file: the reference an item stores and the asset it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedArtwork {
    /// The stored reference, `music/artwork/{sha256}.{ext}`.
    pub reference: String,
    /// The asset with its exact size.
    pub asset: AssetRef,
}

/// Verified bytes of a managed artwork file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedArtworkBytes {
    /// Media type of the image, one of [`ARTWORK_CONTENT_TYPES`].
    pub content_type: String,
    /// File bytes, at most [`MAX_ARTWORK_BYTES`].
    pub bytes: Vec<u8>,
}

fn unsupported() -> MusicLibraryError {
    MusicLibraryError::validation(FIELD, "must be a PNG, JPEG, WebP, or GIF image")
}

fn too_large() -> MusicLibraryError {
    MusicLibraryError::validation(FIELD, format!("must be at most {MAX_ARTWORK_BYTES} bytes"))
}

fn invalid_reference(error: InvalidArtworkReference) -> MusicLibraryError {
    MusicLibraryError::validation(FIELD, error.to_string())
}

/// Describes image bytes as managed artwork after checking their size, type, and dimensions.
pub fn describe_artwork(bytes: &[u8]) -> MusicLibraryResult<ManagedArtwork> {
    if bytes.is_empty() {
        return Err(unsupported());
    }
    let byte_size = u32::try_from(bytes.len())
        .ok()
        .filter(|size| *size <= MAX_ARTWORK_BYTES)
        .ok_or_else(too_large)?;
    let metadata = parse_display_image_metadata(bytes).map_err(|_| unsupported())?;
    validate_managed_image_dimensions(metadata)
        .map_err(|error| MusicLibraryError::validation(FIELD, error.to_string()))?;
    let file = ArtworkFile {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        content_type: metadata.kind.mime_type().to_string(),
    };
    Ok(ManagedArtwork {
        reference: file.reference().map_err(invalid_reference)?,
        asset: file.asset(byte_size).map_err(invalid_reference)?,
    })
}

/// Copies image bytes into the artwork class directory of a vault and returns the managed
/// artwork. An identical file already in place is kept.
pub fn import_artwork(vault_root: &Path, bytes: &[u8]) -> MusicLibraryResult<ManagedArtwork> {
    let artwork = describe_artwork(bytes)?;
    let path = artwork.asset.path(vault_root);
    if !holds_bytes(vault_root, &path, bytes)? {
        write_file_atomically_within(vault_root, &path, bytes, FileAccess::Default)
            .map_err(|error| MusicLibraryError::runtime("store music artwork", error))?;
    }
    Ok(artwork)
}

/// Whether the content-addressed file already holds exactly `bytes`.
fn holds_bytes(vault_root: &Path, path: &Path, bytes: &[u8]) -> MusicLibraryResult<bool> {
    if !is_regular_file(path) {
        return Ok(false);
    }
    match read_contained(vault_root, path) {
        Ok(current) => Ok(current == bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        // A damaged or oversized file under the right name is replaced like a missing one.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::FileTooLarge | io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(MusicLibraryError::runtime("inspect music artwork", error)),
    }
}

/// Reads a file of the artwork class without following a symlinked directory or final link.
fn read_contained(vault_root: &Path, path: &Path) -> io::Result<Vec<u8>> {
    if let Some(directory) = path.parent() {
        require_directories_within(vault_root, directory)?;
    }
    read_regular_file_capped(path, u64::from(MAX_ARTWORK_BYTES), FinalSymlink::Refuse)
}

/// The vault path a stored reference names, after checking its form.
pub fn artwork_path(vault_root: &Path, reference: &str) -> MusicLibraryResult<PathBuf> {
    parse_artwork_reference(reference)
        .and_then(|file| file.path(vault_root))
        .map_err(invalid_reference)
}

/// Reads the managed artwork a stored reference names and checks that the bytes are a
/// supported image of the digest and type the reference names.
pub fn read_artwork(vault_root: &Path, reference: &str) -> MusicLibraryResult<ManagedArtworkBytes> {
    let file = parse_artwork_reference(reference).map_err(invalid_reference)?;
    let path = file.path(vault_root).map_err(invalid_reference)?;
    let bytes = read_contained(vault_root, &path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => MusicLibraryError::not_found("music artwork", reference),
        _ => MusicLibraryError::runtime("read music artwork", error),
    })?;
    let described = describe_artwork(&bytes)?;
    if described.reference != reference {
        return Err(MusicLibraryError::conflict(format!(
            "music artwork '{reference}' does not match its file"
        )));
    }
    Ok(ManagedArtworkBytes {
        content_type: file.content_type,
        bytes,
    })
}

/// The managed artwork a stored reference names, with the exact size of its verified file.
pub fn stored_artwork(vault_root: &Path, reference: &str) -> MusicLibraryResult<ManagedArtwork> {
    let artwork = read_artwork(vault_root, reference)?;
    describe_artwork(&artwork.bytes)
}

/// Checks Music artwork that sync received, given the file that holds its bytes after their
/// size and digest matched the reference: the bytes must be a supported image within the
/// limits, of the content type the reference names. Sync registers it as the check of
/// [`ARTWORK_CLASS`].
pub fn check_synced_artwork(asset: &AssetRef, path: &Path) -> Result<(), String> {
    let bytes = read_regular_file_capped(path, u64::from(MAX_ARTWORK_BYTES), FinalSymlink::Refuse)
        .map_err(|error| format!("read synced music artwork: {error}"))?;
    let described = describe_artwork(&bytes)
        .map_err(|error| format!("the bytes are not supported artwork: {error}"))?;
    if described.asset.content_type() != asset.content_type() {
        return Err(format!(
            "the bytes are {} but the reference names {}",
            described.asset.content_type(),
            asset.content_type()
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::MusicLibraryErrorCode;

    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 33];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[8..12].copy_from_slice(&13_u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes[24] = 8;
        bytes[25] = 6;
        bytes
    }

    fn gif(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0]);
        bytes
    }

    fn vault(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ganbaru-music-artwork-{name}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn imported_artwork_is_stored_under_the_engine_path_of_its_class() {
        let root = vault("import");
        let bytes = png(640, 480);
        let artwork = import_artwork(&root, &bytes).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));

        assert_eq!(artwork.reference, format!("music/artwork/{digest}.png"));
        assert_eq!(artwork.asset.byte_size() as usize, bytes.len());
        assert_eq!(artwork.asset.class(), &ARTWORK_CLASS);
        let path = root
            .join("assets")
            .join("music")
            .join("artwork")
            .join(format!("{digest}.png"));
        assert_eq!(artwork.asset.path(&root), path);
        assert_eq!(artwork_path(&root, &artwork.reference).unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), bytes);

        let again = import_artwork(&root, &bytes).unwrap();
        assert_eq!(again, artwork);
        let read = read_artwork(&root, &artwork.reference).unwrap();
        assert_eq!(read.content_type, "image/png");
        assert_eq!(read.bytes, bytes);
        assert_eq!(stored_artwork(&root, &artwork.reference).unwrap(), artwork);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_import_accepts_gif_through_the_display_parser() {
        let root = vault("gif");
        let artwork = import_artwork(&root, &gif(32, 32)).unwrap();
        assert!(artwork.reference.ends_with(".gif"));
        assert_eq!(artwork.asset.content_type(), "image/gif");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_import_refuses_unsupported_oversized_and_huge_images() {
        let root = vault("refuse");
        let mut bmp = b"BM".to_vec();
        bmp.resize(64, 0);
        for bytes in [Vec::new(), bmp, b"not an image".to_vec()] {
            let error = import_artwork(&root, &bytes).unwrap_err();
            assert_eq!(error.code, MusicLibraryErrorCode::Validation);
        }
        let mut oversized = png(16, 16);
        oversized.resize(MAX_ARTWORK_BYTES as usize + 1, 0);
        assert_eq!(import_artwork(&root, &oversized).unwrap_err(), too_large());
        let huge = import_artwork(&root, &png(9_000, 10)).unwrap_err();
        assert_eq!(huge.code, MusicLibraryErrorCode::Validation);
        assert!(!ARTWORK_CLASS.directory(&root).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_import_replaces_a_damaged_file_under_the_same_name() {
        let root = vault("damaged");
        let bytes = png(10, 10);
        let artwork = import_artwork(&root, &bytes).unwrap();
        let path = artwork.asset.path(&root);
        fs::write(&path, b"damaged").unwrap();
        assert_eq!(
            read_artwork(&root, &artwork.reference).unwrap_err().code,
            MusicLibraryErrorCode::Validation
        );
        import_artwork(&root, &bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn artwork_reads_refuse_paths_missing_files_and_mismatched_bytes() {
        let root = vault("read");
        for reference in ["/custom/artwork.png", "music/artwork/../vault.json", ""] {
            assert_eq!(
                read_artwork(&root, reference).unwrap_err().code,
                MusicLibraryErrorCode::Validation
            );
        }
        let missing = format!("music/artwork/{}.png", "a".repeat(64));
        assert_eq!(
            read_artwork(&root, &missing).unwrap_err().code,
            MusicLibraryErrorCode::NotFound
        );
        let artwork = import_artwork(&root, &png(12, 12)).unwrap();
        fs::write(artwork.asset.path(&root), png(13, 13)).unwrap();
        assert_eq!(
            read_artwork(&root, &artwork.reference).unwrap_err().code,
            MusicLibraryErrorCode::Conflict
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn artwork_reads_refuse_symlinked_files_and_directories() {
        let root = vault("symlink");
        let outside = vault("symlink-outside");
        let bytes = png(20, 20);
        let artwork = describe_artwork(&bytes).unwrap();
        let target = outside.join(artwork.asset.file_name());
        fs::write(&target, &bytes).unwrap();
        let directory = ARTWORK_CLASS.directory(&root);
        fs::create_dir_all(&directory).unwrap();
        std::os::unix::fs::symlink(&target, artwork.asset.path(&root)).unwrap();
        assert!(read_artwork(&root, &artwork.reference).is_err());

        fs::remove_dir_all(root.join("assets").join("music")).unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("assets").join("music")).unwrap();
        assert!(read_artwork(&root, &artwork.reference).is_err());
        assert!(import_artwork(&root, &bytes).is_err());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn synced_artwork_checks_the_type_the_reference_names() {
        let root = vault("synced");
        let artwork = import_artwork(&root, &png(8, 8)).unwrap();
        let path = artwork.asset.path(&root);
        assert_eq!(check_synced_artwork(&artwork.asset, &path), Ok(()));
        let as_gif = ArtworkFile {
            sha256: artwork.asset.sha256().to_string(),
            content_type: "image/gif".to_string(),
        }
        .asset(artwork.asset.byte_size())
        .unwrap();
        assert!(check_synced_artwork(&as_gif, &path).is_err());
        fs::write(&path, b"plain text").unwrap();
        assert!(check_synced_artwork(&artwork.asset, &path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
