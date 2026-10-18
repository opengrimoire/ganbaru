//! Android YouTube host in a separate WebView.
//!
//! The player page runs in its own native WebView with no JavaScript
//! interfaces and no document-start scripts, so the app WebView can deny every
//! frame source. These commands are the only way the app WebView reaches that
//! view: Rust owns the page, its origin, and its policy, and validates every
//! URL, placement, and message before Kotlin sees it.

use serde::Deserialize;

use super::super::media_server::constant_time_token_eq;

/// Origin the native host serves the player page from. Android reserves this
/// host for app-provided content, so it never reaches the network.
pub(crate) const NATIVE_YOUTUBE_HOST_ORIGIN: &str = "https://appassets.androidplatform.net";
/// Only path the native host serves.
pub(crate) const NATIVE_YOUTUBE_HOST_PATH: &str = "/youtube-player.html";
const NATIVE_YOUTUBE_HOST_DOMAIN: &str = "appassets.androidplatform.net";
const MAX_NATIVE_HOST_URL_BYTES: usize = 4 * 1024;
const MAX_NATIVE_HOST_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_NATIVE_HOST_COORDINATE: f64 = 100_000.0;
const MAX_NATIVE_HOST_EXTENT: f64 = 32_768.0;
const MAX_NATIVE_HOST_SCALE: f64 = 8.0;
const NATIVE_HOST_QUERY_KEYS: &[&str] = &[
    "token",
    "load",
    "sourceKind",
    "videoId",
    "playlistId",
    "startMs",
    "endMs",
    "resumeMs",
    "volume",
    "rate",
    "autoplay",
];
const NATIVE_HOST_COMMAND_TYPE: &str = "ganbaru-youtube-command";

/// The two native host views: the visible player and the hidden source resolver.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NativeYouTubeHostInstance {
    Playback,
    Resolver,
}

impl NativeYouTubeHostInstance {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Playback => "playback",
            Self::Resolver => "resolver",
        }
    }
}

/// Host placement in CSS pixels of the app WebView.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub(crate) struct NativeYouTubeHostBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub visible: bool,
}

/// Builds the tokenized player URL the frontend extends with load parameters.
pub(crate) fn native_youtube_host_url(token: &str) -> String {
    format!("{NATIVE_YOUTUBE_HOST_ORIGIN}{NATIVE_YOUTUBE_HOST_PATH}?token={token}")
}

/// Accepts only the player page on the native host origin with this
/// session's token and the known load parameters.
pub(crate) fn validate_native_host_url(candidate: &str, token: &str) -> Result<String, String> {
    if candidate.len() > MAX_NATIVE_HOST_URL_BYTES {
        return Err("YouTube host URL is too long".to_string());
    }
    let url = reqwest::Url::parse(candidate).map_err(|_| "Invalid YouTube host URL".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some(NATIVE_YOUTUBE_HOST_DOMAIN)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != NATIVE_YOUTUBE_HOST_PATH
        || url.fragment().is_some()
        || url.origin().ascii_serialization() != NATIVE_YOUTUBE_HOST_ORIGIN
    {
        return Err("YouTube host URL is outside the native host page".to_string());
    }
    let mut seen = Vec::with_capacity(NATIVE_HOST_QUERY_KEYS.len());
    let mut authorized = false;
    for (key, value) in url.query_pairs() {
        if !NATIVE_HOST_QUERY_KEYS.contains(&key.as_ref()) || seen.contains(&key) {
            return Err("YouTube host URL has unexpected parameters".to_string());
        }
        if key == "token" {
            authorized = constant_time_token_eq(token, &value);
        }
        seen.push(key);
    }
    if !authorized {
        return Err("YouTube host URL is not authorized".to_string());
    }
    Ok(url.into())
}

/// Rejects non-finite or implausible placements before they size a native view.
pub(crate) fn validate_native_host_bounds(
    bounds: NativeYouTubeHostBounds,
) -> Result<NativeYouTubeHostBounds, String> {
    let coordinates_valid = [bounds.x, bounds.y]
        .iter()
        .all(|value| value.is_finite() && value.abs() <= MAX_NATIVE_HOST_COORDINATE);
    let extents_valid = [bounds.width, bounds.height]
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0 && *value <= MAX_NATIVE_HOST_EXTENT);
    let scale_valid =
        bounds.scale.is_finite() && bounds.scale > 0.0 && bounds.scale <= MAX_NATIVE_HOST_SCALE;
    if coordinates_valid && extents_valid && scale_valid {
        Ok(bounds)
    } else {
        Err("YouTube host placement is invalid".to_string())
    }
}

