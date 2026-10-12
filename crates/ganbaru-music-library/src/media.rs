//! Local media file recognition shared by refresh, relink, repair, and folder pickers.

use std::path::Path;

/// File extensions the desktop player accepts as local audio or video media.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "aac", "aif", "aiff", "alac", "ape", "avi", "flac", "flv", "m4a", "m4v", "mkv", "mov", "mp3",
    "mp4", "mpeg", "mpg", "ogg", "ogv", "opus", "wav", "webm", "wma", "wmv",
];

/// Returns whether a path names a supported media file by its extension, ignoring case.
pub fn is_supported_media_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

#[cfg(test)]
mod tests {
    use super::is_supported_media_path;
    use std::path::Path;

    #[test]
    fn media_path_support_accepts_audio_and_video_extensions() {
        assert!(is_supported_media_path(Path::new("/music/focus.flac")));
        assert!(is_supported_media_path(Path::new("/video/reference.mkv")));
        assert!(!is_supported_media_path(Path::new("/notes/readme.txt")));
    }
}
