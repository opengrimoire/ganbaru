import { translate } from "$lib/i18n/translator.svelte";
import type { NativeMusicIssue } from "./session/native-session";
import type { MusicErrorKey } from "./user-errors";

/**
 * Stable playback failure codes reported by the browser playback adapters.
 *
 * The code, not display text, travels through the native Music session in the snapshot `error`
 * field, so every window can show the message in its own language.
 */
export const MUSIC_PLAYBACK_ERROR_CODES = [
  "youtube-invalid-video",
  "youtube-html5",
  "youtube-unavailable",
  "youtube-embedding-blocked",
  "youtube-client-rejected",
  "youtube-failed",
  "youtube-playlist-empty",
  "youtube-player-failed",
  "local-not-ready",
  "local-aborted",
  "local-network",
  "local-decode",
  "local-unsupported",
  "local-failed",
  "local-play-rejected",
] as const;

export type MusicPlaybackErrorCode = (typeof MUSIC_PLAYBACK_ERROR_CODES)[number];

/** Message for every playback failure code. */
export const MUSIC_PLAYBACK_ERROR_MESSAGE_KEYS: Readonly<Record<MusicPlaybackErrorCode, MusicErrorKey>> = {
  "youtube-invalid-video": "music.errors.youtubeInvalidVideo",
  "youtube-html5": "music.errors.youtubeHtml5",
  "youtube-unavailable": "music.errors.youtubeUnavailable",
  "youtube-embedding-blocked": "music.errors.youtubeEmbeddingBlocked",
  "youtube-client-rejected": "music.errors.youtubeClientRejected",
  "youtube-failed": "music.errors.playbackFailed",
  "youtube-playlist-empty": "music.errors.youtubePlaylistEmpty",
  "youtube-player-failed": "music.errors.youtubePlayerFailed",
  "local-not-ready": "music.errors.localNotReady",
  "local-aborted": "music.errors.localAborted",
  "local-network": "music.errors.localNetwork",
  "local-decode": "music.errors.localDecode",
  "local-unsupported": "music.errors.localUnsupported",
  "local-failed": "music.errors.playbackFailed",
  "local-play-rejected": "music.errors.localPlayRejected",
};

/** Whether `value` is a playback failure code. */
export function isMusicPlaybackErrorCode(value: unknown): value is MusicPlaybackErrorCode {
  return typeof value === "string" && Object.hasOwn(MUSIC_PLAYBACK_ERROR_MESSAGE_KEYS, value);
}

/**
 * Maps a YouTube IFrame Player API error code to a playback failure code.
 *
 * @param code The `data` value of the player's `onError` event.
 */
export function youtubePlaybackErrorCode(code: number): MusicPlaybackErrorCode {
  switch (code) {
    case 2: return "youtube-invalid-video";
    case 5: return "youtube-html5";
    case 100: return "youtube-unavailable";
    case 101:
    case 150: return "youtube-embedding-blocked";
    case 153: return "youtube-client-rejected";
    default: return "youtube-failed";
  }
}

/**
 * Maps an `HTMLMediaElement` error code to a playback failure code.
 *
 * @param code The element's `MediaError.code`, or undefined when the element reports none.
 */
export function localMediaErrorCode(code: number | undefined): MusicPlaybackErrorCode {
  switch (code) {
    case 1: return "local-aborted";
    case 2: return "local-network";
    case 3: return "local-decode";
    case 4: return "local-unsupported";
    default: return "local-failed";
  }
}

/**
 * The localized message for a playback error in a Music snapshot.
 *
 * Known codes get their own message. Anything else, such as diagnostic text the native session
 * wrote, gets the generic playback message.
 *
 * @param error The snapshot `error` value.
 */
export function musicPlaybackErrorMessage(error: string): string {
  return translate(isMusicPlaybackErrorCode(error) ? MUSIC_PLAYBACK_ERROR_MESSAGE_KEYS[error] : "music.errors.playbackFailed");
}

/**
 * The localized notice for a native session issue that has no playback error, or null when the
 * issue needs no notice in the player.
 *
 * @param issue The snapshot `issue` value.
 * @param decodesBrowserMedia Whether this window plays browser sources itself.
 */
export function nativeMusicIssueMessage(issue: NativeMusicIssue | null, decodesBrowserMedia: boolean): string | null {
  switch (issue) {
    case "browser-host-unavailable":
      return translate(decodesBrowserMedia ? "music.nativeSession.browserUnavailable" : "music.nativeSession.browserUnavailableOtherWindow");
    case "interrupted":
      return translate("music.nativeSession.interrupted");
    case "no-eligible-items":
    case "source-failure":
    case "persistence-failure":
    case "restored":
    case null:
      return null;
  }
}

/**
 * The localized player message for a native snapshot: its playback error when it has one,
 * otherwise the notice for its issue.
 *
 * @param error The snapshot `error` value.
 * @param issue The snapshot `issue` value.
 * @param decodesBrowserMedia Whether this window plays browser sources itself.
 */
export function nativeMusicSnapshotMessage(
  error: string | null,
  issue: NativeMusicIssue | null,
  decodesBrowserMedia: boolean,
): string | null {
  return error ? musicPlaybackErrorMessage(error) : nativeMusicIssueMessage(issue, decodesBrowserMedia);
}
