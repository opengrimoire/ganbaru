use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn classifies_supported_local_media_files() {
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Image, Path::new("image.png"), &png).unwrap(),
        (NotesFileAssetKind::Image, "image/png", "png".to_string(),),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Pdf,
            Path::new("report.pdf"),
            b"%PDF-1.7"
        )
        .unwrap(),
        (
            NotesFileAssetKind::Pdf,
            "application/pdf",
            "pdf".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("focus.m4a"), ISO_MEDIA,)
            .unwrap(),
        (NotesFileAssetKind::Audio, "audio/mp4", "m4a".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Video, Path::new("clip.mp4"), ISO_MEDIA,)
            .unwrap(),
        (NotesFileAssetKind::Video, "video/mp4", "mp4".to_string()),
    );
}

#[test]
fn rejects_unsafe_or_mismatched_local_media_files() {
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Image,
            Path::new("image.svg"),
            b"<svg />"
        )
        .unwrap_err(),
        "local image blocks support PNG, JPG, and WebP files",
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Pdf,
            Path::new("report.txt"),
            b"%PDF-1.7"
        )
        .unwrap_err(),
        "local PDF blocks require a PDF file",
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("focus.exe"), b"").unwrap_err(),
        "local audio blocks support MP3, WAV, OGG, OGA, and M4A files",
    );
    assert_eq!(
        validate_notes_file_relative_path("notes/files/nested/file.pdf").unwrap_err(),
        "notes file path cannot contain nested or parent paths",
    );
}

#[test]
fn resolves_local_import_paths_only_inside_import_root() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ganbaru-notes-import-test-{nonce}"));
    let root = base.join("export");
    let file = root.join("asset.pdf");
    let outside = base.join("secret.pdf");
    fs::create_dir_all(&root).unwrap();
    fs::write(&file, b"%PDF-1.7").unwrap();
    fs::write(&outside, b"%PDF-1.7").unwrap();

    let resolved = resolve_import_candidate_path(root.to_str().unwrap(), "asset.pdf").unwrap();
    assert_eq!(resolved, fs::canonicalize(&file).unwrap());

    let escaped =
        resolve_import_candidate_path(root.to_str().unwrap(), "../secret.pdf").unwrap_err();
    assert_eq!(escaped.code, "import_reference_path_escape");

    let _ = fs::remove_dir_all(&base);
}
const ID3_MP3: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x00";
const ISO_MEDIA: &[u8] = b"\x00\x00\x00\x18ftypM4A \x00\x00\x00\x00";

fn temporary_import_base(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!("ganbaru-notes-{label}-{nonce}"));
    fs::create_dir_all(base.join("export")).unwrap();
    fs::create_dir_all(base.join("vault")).unwrap();
    base
}

#[test]
fn media_blocks_take_their_content_type_from_the_container_signature() {
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("focus.mp3"), ID3_MP3).unwrap(),
        (NotesFileAssetKind::Audio, "audio/mpeg", "mp3".to_string()),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Audio,
            Path::new("focus.mp3"),
            &[0, 0, 0xff, 0xfb, 0x90, 0x00],
        )
        .unwrap(),
        (NotesFileAssetKind::Audio, "audio/mpeg", "mp3".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("focus.ogg"), ID3_MP3).unwrap(),
        (NotesFileAssetKind::Audio, "audio/mpeg", "ogg".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("voice.m4a"), ISO_MEDIA)
            .unwrap(),
        (NotesFileAssetKind::Audio, "audio/mp4", "m4a".to_string()),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Audio,
            Path::new("tone.wav"),
            b"RIFF\x24\x00\x00\x00WAVEfmt ",
        )
        .unwrap(),
        (NotesFileAssetKind::Audio, "audio/wav", "wav".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Video, Path::new("clip.mov"), ISO_MEDIA)
            .unwrap(),
        (
            NotesFileAssetKind::Video,
            "video/quicktime",
            "mov".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Video, Path::new("clip.mp4"), ISO_MEDIA)
            .unwrap(),
        (NotesFileAssetKind::Video, "video/mp4", "mp4".to_string()),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("old.mov"),
            b"\x00\x00\x00\x08wide\x00\x00\x00\x00mdat",
        )
        .unwrap(),
        (
            NotesFileAssetKind::Video,
            "video/quicktime",
            "mov".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("talk.mkv"),
            &[0x1a, 0x45, 0xdf, 0xa3, 0x01],
        )
        .unwrap(),
        (
            NotesFileAssetKind::Video,
            "video/x-matroska",
            "mkv".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("talk.wmv"),
            &ASF_HEADER_GUID
        )
        .unwrap(),
        (
            NotesFileAssetKind::Video,
            "video/x-ms-wmv",
            "wmv".to_string()
        ),
    );
    let mut transport = vec![0_u8; MPEG_TRANSPORT_PACKET_BYTES * 2];
    transport[0] = 0x47;
    transport[MPEG_TRANSPORT_PACKET_BYTES] = 0x47;
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("broadcast.mpg"),
            &transport
        )
        .unwrap(),
        (NotesFileAssetKind::Video, "video/mp2t", "mpg".to_string()),
    );
}