/// Serializes a bounded player command. The page still checks its token.
pub(crate) fn validate_native_host_message(message: &serde_json::Value) -> Result<String, String> {
    let object = message
        .as_object()
        .ok_or_else(|| "YouTube host message must be an object".to_string())?;
    if object.get("type").and_then(serde_json::Value::as_str) != Some(NATIVE_HOST_COMMAND_TYPE) {
        return Err("YouTube host message has an unsupported type".to_string());
    }
    let serialized = serde_json::to_string(message)
        .map_err(|_| "YouTube host message is invalid".to_string())?;
    if serialized.len() > MAX_NATIVE_HOST_MESSAGE_BYTES {
        return Err("YouTube host message is too large".to_string());
    }
    Ok(serialized)
}

#[cfg(mobile)]
pub(crate) mod commands {
    use super::*;
    use crate::music::media_server::MusicHostState;
    use crate::music::youtube::player_page::{
        youtube_host_content_security_policy, youtube_host_html,
    };
    use ganbaru_mobile_media::{MobileMediaExt, MobileYouTubeBounds, MobileYouTubePage};
    use tauri::State;

    fn mobile_bounds(bounds: NativeYouTubeHostBounds) -> Result<MobileYouTubeBounds, String> {
        let bounds = validate_native_host_bounds(bounds)?;
        Ok(MobileYouTubeBounds {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            scale: bounds.scale,
            visible: bounds.visible,
        })
    }

    /// Opens or replaces a native YouTube host view on the player page.
    #[tauri::command]
    pub async fn music_youtube_native_open(
        app: tauri::AppHandle,
        state: State<'_, MusicHostState>,
        instance: NativeYouTubeHostInstance,
        url: String,
        bounds: NativeYouTubeHostBounds,
        channel: tauri::ipc::Channel,
    ) -> Result<(), String> {
        let url = validate_native_host_url(&url, state.token())?;
        let bounds = mobile_bounds(bounds)?;
        let page = MobileYouTubePage {
            url: &url,
            origin: NATIVE_YOUTUBE_HOST_ORIGIN,
            path: NATIVE_YOUTUBE_HOST_PATH,
            html: youtube_host_html(),
            content_security_policy: youtube_host_content_security_policy(),
        };
        app.mobile_media()
            .youtube_open(instance.as_str(), &page, bounds, channel)
            .await
    }

    /// Moves, resizes, shows, or hides a native YouTube host view.
    #[tauri::command]
    pub async fn music_youtube_native_bounds(
        app: tauri::AppHandle,
        instance: NativeYouTubeHostInstance,
        bounds: NativeYouTubeHostBounds,
    ) -> Result<(), String> {
        let bounds = mobile_bounds(bounds)?;
        app.mobile_media()
            .youtube_bounds(instance.as_str(), bounds)
            .await
    }

    /// Delivers a player command to a native YouTube host page.
    #[tauri::command]
    pub async fn music_youtube_native_post(
        app: tauri::AppHandle,
        instance: NativeYouTubeHostInstance,
        message: serde_json::Value,
    ) -> Result<(), String> {
        let message = validate_native_host_message(&message)?;
        app.mobile_media()
            .youtube_post(instance.as_str(), &message)
            .await
    }

    /// Destroys a native YouTube host view.
    #[tauri::command]
    pub async fn music_youtube_native_close(
        app: tauri::AppHandle,
        instance: NativeYouTubeHostInstance,
    ) -> Result<(), String> {
        app.mobile_media().youtube_close(instance.as_str()).await
    }
}

/// Desktop test builds compile the mobile handler list, so they register handlers that refuse every call.
#[cfg(all(test, desktop))]
pub(crate) mod commands {
    use super::*;

    const UNSUPPORTED: &str = "the native YouTube host is only available on Android";

    #[tauri::command]
    pub async fn music_youtube_native_open(
        _instance: NativeYouTubeHostInstance,
        _url: String,
        _bounds: NativeYouTubeHostBounds,
        _channel: tauri::ipc::Channel,
    ) -> Result<(), String> {
        Err(UNSUPPORTED.to_owned())
    }

    #[tauri::command]
    pub async fn music_youtube_native_bounds(
        _instance: NativeYouTubeHostInstance,
        _bounds: NativeYouTubeHostBounds,
    ) -> Result<(), String> {
        Err(UNSUPPORTED.to_owned())
    }

    #[tauri::command]
    pub async fn music_youtube_native_post(
        _instance: NativeYouTubeHostInstance,
        _message: serde_json::Value,
    ) -> Result<(), String> {
        Err(UNSUPPORTED.to_owned())
    }

