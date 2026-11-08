import { describe, expect, it } from "vitest";
import {
  MUSIC_BEATS,
  MUSIC_DAY_VIEW,
  MUSIC_EVENT,
  MUSIC_LOCAL_TRACKS,
  MUSIC_PLAYLISTS,
  MUSIC_PROJECT_PLAYLISTS,
  MUSIC_RAIL,
  MUSIC_STILL,
  MUSIC_YOUTUBE_VIDEOS,
  musicAddMenu,
  musicBuilderShown,
  musicCalendarShown,
  musicClock,
  musicNowPlaying,
  musicPhase,
  musicPlaylistBump,
  musicPlaylistCount,
  musicProjectChosen,
  musicProjectPicker,
  musicRailRecorded,
  musicRowFill,
  musicSource,
  musicSourceIn,
  musicTouches,
  musicTrackAdded,
} from "./welcome-music-scene";

const B = MUSIC_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome music scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("adds a local track to the work playlist and a video to the calm break playlist", () => {
    expect(musicSource(B.localAddTap)).toBe("local");
    expect(musicAddMenu(B.localAddTap - 1)).toBeNull();
    expect(musicAddMenu(B.localAddTap + 400)).toMatchObject({ source: "local", row: 0, playlist: "workFocus", shown: 1, picked: false });
    expect(musicAddMenu(B.localPickTap)).toMatchObject({ picked: true });
    expect(musicAddMenu(B.localMenuOut + 1000)).toBeNull();
    expect(musicTrackAdded(B.localPickTap - 1, "local", 0)).toBe(false);
    expect(musicTrackAdded(B.localPickTap, "local", 0)).toBe(true);
    expect(musicPlaylistCount(0, "workFocus") + 1).toBe(musicPlaylistCount(B.localPickTap, "workFocus"));

    expect(musicSource(B.youtubeTap)).toBe("youtube");
    expect(musicAddMenu(B.youtubeAddTap + 400)).toMatchObject({ source: "youtube", row: 1, playlist: "breakCalm" });
    expect(musicPlaylistCount(0, "breakCalm") + 1).toBe(musicPlaylistCount(B.youtubePickTap, "breakCalm"));
    expect(musicTrackAdded(B.end, "youtube", 1)).toBe(true);
    expect(musicPlaylistCount(B.end, "breakActive")).toBe(musicPlaylistCount(0, "breakActive"));
  });

  it("bumps a playlist's count only right after a track joins it", () => {
    expect(musicPlaylistBump(B.localPickTap - 1, "workFocus")).toBe(0);
    expect(musicPlaylistBump(B.localPickTap + 100, "workFocus")).toBeGreaterThan(0);
    expect(musicPlaylistBump(B.localPickTap + 100, "breakCalm")).toBe(0);
    expect(musicPlaylistBump(B.youtubePickTap + 100, "breakCalm")).toBeGreaterThan(0);
    expect(musicPlaylistBump(B.end, "workFocus")).toBe(0);
  });

  it("hands over from the builder to the event panel", () => {
    expect(musicBuilderShown(0)).toBe(1);
    expect(musicCalendarShown(0)).toBe(0);
    expect(musicBuilderShown(B.calendarIn)).toBe(0);
    expect(musicCalendarShown(B.calendarIn)).toBe(1);
    expect(musicAddMenu(B.builderOut)).toBeNull();
  });

  it("fills the event's music from the project after it is picked, one phase at a time", () => {
    expect(musicProjectChosen(B.projectPickTap - 1)).toBe(false);
    expect(musicProjectPicker(B.projectTap + 400)).toBe(1);
    expect(musicProjectChosen(B.projectPickTap)).toBe(true);
    expect(musicProjectPicker(B.projectPickTap + 1000)).toBe(0);
    for (const index of MUSIC_PROJECT_PLAYLISTS.keys()) {
      expect(musicRowFill(B.projectPickTap, index)).toBe(0);
      expect(musicRowFill(B.focusStart, index)).toBe(1);
    }
    const mid = Array.from({ length: 2000 }, (_, step) => B.projectPickTap + step).find((elapsed) => musicRowFill(elapsed, 0) === 1);
    expect(mid).toBeDefined();
    expect(musicRowFill(mid ?? 0, MUSIC_PROJECT_PLAYLISTS.length - 1)).toBeLessThan(1);
  });

  it("walks the current time from before the event into its second focus within the strip", () => {
    let previous = -Infinity;
    for (const elapsed of moments()) {
      const clock = musicClock(elapsed);
      expect(clock).toBeGreaterThanOrEqual(previous);
      expect(clock).toBeGreaterThan(MUSIC_DAY_VIEW.start);
      expect(clock).toBeLessThan(MUSIC_DAY_VIEW.end);
      previous = clock;
    }
    expect(MUSIC_RAIL).toMatchObject({ start: MUSIC_EVENT.start, end: MUSIC_EVENT.end });
    expect(musicClock(B.focusStart)).toBe(MUSIC_EVENT.start);
    expect(musicClock(B.breakStart)).toBe(MUSIC_RAIL.breaks[0].start);
    expect(musicClock(B.breakEnd)).toBe(MUSIC_RAIL.breaks[0].end);
  });

  it("plays the phase's playlist, starting a fresh track at each phase boundary", () => {
    expect(musicNowPlaying(B.focusStart - 1)).toBeNull();
    expect(musicPhase(B.focusStart - 1)).toBeNull();
    expect(musicPhase(B.focusStart)).toBe("focus");
    expect(musicNowPlaying(B.focusStart + 500)).toMatchObject({ source: "local", index: 0, playlist: "workFocus", swap: 1 });
    expect(musicPhase(B.breakStart)).toBe("short-break");
    expect(musicNowPlaying(B.breakStart)).toMatchObject({ source: "youtube", index: 1, playlist: "breakCalm", swap: 0, progress: 0 });
    expect(musicPhase(B.breakEnd)).toBe("focus");
    expect(musicNowPlaying(B.breakEnd + 500)).toMatchObject({ source: "local", playlist: "workFocus", swap: 1 });
    for (const elapsed of moments()) {
      const playing = musicNowPlaying(elapsed);
      const phase = musicPhase(elapsed);
      if (!playing || !phase) continue;
      const assigned = MUSIC_PROJECT_PLAYLISTS.find((entry) => entry.phase === phase)?.playlist;
      expect(playing.playlist).toBe(assigned);
      expect(musicPlaylistCount(elapsed, playing.playlist)).toBeGreaterThan(0);
      const tracks = playing.source === "local" ? MUSIC_LOCAL_TRACKS : MUSIC_YOUTUBE_VIDEOS;
      expect(tracks[playing.index]).toBeDefined();
    }
  });

  it("records focus on the rail only while the event's Focus runs, never over its break", () => {
    expect(musicRailRecorded(B.focusStart)).toEqual([]);
    let previous = 0;
    for (const elapsed of moments()) {
      const recorded = musicRailRecorded(elapsed);
      const minutes = recorded.reduce((sum, span) => sum + span.end - span.start, 0);
      expect(minutes).toBeGreaterThanOrEqual(previous);
      for (const span of recorded) {
        expect(MUSIC_RAIL.breaks.some((pause) => span.start < pause.end && span.end > pause.start)).toBe(false);
      }
      previous = minutes;
    }
  });

  it("keeps every animated value between 0 and 1", () => {
    for (const elapsed of moments()) {
      const playing = musicNowPlaying(elapsed);
      const values = [
        musicBuilderShown(elapsed),
        musicCalendarShown(elapsed),
        musicSourceIn(elapsed),
        musicAddMenu(elapsed)?.shown ?? 0,
        musicProjectPicker(elapsed),
        playing?.shown ?? 0,
        playing?.swap ?? 0,
        playing?.progress ?? 0,
        ...Object.values(musicTouches(elapsed)),
        ...MUSIC_PLAYLISTS.map((entry) => musicPlaylistBump(elapsed, entry.key)),
        ...MUSIC_PROJECT_PLAYLISTS.map((_, index) => musicRowFill(elapsed, index)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("shows the event's music filled in and the break's video playing in its still", () => {
    expect(MUSIC_STILL).toBeLessThan(B.end);
    expect(musicCalendarShown(MUSIC_STILL)).toBe(1);
    expect(musicProjectPicker(MUSIC_STILL)).toBe(0);
    expect(musicPhase(MUSIC_STILL)).toBe("short-break");
    expect(musicNowPlaying(MUSIC_STILL)).toMatchObject({ source: "youtube", playlist: "breakCalm", shown: 1, swap: 1 });
  });
});
