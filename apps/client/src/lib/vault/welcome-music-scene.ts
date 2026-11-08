/**
 * Timing for the welcome scene on Music: in the playlist builder, a local track is added to the Work (focus) playlist
 * and a YouTube video to Break (calm). Then a Calendar event picks the Thesis project, which fills the event's music
 * with the project's playlist for each phase, and as the event's Focus runs, the player starts the work playlist and
 * switches to the break playlist when the break starts, and back when it ends.
 */

import { blockerPageIn } from "./welcome-blocker";
import { easeInOutCubic, filmDayRails, filmSpan, filmTap, monotoneCurve, type FilmKeyframe, type FilmRail, type FilmSpan } from "./welcome-film";

/** The built-in playlists the scene uses, by their localized name's key. */
export type MusicScenePlaylist = "workFocus" | "breakCalm" | "breakActive";

/** Where a track in the scene comes from. */
export type MusicSceneSource = "local" | "youtube";

/** One built-in playlist with its icon and how many tracks it holds before the scene adds any. */
export interface MusicScenePlaylistEntry {
  key: MusicScenePlaylist;
  icon: string;
  tracks: number;
}

/** The playlists in the builder's list, as the app names and draws its built-in ones. */
export const MUSIC_PLAYLISTS: readonly MusicScenePlaylistEntry[] = [
  { key: "workFocus", icon: "lucide:laptop", tracks: 12 },
  { key: "breakCalm", icon: "lucide:armchair", tracks: 7 },
  { key: "breakActive", icon: "lucide:footprints", tracks: 9 },
];

/** One track or video in the scene: user content, so it reads the same in every app language. */
export interface MusicSceneTrack {
  title: string;
  artist: string;
  seconds: number;
  /** The palette slots its artwork or thumbnail blends. */
  artwork: readonly [number, number];
}

/** The tracks in the Local music root. */
export const MUSIC_LOCAL_TRACKS: readonly MusicSceneTrack[] = [
  { title: "Quiet Hours", artist: "Lumen Drift", seconds: 222, artwork: [18, 22] },
  { title: "Paper Lanterns", artist: "Kiso", seconds: 245, artwork: [13, 0] },
  { title: "Northbound", artist: "The Low Tides", seconds: 198, artwork: [16, 18] },
  { title: "Slow Orbit", artist: "Mira Vale", seconds: 301, artwork: [20, 7] },
];

/** The videos in the YouTube root. */
export const MUSIC_YOUTUBE_VIDEOS: readonly MusicSceneTrack[] = [
  { title: "Rain on a cabin window", artist: "Stillwater Sounds", seconds: 3600, artwork: [16, 9] },
  { title: "Soft guitar for a short break", artist: "Night Owl Radio", seconds: 750, artwork: [22, 13] },
  { title: "Morning piano", artist: "Aster Keys", seconds: 1450, artwork: [0, 20] },
];

/** One addition in the builder: the track's root and row, and the playlist it goes to. */
export interface MusicSceneAddition {
  source: MusicSceneSource;
  row: number;
  playlist: MusicScenePlaylist;
}

/** The phases an event's music names a playlist for, in the event panel's order. */
export type MusicScenePhase = "focus" | "short-break" | "long-break";

/** The Thesis project's playlist for each phase, which the event copies once the project is picked. */
export const MUSIC_PROJECT_PLAYLISTS: readonly { phase: MusicScenePhase; playlist: MusicScenePlaylist }[] = [
  { phase: "focus", playlist: "workFocus" },
  { phase: "short-break", playlist: "breakCalm" },
  { phase: "long-break", playlist: "breakActive" },
];

/** The Calendar event with Focus, in minutes of the day, and the part of the day the scene's strip shows. */
export const MUSIC_EVENT = { start: 10 * 60, end: 11 * 60 } as const;
export const MUSIC_DAY_VIEW = { start: 9 * 60 + 45, end: 11 * 60 + 10 } as const;

/** The event's Focus rail, planned by the Calendar's own rhythm for the Creative preset. */
export const MUSIC_RAIL: FilmRail = filmDayRails([{ ...MUSIC_EVENT, preset: "creative" }])[0];