#[test]
fn media_blocks_reject_files_whose_bytes_are_not_that_media() {
    let html = b"<!doctype html><script>alert(1)</script>";
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("song.mp3"), html).unwrap_err(),
        "local audio blocks require an audio file",
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Video, Path::new("clip.mp4"), html).unwrap_err(),
        "local video blocks require a supported video file",
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::Audio, Path::new("song.mp3"), &[0; 8192])
            .unwrap_err(),
        "local audio blocks require an audio file",
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("clip.mp4"),
            b"RIFF\x24\x00\x00\x00WAVEfmt ",
        )
        .unwrap_err(),
        "local video blocks require a supported video file",
    );
}

#[test]
fn file_blocks_promote_only_when_the_signature_matches_the_extension() {
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("photo.png"), b"<svg />")
            .unwrap(),
        (
            NotesFileAssetKind::File,
            "application/octet-stream",
            "png".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("song.mp3"), b"<html>").unwrap(),
        (
            NotesFileAssetKind::File,
            "application/octet-stream",
            "mp3".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("song.mp3"), ID3_MP3).unwrap(),
        (NotesFileAssetKind::Audio, "audio/mpeg", "mp3".to_string()),
    );
    let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("photo.jpg"), &png).unwrap(),
        (NotesFileAssetKind::Image, "image/png", "jpg".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("notes.txt"), &png).unwrap(),
        (NotesFileAssetKind::File, "text/plain", "txt".to_string()),
    );
    assert_eq!(
        classify_selected_file(NotesFileAssetKind::File, Path::new("logo.svg"), b"<svg />")
            .unwrap(),
        (
            NotesFileAssetKind::File,
            "application/octet-stream",
            "svg".to_string()
        ),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::File,
            Path::new("report.pdf"),
            b"%PDF-1.7"
        )
        .unwrap(),
        (
            NotesFileAssetKind::Pdf,
            "application/pdf",
            "pdf".to_string()
        ),
    );
}

#[test]
fn import_copies_refuse_oversized_files_through_the_open_handle() {
    let base = temporary_import_base("import-size");
    let vault = base.join("vault");
    let root = base.join("export");
    let path = root.join("big.bin");
    let file = fs::File::create(&path).unwrap();
    file.set_len(NOTES_FILE_MAX_BYTES as u64 + 1).unwrap();
    let blocked = copy_import_file(
        &vault,
        root.to_str().unwrap(),
        "big.bin",
        NotesFileAssetKind::File,
    )
    .unwrap_err();
    assert_eq!(blocked.diagnostics[0].code, "import_reference_too_large");
    file.set_len(NOTES_FILE_MAX_BYTES as u64).unwrap();
    let (stored, _) = copy_import_file(
        &vault,
        root.to_str().unwrap(),
        "big.bin",
        NotesFileAssetKind::File,
    )
    .unwrap();
    assert_eq!(stored.byte_size, NOTES_FILE_MAX_BYTES as i64);
    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn import_symlinks_resolve_only_inside_the_import_root() {
    use std::os::unix::fs::symlink;
    let base = temporary_import_base("import-link");
    let vault = base.join("vault");
    let root = base.join("export");
    fs::write(root.join("song.mp3"), ID3_MP3).unwrap();
    fs::write(base.join("secret.pdf"), b"%PDF-1.7").unwrap();
    symlink(base.join("secret.pdf"), root.join("escape.pdf")).unwrap();
    symlink(root.join("song.mp3"), root.join("alias.mp3")).unwrap();
    let escaped = copy_import_file(
        &vault,
        root.to_str().unwrap(),
        "escape.pdf",
        NotesFileAssetKind::Pdf,
    )
    .unwrap_err();
    assert_eq!(escaped.diagnostics[0].code, "import_reference_path_escape");
    let (stored, _) = copy_import_file(
        &vault,
        root.to_str().unwrap(),
        "alias.mp3",
        NotesFileAssetKind::Audio,
    )
    .unwrap();
    assert_eq!(stored.content_type, "audio/mpeg");
    let _ = fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn managed_previews_refuse_a_symlink_planted_in_the_vault() {
    use std::os::unix::fs::symlink;
    let base = temporary_import_base("preview-link");
    fs::write(base.join("secret.txt"), b"secret").unwrap();
    let planted = base.join("planted.pdf");
    symlink(base.join("secret.txt"), &planted).unwrap();
    assert_eq!(
        read_preview_file_capped(&planted, 6).unwrap_err(),
        "notes file path must be a regular file",
    );
    fs::write(base.join("grown.pdf"), b"%PDF-1.7 grown").unwrap();
    assert_eq!(
        read_preview_file_capped(&base.join("grown.pdf"), 8).unwrap_err(),
        "notes file size no longer matches its recorded metadata",
    );
    let _ = fs::remove_dir_all(&base);
}
