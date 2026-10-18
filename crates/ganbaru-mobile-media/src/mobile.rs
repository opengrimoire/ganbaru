use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaru.mobile.media";

#[derive(Debug)]
pub struct MobileMedia<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for MobileMedia<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileMediaProbe {
    pub path: String,
    pub title: String,
    pub file_size_bytes: u64,
    pub extension: Option<String>,
    pub media_kind: String,
    pub playable_start_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileMediaTreeTrack {
    pub uri: String,
    pub relative_path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_number: Option<i64>,
    pub artwork_uri: Option<String>,
    pub embedded_artwork_candidate: bool,
    pub duration_ms: Option<i64>,
    pub file_size_bytes: Option<i64>,
    pub modified_at_ms: Option<i64>,
    pub media_kind: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileMediaTree {
    pub tree_uri: String,
    pub display_name: String,
    pub tracks: Vec<MobileMediaTreeTrack>,
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PathRequest<'a> {
    path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TreeScanRequest<'a> {
    tree_uri: &'a str,
    max_files: u32,
    max_depth: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TreePickRequest {
    max_files: u32,
    max_depth: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PickArtworkFileResponse {
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtworkDataUrlResponse {
    data_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtworkRequest<'a> {
    path: &'a str,
    embedded: bool,
    max_bytes: u64,
}

#[derive(Deserialize)]
struct OptionalTreeResponse {
    tree: Option<MobileMediaTree>,
}

pub(crate) fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> tauri::Result<MobileMedia<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MobileMediaPlugin")?;
    Ok(MobileMedia(handle))
}

impl<R: Runtime> MobileMedia<R> {
    /// Attaches the process-owned native callback to the Media3 service once.
    pub async fn attach_session(&self, channel: tauri::ipc::Channel) -> Result<(), String> {
        #[derive(Serialize)]
        struct AttachRequest {
            channel: tauri::ipc::Channel,
        }
        self.command("attachSession", AttachRequest { channel })
            .await
    }

    async fn command<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        payload: impl Serialize,
    ) -> Result<T, String> {
        self.0
            .run_mobile_plugin_async(name, payload)
            .await
            .map_err(|error| format!("Android media {name}: {error}"))
    }

    pub async fn probe(&self, path: &str) -> Result<MobileMediaProbe, String> {
        self.command("probe", PathRequest { path }).await
    }

    pub async fn pick_media_tree(
        &self,
        max_files: u32,
        max_depth: u32,
    ) -> Result<Option<MobileMediaTree>, String> {
        self.command::<OptionalTreeResponse>(
            "pickMediaTree",
            TreePickRequest {
                max_files,
                max_depth,
            },
        )
        .await
        .map(|response| response.tree)
    }

    pub async fn scan_media_tree(
        &self,
        tree_uri: &str,
        max_files: u32,
        max_depth: u32,
    ) -> Result<MobileMediaTree, String> {
        self.command(
            "scanMediaTree",
            TreeScanRequest {
                tree_uri,
                max_files,
                max_depth,
            },
        )
        .await
    }

    pub async fn pick_artwork_file(&self) -> Result<Option<String>, String> {
        self.command::<PickArtworkFileResponse>("pickArtworkFile", ())
            .await
            .map(|response| response.uri)
    }

    pub async fn artwork_data_url(
        &self,
        path: &str,
        embedded: bool,
        max_bytes: u64,
    ) -> Result<Option<String>, String> {
        self.command::<ArtworkDataUrlResponse>(
            "artworkDataUrl",
            ArtworkRequest {
                path,
                embedded,
                max_bytes,
            },
        )
        .await
        .map(|response| response.data_url)
    }

    /// Opens a separate, bridgeless WebView that serves `page` and forwards its
    /// host-origin messages to `channel`. Callers validate every field first.
    pub async fn youtube_open(
        &self,
        instance: &str,
        page: &MobileYouTubePage<'_>,
        bounds: MobileYouTubeBounds,
        channel: tauri::ipc::Channel,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct OpenRequest<'a> {
            instance: &'a str,
            #[serde(flatten)]
            page: &'a MobileYouTubePage<'a>,
            #[serde(flatten)]
            bounds: MobileYouTubeBounds,
            channel: tauri::ipc::Channel,
        }
        self.command(
            "youtubeOpen",
            OpenRequest {
                instance,
                page,
                bounds,
                channel,
            },
        )
        .await
    }

    /// Moves, resizes, shows, or hides a YouTube host WebView.
    pub async fn youtube_bounds(
        &self,
        instance: &str,
        bounds: MobileYouTubeBounds,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        struct BoundsRequest<'a> {
            instance: &'a str,
            #[serde(flatten)]
            bounds: MobileYouTubeBounds,
        }
        self.command("youtubeBounds", BoundsRequest { instance, bounds })
            .await
    }

    /// Posts a serialized command to the YouTube host page.
    pub async fn youtube_post(&self, instance: &str, message: &str) -> Result<(), String> {
        #[derive(Serialize)]
        struct PostRequest<'a> {
            instance: &'a str,
            message: &'a str,
        }
        self.command("youtubePost", PostRequest { instance, message })
            .await
    }

    /// Destroys a YouTube host WebView.
    pub async fn youtube_close(&self, instance: &str) -> Result<(), String> {
        #[derive(Serialize)]
        struct CloseRequest<'a> {
            instance: &'a str,
        }
        self.command("youtubeClose", CloseRequest { instance })
            .await
    }
}

/// Page served by a native YouTube host WebView at `url`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileYouTubePage<'a> {
    pub url: &'a str,
    pub origin: &'a str,
    pub path: &'a str,
    pub html: &'a str,
    pub content_security_policy: &'a str,
}

/// Host placement in CSS pixels of the app WebView, with its device pixel ratio.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct MobileYouTubeBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub visible: bool,
}

/// Access Ganbaru's Android media adapter from managed Tauri state.
pub trait MobileMediaExt<R: Runtime> {
    fn mobile_media(&self) -> &MobileMedia<R>;
}

impl<R: Runtime, T: Manager<R>> MobileMediaExt<R> for T {
    fn mobile_media(&self) -> &MobileMedia<R> {
        self.state::<MobileMedia<R>>().inner()
    }
}