/** The event's first break, where the player switches playlists. */
const FIRST_BREAK: FilmSpan = MUSIC_RAIL.breaks[0];

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const MUSIC_BEATS = {
  /** A tap on a local track's add button opens the playlists, and a tap on Work (focus) adds it there. */
  localAddTap: 1500,
  localPickTap: 2400,
  localMenuOut: 2750,
  /** A tap opens the YouTube root, and one of its videos is added to Break (calm). */
  youtubeTap: 3500,
  youtubeAddTap: 4700,
  youtubePickTap: 5600,
  youtubeMenuOut: 5950,
  /** The builder gives way to a Calendar event's panel. */
  builderOut: 6500,
  calendarIn: 6900,
  /** A tap opens the project picker, and a tap on Thesis fills the event's music from the project. */
  projectTap: 8000,
  projectPickTap: 9000,
  /** The event's Focus starts and the work playlist plays. */
  focusStart: 10400,
  /** The break starts and the break playlist plays, then the next focus brings back the work playlist. */
  breakStart: 12600,
  breakEnd: 14600,
  end: 16600,
} as const;

const B = MUSIC_BEATS;

/** The frame shown as a still with reduced motion: the event's music filled in and the break's video playing. */
export const MUSIC_STILL = B.breakStart + 900;

/** The builder's two additions, in the order they happen, with the beats of their taps. */
const ADDITIONS: readonly (MusicSceneAddition & { addTap: number; pickTap: number; menuOut: number })[] = [
  { source: "local", row: 0, playlist: "workFocus", addTap: B.localAddTap, pickTap: B.localPickTap, menuOut: B.localMenuOut },
  { source: "youtube", row: 1, playlist: "breakCalm", addTap: B.youtubeAddTap, pickTap: B.youtubePickTap, menuOut: B.youtubeMenuOut },
];

/** How long a menu takes to open and to close, how long a count's bump lasts, and the stagger as the event's music fills. */
const MENU_IN_MS = 160;
const MENU_OUT_MS = 140;
const BUMP_MS = 520;
const SCREEN_FADE_MS = 300;
const ROW_FILL_DELAY_MS = 180;
const ROW_FILL_STAGGER_MS = 240;
const ROW_FILL_MS = 220;
const PLAYER_IN_MS = 360;
/** How far a track plays across the scene, as a fraction of its length per second of scene time. */
const TRACK_PROGRESS_PER_MS = 1 / 9000;

/** The current time against scene time, in minutes of the day: it reaches the event, crosses its first focus and break, and moves into the next focus. */
const CLOCK: readonly FilmKeyframe[] = [
  [B.calendarIn, MUSIC_EVENT.start - 6],
  [B.focusStart, MUSIC_EVENT.start],
  [B.breakStart, FIRST_BREAK.start],
  [B.breakEnd, FIRST_BREAK.end],
  [B.end, FIRST_BREAK.end + 8],
];

/** How far the playlist builder shows, from 0 to 1. */
export function musicBuilderShown(elapsed: number): number {
  return 1 - easeInOutCubic(filmSpan(elapsed, B.builderOut, B.builderOut + SCREEN_FADE_MS));
}

/** How far the Calendar event's panel shows, from 0 to 1. */
export function musicCalendarShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.calendarIn - SCREEN_FADE_MS, B.calendarIn));
}

/** The builder's open root. */
export function musicSource(elapsed: number): MusicSceneSource {
  return elapsed >= B.youtubeTap ? "youtube" : "local";
}

/** How far the open root's list has faded in after it opens, from 0 to 1. */
export function musicSourceIn(elapsed: number): number {
  return blockerPageIn(elapsed, [B.youtubeTap]);
}

/** The playlists menu opened from a track's add button, or null: how far it shows, and whether its playlist is ticked. */
export function musicAddMenu(elapsed: number): (MusicSceneAddition & { shown: number; picked: boolean }) | null {
  for (const addition of ADDITIONS) {
    if (elapsed < addition.addTap || elapsed >= addition.menuOut + MENU_OUT_MS) continue;
    const shown = filmSpan(elapsed, addition.addTap, addition.addTap + MENU_IN_MS) * (1 - filmSpan(elapsed, addition.menuOut, addition.menuOut + MENU_OUT_MS));
    return { source: addition.source, row: addition.row, playlist: addition.playlist, shown, picked: elapsed >= addition.pickTap };
  }
  return null;
}

/** Whether the track at `row` of `source` is in a playlist yet, so its add button shows a tick. */
export function musicTrackAdded(elapsed: number, source: MusicSceneSource, row: number): boolean {
  return ADDITIONS.some((addition) => addition.source === source && addition.row === row && elapsed >= addition.pickTap);
}

/** How many tracks `playlist` holds at this moment. */
export function musicPlaylistCount(elapsed: number, playlist: MusicScenePlaylist): number {
  const base = MUSIC_PLAYLISTS.find((entry) => entry.key === playlist)?.tracks ?? 0;
  return base + ADDITIONS.filter((addition) => addition.playlist === playlist && elapsed >= addition.pickTap).length;
}

