import { describe, expect, it } from "vitest";
import { musicBuilderPlaybackDecision } from "./playback-transition";

describe("music builder playback transitions", () => {
  it("keeps Review ownership during internal builder navigation", () => {
    expect(musicBuilderPlaybackDecision(true, "internal-navigation")).toBe("continue-review");
  });

  it("releases or supersedes ownership when another playback context starts", () => {
    expect(musicBuilderPlaybackDecision(true, "explicit-playback")).toBe("release-review");
    expect(musicBuilderPlaybackDecision(true, "automation-boundary")).toBe("supersede-review");
  });

  it("does nothing when Review does not own playback", () => {
    expect(musicBuilderPlaybackDecision(false, "explicit-playback")).toBe("no-review-action");
  });
});
