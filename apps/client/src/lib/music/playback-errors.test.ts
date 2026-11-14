import { describe, expect, it } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import {
  localMediaErrorCode,
  MUSIC_PLAYBACK_ERROR_CODES,
  MUSIC_PLAYBACK_ERROR_MESSAGE_KEYS,
  musicPlaybackErrorMessage,
  nativeMusicIssueMessage,
  nativeMusicSnapshotMessage,
  youtubePlaybackErrorCode,
} from "./playback-errors";

function catalogText(catalog: unknown, key: string): unknown {
  return key.split(".").reduce<unknown>(
    (node, part) => (node && typeof node === "object" ? (node as Record<string, unknown>)[part] : undefined),
    catalog,
  );
}

describe("music playback error messages", () => {
  it("has English and Spanish text for every playback error code", () => {
    for (const code of MUSIC_PLAYBACK_ERROR_CODES) {
      const key = MUSIC_PLAYBACK_ERROR_MESSAGE_KEYS[code];
      expect(typeof catalogText(en, key), `en ${code}`).toBe("string");
      expect(typeof catalogText(es, key), `es ${code}`).toBe("string");
    }
  });

  it("maps YouTube player error numbers to stable codes", () => {
    expect(youtubePlaybackErrorCode(2)).toBe("youtube-invalid-video");
    expect(youtubePlaybackErrorCode(5)).toBe("youtube-html5");
    expect(youtubePlaybackErrorCode(100)).toBe("youtube-unavailable");
    expect(youtubePlaybackErrorCode(101)).toBe("youtube-embedding-blocked");
    expect(youtubePlaybackErrorCode(150)).toBe("youtube-embedding-blocked");
    expect(youtubePlaybackErrorCode(153)).toBe("youtube-client-rejected");
    expect(youtubePlaybackErrorCode(999)).toBe("youtube-failed");
  });

  it("maps media element error codes to stable codes", () => {
    expect(localMediaErrorCode(1)).toBe("local-aborted");
    expect(localMediaErrorCode(2)).toBe("local-network");
    expect(localMediaErrorCode(3)).toBe("local-decode");
    expect(localMediaErrorCode(4)).toBe("local-unsupported");
    expect(localMediaErrorCode(undefined)).toBe("local-failed");
  });

  it("shows the specific message for a known code", () => {
    expect(musicPlaybackErrorMessage("youtube-embedding-blocked")).toBe(en.music.errors.youtubeEmbeddingBlocked);
  });

  it("shows the generic playback message for native English text or unknown codes", () => {
    expect(musicPlaybackErrorMessage("The decoder stopped unexpectedly.")).toBe(en.music.errors.playbackFailed);
    expect(musicPlaybackErrorMessage("toString")).toBe(en.music.errors.playbackFailed);
  });
});

describe("native music snapshot messages", () => {
  it("explains issues a person can act on and stays quiet for the rest", () => {
    expect(nativeMusicIssueMessage("browser-host-unavailable", true)).toBe(en.music.nativeSession.browserUnavailable);
    expect(nativeMusicIssueMessage("browser-host-unavailable", false)).toBe(en.music.nativeSession.browserUnavailableOtherWindow);
    expect(nativeMusicIssueMessage("interrupted", true)).toBe(en.music.nativeSession.interrupted);
    expect(nativeMusicIssueMessage("restored", true)).toBeNull();
    expect(nativeMusicIssueMessage(null, true)).toBeNull();
  });

  it("prefers the playback error over the issue", () => {
    expect(nativeMusicSnapshotMessage("local-decode", "interrupted", true)).toBe(en.music.errors.localDecode);
    expect(nativeMusicSnapshotMessage(null, "interrupted", true)).toBe(en.music.nativeSession.interrupted);
  });
});