/** How far `playlist`'s count has bumped after a track joins it, from 0 to 1; 0 when it is not bumping. */
export function musicPlaylistBump(elapsed: number, playlist: MusicScenePlaylist): number {
  for (const addition of ADDITIONS) {
    if (addition.playlist !== playlist) continue;
    const progress = filmSpan(elapsed, addition.pickTap, addition.pickTap + BUMP_MS);
    if (progress > 0 && progress < 1) return progress;
  }
  return 0;
}

/** The scene's touches, each from 0 to 1 as it shows. */
export function musicTouches(elapsed: number): {
  localAdd: number;
  localPick: number;
  youtubeRoot: number;
  youtubeAdd: number;
  youtubePick: number;
  project: number;
  projectPick: number;
} {
  return {
    localAdd: filmTap(elapsed, B.localAddTap),
    localPick: filmTap(elapsed, B.localPickTap),
    youtubeRoot: filmTap(elapsed, B.youtubeTap),
    youtubeAdd: filmTap(elapsed, B.youtubeAddTap),
    youtubePick: filmTap(elapsed, B.youtubePickTap),
    project: filmTap(elapsed, B.projectTap),
    projectPick: filmTap(elapsed, B.projectPickTap),
  };
}

/** How far the project picker shows, from 0 to 1. */
export function musicProjectPicker(elapsed: number): number {
  return filmSpan(elapsed, B.projectTap, B.projectTap + MENU_IN_MS) * (1 - filmSpan(elapsed, B.projectPickTap + 120, B.projectPickTap + 120 + MENU_OUT_MS));
}

/** Whether the event has the Thesis project. */
export function musicProjectChosen(elapsed: number): boolean {
  return elapsed >= B.projectPickTap;
}

/** How far the event's music row at `index` has taken the project's playlist, from 0 to 1. */
export function musicRowFill(elapsed: number, index: number): number {
  const start = B.projectPickTap + ROW_FILL_DELAY_MS + index * ROW_FILL_STAGGER_MS;
  return easeInOutCubic(filmSpan(elapsed, start, start + ROW_FILL_MS));
}

/** The current time, in minutes of the day. */
export function musicClock(elapsed: number): number {
  return monotoneCurve(CLOCK, elapsed);
}

/** The Focus phase the current time is in, or null outside the event's Focus. */
export function musicPhase(elapsed: number): MusicScenePhase | null {
  if (elapsed < B.focusStart) return null;
  const now = musicClock(elapsed);
  const pause = MUSIC_RAIL.breaks.find((span) => now >= span.start && now < span.end);
  if (pause) return pause.long ? "long-break" : "short-break";
  return now >= MUSIC_RAIL.start && now < MUSIC_RAIL.end ? "focus" : null;
}

/** The focus the rail has recorded up to the current time, in minutes of the day, once the event's Focus runs. */
export function musicRailRecorded(elapsed: number): FilmSpan[] {
  if (elapsed < B.focusStart) return [];
  const now = Math.min(musicClock(elapsed), MUSIC_RAIL.end);
  return MUSIC_RAIL.focus.filter((span) => span.start < now).map((span) => ({ start: span.start, end: Math.min(span.end, now) }));
}

/** What the player plays at this moment: the track, the playlist it comes from, and how far it has played and shown. */
export interface MusicNowPlaying {
  source: MusicSceneSource;
  index: number;
  playlist: MusicScenePlaylist;
  /** How far the player has risen into view, from 0 to 1. */
  shown: number;
  /** How far this track has faded in after the playlist switched, from 0 to 1. */
  swap: number;
  /** How far through the track it is, from 0 to 1. */
  progress: number;
}

/** The player at this moment, or null before the event's Focus starts. A fresh track starts at each phase boundary. */
export function musicNowPlaying(elapsed: number): MusicNowPlaying | null {
  if (elapsed < B.focusStart) return null;
  const shown = easeInOutCubic(filmSpan(elapsed, B.focusStart, B.focusStart + PLAYER_IN_MS));
  const [since, source, index, playlist]: [number, MusicSceneSource, number, MusicScenePlaylist] =
    elapsed >= B.breakEnd
      ? [B.breakEnd, "local", 2, "workFocus"]
      : elapsed >= B.breakStart
        ? [B.breakStart, "youtube", 1, "breakCalm"]
        : [B.focusStart, "local", 0, "workFocus"];
  const swap = since === B.focusStart ? 1 : blockerPageIn(elapsed, [since]);
  const progress = Math.min(1, (elapsed - since) * TRACK_PROGRESS_PER_MS);
  return { source, index, playlist, shown, swap, progress };
}
