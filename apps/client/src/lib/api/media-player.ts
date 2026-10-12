import { invoke } from "@tauri-apps/api/core";

export type LocalBackendKind = "none" | "rodio" | "webview" | "media3";

export interface MediaProbe {
  path: string;
  title: string;
  fileSizeBytes: number;
  extension: string | null;
  mediaKind: "audio" | "video" | "unknown";
  playableStartMs: number | null;
}

/** Inspect a local source without changing the native Music session. */
export function probeLocalMedia(path: string): Promise<MediaProbe> {
  return invoke("media_player_probe", { path });
}
