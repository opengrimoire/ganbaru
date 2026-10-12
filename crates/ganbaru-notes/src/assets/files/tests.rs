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
        classify_selected_file(
            NotesFileAssetKind::Audio,
            Path::new("focus.m4a"),
            b"not sniffed",
        )
        .unwrap(),
        (NotesFileAssetKind::Audio, "audio/mp4", "m4a".to_string()),
    );
    assert_eq!(
        classify_selected_file(
            NotesFileAssetKind::Video,
            Path::new("clip.mp4"),
            b"not sniffed",
        )
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
fn keeps_only_supported_https_external_import_references() {
    let image = prepare_external_import_reference("https://example.com/image.png", Some("image"));
    assert_eq!(image.action, "external_reference");
    assert_eq!(
        image.external_url,
        Some("https://example.com/image.png".to_string())
    );

    let youtube =
        prepare_external_import_reference("https://www.youtube.com/watch?v=abc123", Some("video"));
    assert_eq!(youtube.action, "external_reference");

    let http = prepare_external_import_reference("http://example.com/image.png", Some("image"));
    assert_eq!(http.action, "blocked");
    assert_eq!(
        http.diagnostics[0].code,
        "import_reference_external_requires_https"
    );

    let mismatched =
        prepare_external_import_reference("https://example.com/image.txt", Some("image"));
    assert_eq!(mismatched.action, "blocked");
    assert_eq!(
        mismatched.diagnostics[0].code,
        "import_reference_external_unsupported_type"
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

#[test]
fn import_file_policy_requires_supported_contexts_and_explicit_choices() {
    assert!(import_context_is_supported("notion_export"));
    assert!(import_context_is_supported("html_import"));
    assert!(import_context_is_supported("markdown_import"));
    assert!(!import_context_is_supported("unknown"));

    assert!(import_choice_is_supported("copy_local_file"));
    assert!(import_choice_is_supported("keep_external_reference"));
    assert!(import_choice_is_supported("skip"));
    assert!(!import_choice_is_supported(""));

    assert!(is_url_like_import_reference("https://example.com/file.pdf"));
    assert!(is_url_like_import_reference(
        "ganbaru-asset:notes/files/a.pdf"
    ));
    assert!(!is_url_like_import_reference("files/a.pdf"));
}
