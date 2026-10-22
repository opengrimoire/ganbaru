import { Channel, invoke } from "@tauri-apps/api/core";
import { translate } from "$lib/i18n/translator.svelte";

export interface MediaFolderTrack {
  path: string;
  title: string;
  artworkPath: string | null;
}

export interface MediaFolderSelection {
  folderPath: string;
  displayName?: string | null;
  tracks: MediaFolderTrack[];
  truncated: boolean;
}

export interface MusicYouTubeMetadataItem {
  videoId: string | null;
  title: string;
  channel: string;
}

export interface MusicYouTubeMetadataResponse {
  playlist: MusicYouTubeMetadataItem | null;
  videos: MusicYouTubeMetadataItem[];
  truncated: boolean;
}

export async function pickMediaFolder(): Promise<MediaFolderSelection | null> {
  return invoke("music_pick_media_folder");
}

export async function detectDefaultMusicFolder(): Promise<MediaFolderSelection | null> {
  return invoke("music_detect_default_folder");
}

export async function pickMusicRootBindingFolder(): Promise<string | null> {
  return invoke<string | null>("music_pick_root_binding_folder");
}

export async function pickMediaFile(): Promise<string | null> {
  return invoke("music_pick_media_file");
}

export async function pickSoundscapeFile(): Promise<string | null> {
  return invoke("music_pick_soundscape_file");
}

/**
 * Lets the user pick an image, copies it into the active vault as managed artwork, and returns
 * the managed reference an artwork override stores, or `null` when canceled.
 */
export async function pickArtworkFile(): Promise<string | null> {
  return invoke("music_pick_artwork_file");
}

/** Reads managed artwork of the active vault, by its reference, as a data URL. */
export async function loadMusicArtworkAssetDataUrl(reference: string): Promise<string> {
  return invoke("music_artwork_asset_data_url", { reference });
}

/** Hosts managed artwork of the active vault for one playback lifecycle. */
export async function registerMusicArtworkAsset(reference: string, generation: number): Promise<string> {
  return invoke("music_register_artwork_asset", { reference, generation });
}

export async function pickAndReadMusicInterchangeFile(): Promise<string | null> {
  const contents = await invoke<unknown>("music_pick_and_read_interchange_file");
  if (contents !== null && typeof contents !== "string") throw new Error("Invalid Music import file response");
  return contents;
}

export async function loadArtworkDataUrl(path: string): Promise<string> {
  return invoke("music_artwork_data_url", { path });
}

export async function loadEmbeddedArtworkDataUrl(path: string): Promise<string | null> {
  return invoke("music_embedded_artwork_data_url", { path });
}

export async function registerMediaFile(path: string, generation: number): Promise<string> {
  return invoke("music_register_media_file", { path, generation });
}

export async function registerEmbeddedArtwork(
  path: string,
  generation: number,
): Promise<string | null> {
  return invoke("music_register_embedded_artwork", { path, generation });
}

/** Retains only the hosted media URLs referenced by the current playback lifecycle. */
export async function retainHostedMedia(mediaUrls: string[], generation: number): Promise<void> {
  await invoke("music_retain_hosted_media", { mediaUrls, generation });
}

/** Removes registrations abandoned by one stale playback lifecycle. */
export async function unregisterHostedMedia(
  mediaUrls: string[],
  generation: number,
): Promise<void> {
  await invoke("music_unregister_hosted_media", { mediaUrls, generation });
}

export async function revealLocalFile(path: string): Promise<void> {
  await invoke("music_reveal_local_file", { path });
}

export async function getYouTubeHostUrl(): Promise<string> {
  return invoke("music_youtube_host_url");
}

/** Android native YouTube host instance: the visible player or the hidden source resolver. */
export type NativeYouTubeHostInstance = "playback" | "resolver";

/** Native YouTube host placement in CSS pixels of the app WebView. */
export interface NativeYouTubeHostBounds {
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  visible: boolean;
}

/** Opens a separate Android WebView for the YouTube host page; its messages arrive on `channel`. */
export async function openNativeYouTubeHost(
  instance: NativeYouTubeHostInstance,
  url: string,
  bounds: NativeYouTubeHostBounds,
  channel: Channel<unknown>,
): Promise<void> {
  await invoke("music_youtube_native_open", { instance, url, bounds, channel });
}

export async function setNativeYouTubeHostBounds(
  instance: NativeYouTubeHostInstance,
  bounds: NativeYouTubeHostBounds,
): Promise<void> {
  await invoke("music_youtube_native_bounds", { instance, bounds });
}

export async function postNativeYouTubeHostMessage(
  instance: NativeYouTubeHostInstance,
  message: Record<string, unknown>,
): Promise<void> {
  await invoke("music_youtube_native_post", { instance, message });
}

export async function closeNativeYouTubeHost(instance: NativeYouTubeHostInstance): Promise<void> {
  await invoke("music_youtube_native_close", { instance });
}

export async function getYouTubeMetadata(
  playlistId: string | null,
  videoIds: string[],
): Promise<MusicYouTubeMetadataResponse> {
  return invoke("music_youtube_metadata", {
    request: { playlistId, videoIds },
  });
}

/** Loads a validated YouTube thumbnail from the device-local image cache. */
export async function getYouTubeThumbnail(videoId: string): Promise<string | null> {
  return invoke("music_youtube_thumbnail", { videoId });
}

/**
 * Asks natively before playing local files that no picker or root binding on this device
 * granted. Resolves `false` when the user cancels; granted files never show the dialog.
 */
export async function confirmLocalMusicSources(paths: readonly string[]): Promise<boolean> {
  return invoke<boolean>("music_confirm_local_sources", {
    paths,
    prompt: {
      title: translate("music.nativeSession.localSourceReviewTitle"),
      message: translate("music.nativeSession.localSourceReviewMessage"),
      allowLabel: translate("music.nativeSession.localSourceReviewAllow"),
      cancelLabel: translate("music.nativeSession.localSourceReviewCancel"),
      moreLabel: translate("music.nativeSession.localSourceReviewMore"),
    },
  });
}
