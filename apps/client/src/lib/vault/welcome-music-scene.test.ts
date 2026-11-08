import { describe, expect, it } from "vitest";
import {
  MUSIC_BEATS,
  MUSIC_DEFAULT_RAIL,
  MUSIC_EVENT,
  MUSIC_PLAYLISTS,
  MUSIC_PROJECT_PLAYLISTS,
  MUSIC_PROJECT_PRESET,
  MUSIC_RAIL,
  MUSIC_STILL,
  musicBuilderShown,
  musicBuilderTouches,
  musicCalendarShown,
  musicClock,
  musicEventPanelFrame,
  musicEventRail,
  musicItemReviewed,
  musicNowPlaying,
  musicPanelOpen,
  musicPhase,
  musicPhaseSeconds,
  musicPickerPhase,
  musicPlaylistBump,
  musicPlaylistChecked,
  musicPlaylistCount,
  musicRailRecorded,
  musicSectionOpen,
  musicReviewItem,
  musicReviewItemIn,
  musicReviewProgress,
  musicReviewProgressOfTrack,
  musicNotification,
  musicTracks,
  type MusicScenePlaylist,
} from "./welcome-music-scene";

const B = MUSIC_BEATS;

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(B.end / step) + 1 }, (_, index) => index * step);
}

function lookup(key: MusicScenePlaylist): { icon: string; name: string } {
  return { icon: `icon:${key}`, name: `name:${key}` };
}

