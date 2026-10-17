//! Media inspection and native playback.
//!
//! Desktop decodes local files with the native engine in `ganbaru-music-player`. Android
//! delegates inspection to the platform media plugin, and playback belongs to the native Music
//! session there.

#[cfg(desktop)]
pub(crate) use ganbaru_music_player::{
    MediaPlayerError, MediaPlayerState, PlayerStatus, apply_session_effect, session_snapshot,
};

/// Inspect a local media file without mutating decoder or session state.
#[cfg(desktop)]
#[tauri::command]
pub(crate) fn media_player_probe(
    path: String,
) -> Result<ganbaru_music_player::MediaProbe, MediaPlayerError> {
    ganbaru_music_player::probe_local_file(&path)
}

/// Inspect a selected document without mutating decoder or session state.
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn media_player_probe(
    app: tauri::AppHandle,
    path: String,
) -> Result<ganbaru_mobile_media::MobileMediaProbe, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    app.mobile_media().probe(&path).await
}
