import { describe, expect, it } from "vitest";
import {
  YOUTUBE_START_IDLE,
  YOUTUBE_START_TIMEOUT_MS,
  beginYouTubeStart,
  isYouTubeStarting,
  youtubeStartAfterDeadline,
  youtubeStartAfterStatus,
} from "./youtube-start-state";

describe("YouTube start state", () => {
  it("waits only for loads that autoplay", () => {
    expect(isYouTubeStarting(beginYouTubeStart(3, true, 1_000))).toBe(true);
    expect(beginYouTubeStart(3, false, 1_000)).toEqual(YOUTUBE_START_IDLE);
  });

  it("keeps waiting through ready and loading reports and settles on any other status", () => {
    const starting = beginYouTubeStart(3, true, 1_000);
    expect(youtubeStartAfterStatus(starting, "ready")).toBe(starting);
    expect(youtubeStartAfterStatus(starting, "loading")).toBe(starting);
    for (const status of ["playing", "paused", "ended", "error", "idle"] as const) {
      expect(youtubeStartAfterStatus(starting, status)).toEqual(YOUTUBE_START_IDLE);
    }
  });

  it("settles a start stuck in ready at its deadline but never one of another generation", () => {
    const starting = beginYouTubeStart(3, true, 1_000);
    const deadline = 1_000 + YOUTUBE_START_TIMEOUT_MS;
    expect(youtubeStartAfterDeadline(starting, 3, deadline - 1)).toBe(starting);
    expect(youtubeStartAfterDeadline(starting, 2, deadline)).toBe(starting);
    expect(youtubeStartAfterDeadline(starting, 3, deadline)).toEqual(YOUTUBE_START_IDLE);
    expect(youtubeStartAfterDeadline(YOUTUBE_START_IDLE, 3, deadline)).toEqual(YOUTUBE_START_IDLE);
  });

  it("extends the deadline by the time spent resolving what to play", () => {
    const starting = beginYouTubeStart(3, true, 1_000, 18_000);
    const deadline = 1_000 + 18_000 + YOUTUBE_START_TIMEOUT_MS;
    expect(youtubeStartAfterDeadline(starting, 3, deadline - 1)).toBe(starting);
    expect(youtubeStartAfterDeadline(starting, 3, deadline)).toEqual(YOUTUBE_START_IDLE);
  });
});
