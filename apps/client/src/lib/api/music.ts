import { invoke } from "@tauri-apps/api/core";

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

export async function pickArtworkFile(): Promise<string | null> {
  return invoke("music_pick_artwork_file");
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
