import { describe, expect, it } from "vitest";
import {
  musicHardwareActionFromKey,
  parseMusicHardwareControlPayload,
} from "./hardware-controls";

describe("musicHardwareActionFromKey", () => {
  it("maps browser media-key names to player actions", () => {
    expect(musicHardwareActionFromKey({ key: "MediaPlayPause", code: "" })).toBe("playPause");
    expect(musicHardwareActionFromKey({ key: "MediaTrackNext", code: "" })).toBe("nextTrack");
    expect(musicHardwareActionFromKey({ key: "MediaTrackPrevious", code: "" })).toBe("previousTrack");
  });

  it("falls back to the physical key code when needed", () => {
    expect(musicHardwareActionFromKey({ key: "Unidentified", code: "AudioPlay" })).toBe("play");
    expect(musicHardwareActionFromKey({ key: "Unidentified", code: "AudioStop" })).toBe("stop");
  });

  it("ignores normal keyboard shortcuts", () => {
    expect(musicHardwareActionFromKey({ key: " ", code: "Space" })).toBe(null);
    expect(musicHardwareActionFromKey({ key: "ArrowRight", code: "ArrowRight" })).toBe(null);
  });
});

describe("parseMusicHardwareControlPayload", () => {
  it("validates native control payloads", () => {
    expect(parseMusicHardwareControlPayload({
      action: "seekTo",
      positionMs: 42_400.2,
    })).toEqual({
      action: "seekTo",
      positionMs: 42_400,
    });
  });

  it("rejects unknown actions", () => {
    expect(parseMusicHardwareControlPayload({ action: "deleteEverything" })).toBe(null);
  });
});