describe("welcome music scene", () => {
  it("keeps its beats in order", () => {
    const beats = Object.values(B);
    expect(beats).toEqual([...beats].sort((a, b) => a - b));
  });

  it("keeps every fraction between 0 and 1", () => {
    for (const elapsed of moments()) {
      const touches = musicBuilderTouches(elapsed);
      const values = [
        musicBuilderShown(elapsed),
        musicCalendarShown(elapsed),
        musicReviewItemIn(elapsed),
        musicReviewProgressOfTrack(elapsed),
        touches.localPick,
        touches.localSave,
        touches.youtubePick,
        touches.youtubeSave,
        ...MUSIC_PLAYLISTS.map((entry) => musicPlaylistBump(elapsed, entry.key)),
        musicNowPlaying(elapsed)?.progress ?? 0,
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("hands the builder over to the Calendar without a gap", () => {
    expect(musicBuilderShown(0)).toBe(1);
    expect(musicCalendarShown(0)).toBe(0);
    expect(musicBuilderShown(B.calendarIn)).toBe(0);
    expect(musicCalendarShown(B.calendarIn)).toBe(1);
    for (const elapsed of moments()) {
      expect(musicBuilderShown(elapsed) + musicCalendarShown(elapsed)).toBeGreaterThan(0);
    }
  });

  it("reviews the local track, then the YouTube video, then opens the next video", () => {
    expect(musicReviewItem(0)).toEqual({ source: "local", row: 2 });
    expect(musicReviewItem(B.localSaveTap)).toEqual({ source: "youtube", row: 1 });
    expect(musicReviewItem(B.youtubeSaveTap)).toEqual({ source: "youtube", row: 2 });
    expect(musicItemReviewed(B.localSaveTap - 1, "local", 2)).toBe(false);
    expect(musicItemReviewed(B.localSaveTap, "local", 2)).toBe(true);
    expect(musicItemReviewed(B.end, "youtube", 2)).toBe(false);
  });

  it("counts review progress up by one at each save", () => {
    const total = musicTracks("local").length + musicTracks("youtube").length;
    expect(musicReviewProgress(0)).toEqual({ reviewed: 3, total });
    expect(musicReviewProgress(B.localSaveTap)).toEqual({ reviewed: 4, total });
    expect(musicReviewProgress(B.youtubeSaveTap)).toEqual({ reviewed: 5, total });
  });

  it("checks a playlist at the pick tap and adds the track to it only at the save", () => {
    expect(musicPlaylistChecked(B.localPickTap - 1, "workFocus")).toBe(false);
    expect(musicPlaylistChecked(B.localPickTap, "workFocus")).toBe(true);
    expect(musicPlaylistCount(B.localPickTap, "workFocus")).toBe(12);
    expect(musicPlaylistChecked(B.localSaveTap, "workFocus")).toBe(false);
    expect(musicPlaylistCount(B.localSaveTap, "workFocus")).toBe(13);
    expect(musicPlaylistChecked(B.youtubePickTap, "breakCalm")).toBe(true);
    expect(musicPlaylistCount(B.youtubeSaveTap, "breakCalm")).toBe(8);
    expect(musicPlaylistCount(B.end, "workGanbare")).toBe(8);
    expect(musicPlaylistBump(B.localSaveTap + 100, "workFocus")).toBeGreaterThan(0);
    expect(musicPlaylistBump(B.localSaveTap + 100, "breakCalm")).toBe(0);
  });

  it("restarts the review's progress on each track it opens", () => {
    expect(musicReviewProgressOfTrack(B.localSaveTap)).toBe(0);
    expect(musicReviewProgressOfTrack(B.localSaveTap - 1)).toBeGreaterThan(0);
    expect(musicReviewItemIn(B.localSaveTap)).toBe(0);
  });

  it("gives the event the project's playlist for each phase once the project is picked", () => {
    const before = musicEventPanelFrame(B.projectPick - 1, "Title", lookup);
    const after = musicEventPanelFrame(B.projectPick, "Title", lookup);
    expect(before.picked).toBe(false);
    expect(before.music?.enabled).toBe(false);
    expect(before.music?.rows.every((row) => row.fill === 0)).toBe(true);
    expect(after.picked).toBe(true);
    expect(after.preset).toBe(MUSIC_PROJECT_PRESET);
    expect(after.music?.rows.map((row) => [row.phase, row.playlistName, row.fill])).toEqual(
      MUSIC_PROJECT_PLAYLISTS.map(({ phase, playlist }) => [phase, `name:${playlist}`, 1]),
    );
    expect(musicEventRail(B.projectPick - 1)).toBe(MUSIC_DEFAULT_RAIL);
    expect(musicEventRail(B.projectPick)).toBe(MUSIC_RAIL);
  });

  it("opens the Music section and scrolls to it only after its header is tapped", () => {
    const closed = musicEventPanelFrame(B.musicTap - 1, "Title", lookup);
    const open = musicEventPanelFrame(B.saveTap - 1, "Title", lookup);
    expect(closed.music?.expanded).toBe(0);
    expect(closed.scroll).toBe(0);
    expect(open.music?.expanded).toBe(1);
    expect(open.scroll).toBe(1);
    expect(musicPanelOpen(B.eventTap - 1)).toBe(false);
    expect(musicPanelOpen(B.eventTap)).toBe(true);
    expect(musicPanelOpen(B.saveTap)).toBe(false);
  });

  it("opens the Focus, then the short break picker, each on the project playlist and closed by its tap", () => {
    expect(musicEventPanelFrame(B.focusPickerTap - 1, "Title", lookup).music?.picker).toBeNull();
    const focus = musicEventPanelFrame(B.focusPickerTap, "Title", lookup).music?.picker;
    expect(focus?.phase).toBe("focus");
    expect(focus?.selected).toBe("workFocus");
    expect(focus?.options.map((option) => [option.key, option.count])).toEqual(
      MUSIC_PLAYLISTS.map((entry) => [entry.key, entry.tracks + (entry.key === "workFocus" || entry.key === "breakCalm" ? 1 : 0)]),
    );
    expect(musicPickerPhase(B.focusOptionTap - 1)).toBe("focus");
    expect(musicPickerPhase(B.focusOptionTap)).toBeNull();
    expect(musicPickerPhase(B.breakPickerTap)).toBe("short-break");
    expect(musicEventPanelFrame(B.breakPickerTap, "Title", lookup).music?.picker?.selected).toBe("breakCalm");
    expect(musicPickerPhase(B.breakOptionTap)).toBeNull();
    for (const elapsed of moments()) {
      const phase = musicPickerPhase(elapsed);
      if (phase) expect(musicSectionOpen(elapsed)).toBe(1);
      if (phase) expect(musicPanelOpen(elapsed)).toBe(true);
    }
  });

  it("moves the current time forward only", () => {
    let previous = musicClock(0);
    for (const elapsed of moments()) {
      const now = musicClock(elapsed);
      expect(now).toBeGreaterThanOrEqual(previous);
      previous = now;
    }
    expect(musicClock(B.focusStart)).toBe(MUSIC_EVENT.start);
  });

  it("runs focus, then the first break, then focus again", () => {
    expect(musicPhase(B.focusStart - 1)).toBeNull();
    expect(musicPhase(B.focusStart)).toBe("focus");
    expect(musicPhase(B.breakStart)).toBe("short-break");
    expect(musicPhase(B.breakEnd)).toBe("focus");
    const seconds = musicPhaseSeconds(B.breakStart);
    expect(seconds.total).toBe(Math.round((MUSIC_RAIL.breaks[0].end - MUSIC_RAIL.breaks[0].start) * 60));
    expect(seconds.remaining).toBe(seconds.total);
  });

  it("plays each phase's project playlist", () => {
    expect(musicNowPlaying(B.focusStart - 1)).toBeNull();
    for (const elapsed of moments(100).filter((value) => value >= B.focusStart)) {
      const phase = musicPhase(elapsed);
      const playing = musicNowPlaying(elapsed);
      const expected = MUSIC_PROJECT_PLAYLISTS.find((entry) => entry.phase === phase)?.playlist;
      expect(playing?.playlist).toBe(expected);
      expect(playing && musicTracks(playing.source)[playing.row]).toBeDefined();
    }
  });

  it("never records focus over a break", () => {
    for (const elapsed of moments()) {
      for (const span of musicRailRecorded(elapsed)) {
        expect(span.end).toBeGreaterThan(span.start);
        for (const pause of MUSIC_RAIL.breaks) {
          expect(span.end <= pause.start || span.start >= pause.end).toBe(true);
        }
      }
    }
  });

  it("shows the autoplay notification once per phase, each gone before the next phase", () => {
    expect(musicNotification(B.focusStart)).toBe(0);
    expect(musicNotification(B.focusNoticeIn + 400)).toBe(1);
    expect(musicNotification(B.breakStart)).toBe(0);
    expect(musicNotification(B.breakNoticeIn + 400)).toBe(1);
    expect(musicNotification(B.breakEnd)).toBe(0);
    for (const elapsed of moments()) {
      const shown = musicNotification(elapsed);
      expect(shown).toBeGreaterThanOrEqual(0);
      expect(shown).toBeLessThanOrEqual(1);
      if (shown > 0) expect(musicNowPlaying(elapsed)?.playlist).toBe(elapsed < B.breakStart ? "workFocus" : "breakCalm");
    }
  });

  it("stills on the notification with the break's video", () => {
    expect(musicNotification(MUSIC_STILL)).toBe(1);
    expect(musicPhase(MUSIC_STILL)).toBe("short-break");
    expect(musicNowPlaying(MUSIC_STILL)).toMatchObject({ source: "youtube", playlist: "breakCalm" });
  });
});