    #[tauri::command]
    pub async fn music_youtube_native_close(
        _instance: NativeYouTubeHostInstance,
    ) -> Result<(), String> {
        Err(UNSUPPORTED.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn bounds() -> NativeYouTubeHostBounds {
        NativeYouTubeHostBounds {
            x: 12.0,
            y: 40.5,
            width: 320.0,
            height: 180.0,
            scale: 2.75,
            visible: true,
        }
    }

    #[test]
    fn native_host_url_accepts_the_tokenized_player_page_with_load_parameters() {
        let base = native_youtube_host_url(TOKEN);
        assert_eq!(validate_native_host_url(&base, TOKEN).unwrap(), base);
        let loaded = format!(
            "{base}&load=7&sourceKind=youtube-playlist&videoId=abc&playlistId=PL1&startMs=0&endMs=9&resumeMs=1&volume=0.8&rate=1&autoplay=true"
        );
        assert!(validate_native_host_url(&loaded, TOKEN).is_ok());
    }

    #[test]
    fn native_host_url_rejects_other_pages_origins_and_tokens() {
        let rejected = [
            format!("http://appassets.androidplatform.net/youtube-player.html?token={TOKEN}"),
            format!("https://appassets.androidplatform.net:444/youtube-player.html?token={TOKEN}"),
            format!(
                "https://appassets.androidplatform.net.evil.test/youtube-player.html?token={TOKEN}"
            ),
            format!("https://user@appassets.androidplatform.net/youtube-player.html?token={TOKEN}"),
            format!("https://appassets.androidplatform.net/other.html?token={TOKEN}"),
            format!("https://appassets.androidplatform.net/youtube-player.html?token={TOKEN}#x"),
            format!(
                "https://appassets.androidplatform.net/youtube-player.html?token={TOKEN}&token={TOKEN}"
            ),
            format!(
                "https://appassets.androidplatform.net/youtube-player.html?token={TOKEN}&next=https://evil.test"
            ),
            "https://appassets.androidplatform.net/youtube-player.html?token=wrong".to_string(),
            "https://appassets.androidplatform.net/youtube-player.html".to_string(),
            format!("https://www.youtube.com/youtube-player.html?token={TOKEN}"),
            format!(
                "{}&videoId={}",
                native_youtube_host_url(TOKEN),
                "a".repeat(MAX_NATIVE_HOST_URL_BYTES)
            ),
        ];
        for candidate in rejected {
            assert!(
                validate_native_host_url(&candidate, TOKEN).is_err(),
                "{candidate}"
            );
        }
    }

    #[test]
    fn native_host_bounds_reject_non_finite_and_oversized_values() {
        assert_eq!(validate_native_host_bounds(bounds()).unwrap(), bounds());
        let invalid = [
            NativeYouTubeHostBounds {
                x: f64::NAN,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                y: f64::INFINITY,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                x: MAX_NATIVE_HOST_COORDINATE + 1.0,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                width: -1.0,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                height: MAX_NATIVE_HOST_EXTENT + 1.0,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                scale: 0.0,
                ..bounds()
            },
            NativeYouTubeHostBounds {
                scale: MAX_NATIVE_HOST_SCALE + 0.1,
                ..bounds()
            },
        ];
        for candidate in invalid {
            assert!(
                validate_native_host_bounds(candidate).is_err(),
                "{candidate:?}"
            );
        }
    }

    #[test]
    fn native_host_messages_must_be_bounded_player_commands() {
        let command = serde_json::json!({
            "type": NATIVE_HOST_COMMAND_TYPE,
            "token": TOKEN,
            "action": "play",
            "volume": 0.5,
        });
        let serialized = validate_native_host_message(&command).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap(),
            command
        );
        let rejected = [
            serde_json::json!("play"),
            serde_json::json!([NATIVE_HOST_COMMAND_TYPE]),
            serde_json::json!({ "type": "ganbaru-youtube-state" }),
            serde_json::json!({ "action": "play" }),
            serde_json::json!({
                "type": NATIVE_HOST_COMMAND_TYPE,
                "padding": "x".repeat(MAX_NATIVE_HOST_MESSAGE_BYTES),
            }),
        ];
        for candidate in rejected {
            assert!(validate_native_host_message(&candidate).is_err());
        }
    }

    #[test]
    fn native_host_instances_deserialize_only_known_names() {
        assert_eq!(
            serde_json::from_str::<NativeYouTubeHostInstance>("\"playback\"").unwrap(),
            NativeYouTubeHostInstance::Playback
        );
        assert_eq!(NativeYouTubeHostInstance::Resolver.as_str(), "resolver");
        assert!(serde_json::from_str::<NativeYouTubeHostInstance>("\"main\"").is_err());
    }

    #[test]
    fn android_app_webview_denies_every_frame_source() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../../../tauri.android.conf.json")).unwrap();
        for key in ["csp", "devCsp"] {
            assert_eq!(
                config["app"]["security"][key]["frame-src"], "'none'",
                "{key} must not allow frames in the app WebView"
            );
        }
    }
}
