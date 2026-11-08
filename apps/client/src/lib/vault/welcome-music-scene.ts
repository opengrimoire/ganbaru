/**
 * Timing for the welcome scene on Music. In the playlist builder's review, a local track is added to the Work (focus)
 * playlist and saved, then a YouTube video to Break (calm). Then in the Calendar, a tap on an event opens its panel,
 * where picking the Thesis project gives the event the project's music for each Focus phase, shown as the Music
 * section opens. Once the event is saved, its Focus starts, and the Focus ring's menu shows the work playlist's track,
 * then the break playlist's video while the break runs, then a work track again.
 */

import type { WelcomeEventPanelFrame, WelcomeEventPanelMusicPhase } from "./welcome-event-panel";
import {
  easeInOutCubic,
  FILM_DEFAULT_PRESET,
  FILM_SHEET_RISE_MS,
  filmDayRails,
  filmSpan,
  filmTap,
  monotoneCurve,
  type FilmKeyframe,
  type FilmRail,
  type FilmSpan,
} from "./welcome-film";

/** The built-in playlists the scene shows, by their localized name's key. */
export type MusicScenePlaylist = "workFocus" | "workGanbare" | "breakCalm" | "breakActive";

/** Where a track in the scene comes from. */
export type MusicSceneSource = "local" | "youtube";

/** One built-in playlist with its icon and how many tracks it holds before the scene adds any. */
export interface MusicScenePlaylistEntry {
  key: MusicScenePlaylist;
  icon: string;
  tracks: number;
}

/** The review's playlist cards, as the app names and draws its built-in playlists. */
export const MUSIC_PLAYLISTS: readonly MusicScenePlaylistEntry[] = [
  { key: "workFocus", icon: "lucide:laptop", tracks: 12 },
  { key: "workGanbare", icon: "lucide:coffee", tracks: 8 },
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
  /** Whether it was reviewed before the scene starts. */
  reviewed: boolean;
}

/** The tracks in the Local music folder, in the review tree's order. */
export const MUSIC_LOCAL_TRACKS: readonly MusicSceneTrack[] = [
  { title: "Northbound", artist: "The Low Tides", seconds: 198, artwork: [16, 18], reviewed: true },
  { title: "Paper Lanterns", artist: "Kiso", seconds: 245, artwork: [13, 0], reviewed: true },
  { title: "Quiet Hours", artist: "Lumen Drift", seconds: 222, artwork: [18, 22], reviewed: false },
];

/** The videos in the YouTube folder, in the review tree's order. */
export const MUSIC_YOUTUBE_VIDEOS: readonly MusicSceneTrack[] = [
  { title: "Rain on a cabin window", artist: "Stillwater Sounds", seconds: 3600, artwork: [16, 9], reviewed: true },
  { title: "Soft guitar for a short break", artist: "Night Owl Radio", seconds: 750, artwork: [22, 13], reviewed: false },
  { title: "Morning piano", artist: "Aster Keys", seconds: 1450, artwork: [0, 20], reviewed: false },
];

/** A track in the review: its folder and its row in that folder. */
export interface MusicSceneItem {
  source: MusicSceneSource;
  row: number;
}

/** The Thesis project's playlist for each phase, which the event takes once the project is picked. */
export const MUSIC_PROJECT_PLAYLISTS: readonly { phase: WelcomeEventPanelMusicPhase; playlist: MusicScenePlaylist }[] = [
  { phase: "focus", playlist: "workFocus" },
  { phase: "short-break", playlist: "breakCalm" },
  { phase: "long-break", playlist: "breakActive" },
];

/** The Calendar event, in minutes of the day, and the Focus preset it has before and after its project is picked. */
export const MUSIC_EVENT = { start: 10 * 60, end: 11 * 60 } as const;
export const MUSIC_PROJECT_PRESET = "creative";

/** The event's Focus rail before and after its project is picked, planned by the Calendar's own rhythm. */
export const MUSIC_DEFAULT_RAIL: FilmRail = filmDayRails([{ ...MUSIC_EVENT, preset: FILM_DEFAULT_PRESET }])[0];
export const MUSIC_RAIL: FilmRail = filmDayRails([{ ...MUSIC_EVENT, preset: MUSIC_PROJECT_PRESET }])[0];

/** The event's first break, where the player switches playlists. */
const FIRST_BREAK: FilmSpan = MUSIC_RAIL.breaks[0];

/** Moments of the scene, in milliseconds from its start. A tap's beat is the moment it lands. */
export const MUSIC_BEATS = {
  /** On the local track, a tap checks Work (focus), and a tap on Save and continue saves it and opens the next track. */
  localPickTap: 1600,
  localSaveTap: 2500,
  /** On the YouTube video, a tap checks Break (calm), and Save and continue saves it. */
  youtubePickTap: 3900,
  youtubeSaveTap: 4800,
  /** The builder gives way to the Calendar. */
  builderOut: 5500,
  calendarIn: 5900,
  /** A tap on the event opens its panel. */
  eventTap: 6600,
  /** The project trigger, then the University group, then Thesis, which gives the event the project's music. */
  projectTap: 7500,
  groupTap: 8300,
  projectPick: 9100,
  /** A tap on the Music section's header opens it on the event's playlists. */
  musicTap: 10100,
  /** Save closes the panel. */
  saveTap: 11900,
  /** The event's Focus starts, and a tap on the Focus ring opens its menu with the track that plays. */
  focusStart: 12700,
  ringTap: 13300,
  /** The break starts and the break playlist plays, then the next focus brings back the work playlist. */
  breakStart: 15100,
  breakEnd: 17100,
  end: 18700,
} as const;

const B = MUSIC_BEATS;

/** The frame shown as a still with reduced motion: the ring's menu open on the break's video. */
export const MUSIC_STILL = B.breakStart + 900;

/** The review's two saves, in the order they happen, with the playlist each track joins. */
const REVIEW_STEPS: readonly (MusicSceneItem & { playlist: MusicScenePlaylist; pickTap: number; saveTap: number })[] = [
  { source: "local", row: 2, playlist: "workFocus", pickTap: B.localPickTap, saveTap: B.localSaveTap },
  { source: "youtube", row: 1, playlist: "breakCalm", pickTap: B.youtubePickTap, saveTap: B.youtubeSaveTap },
];

/** The track the review opens on after the scene's two saves. */
const LAST_ITEM: MusicSceneItem = { source: "youtube", row: 2 };

const SCREEN_FADE_MS = 300;
const BUMP_MS = 520;
const ITEM_SWAP_MS = 260;
const SECTION_OPEN_MS = 300;
const SHEET_EXPAND_MS = 320;
const SCROLL_DELAY_MS = 120;
const SCROLL_MS = 420;
/** How far a track plays across the scene, as a fraction of its length per millisecond of scene time. */
const TRACK_PROGRESS_PER_MS = 1 / 9000;

/** The current time against scene time, in minutes of the day: it reaches the event, then crosses its first break. */
const CLOCK: readonly FilmKeyframe[] = [
  [B.calendarIn, MUSIC_EVENT.start - 9],
  [B.saveTap, MUSIC_EVENT.start - 2],
  [B.focusStart, MUSIC_EVENT.start],
  [B.breakStart, FIRST_BREAK.start],
  [B.breakEnd, FIRST_BREAK.end],
  [B.end, FIRST_BREAK.end + 6],
];

/** The tracks of `source`'s folder. */
export function musicTracks(source: MusicSceneSource): readonly MusicSceneTrack[] {
  return source === "local" ? MUSIC_LOCAL_TRACKS : MUSIC_YOUTUBE_VIDEOS;
}

/** How far the playlist builder shows, from 0 to 1. */
export function musicBuilderShown(elapsed: number): number {
  return 1 - easeInOutCubic(filmSpan(elapsed, B.builderOut, B.builderOut + SCREEN_FADE_MS));
}

/** How far the Calendar shows, from 0 to 1. */
export function musicCalendarShown(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.calendarIn - SCREEN_FADE_MS, B.calendarIn));
}

/** The track the review is on. */
export function musicReviewItem(elapsed: number): MusicSceneItem {
  const step = REVIEW_STEPS.find((entry) => elapsed < entry.saveTap);
  return step ? { source: step.source, row: step.row } : LAST_ITEM;
}

/** How far the review's player has faded in on its track after Save and continue, from 0 to 1. */
export function musicReviewItemIn(elapsed: number): number {
  const save = [...REVIEW_STEPS].reverse().find((entry) => elapsed >= entry.saveTap);
  return save ? easeInOutCubic(filmSpan(elapsed, save.saveTap, save.saveTap + ITEM_SWAP_MS)) : 1;
}

/** Whether the track at `row` of `source` has been reviewed. */
export function musicItemReviewed(elapsed: number, source: MusicSceneSource, row: number): boolean {
  if (musicTracks(source)[row]?.reviewed) return true;
  return REVIEW_STEPS.some((step) => step.source === source && step.row === row && elapsed >= step.saveTap);
}

/** How many tracks across both folders have been reviewed, and how many there are. */
export function musicReviewProgress(elapsed: number): { reviewed: number; total: number } {
  const sources: readonly MusicSceneSource[] = ["local", "youtube"];
  const rows = sources.flatMap((source) => musicTracks(source).map((_, row) => musicItemReviewed(elapsed, source, row)));
  return { reviewed: rows.filter(Boolean).length, total: rows.length };
}

/** Whether `playlist`'s card is checked for the open track, so Save and continue has something to save. */
export function musicPlaylistChecked(elapsed: number, playlist: MusicScenePlaylist): boolean {
  return REVIEW_STEPS.some((step) => step.playlist === playlist && elapsed >= step.pickTap && elapsed < step.saveTap);
}

/** How many tracks `playlist` holds at this moment. */
export function musicPlaylistCount(elapsed: number, playlist: MusicScenePlaylist): number {
  const base = MUSIC_PLAYLISTS.find((entry) => entry.key === playlist)?.tracks ?? 0;
  return base + REVIEW_STEPS.filter((step) => step.playlist === playlist && elapsed >= step.saveTap).length;
}

/** How far `playlist`'s count has bumped after a track is saved to it, from 0 to 1; 0 when it is not bumping. */
export function musicPlaylistBump(elapsed: number, playlist: MusicScenePlaylist): number {
  for (const step of REVIEW_STEPS) {
    if (step.playlist !== playlist) continue;
    const progress = filmSpan(elapsed, step.saveTap, step.saveTap + BUMP_MS);
    if (progress > 0 && progress < 1) return progress;
  }
  return 0;
}

/** The builder's touches, each from 0 to 1 as it shows. */
export function musicBuilderTouches(elapsed: number): { localPick: number; localSave: number; youtubePick: number; youtubeSave: number } {
  return {
    localPick: filmTap(elapsed, B.localPickTap),
    localSave: filmTap(elapsed, B.localSaveTap),
    youtubePick: filmTap(elapsed, B.youtubePickTap),
    youtubeSave: filmTap(elapsed, B.youtubeSaveTap),
  };
}

/** The touch on the event that opens its panel, from 0 to 1. */
export function musicEventTouch(elapsed: number): number {
  return filmTap(elapsed, B.eventTap);
}

/** Whether the event panel is open. */
export function musicPanelOpen(elapsed: number): boolean {
  return elapsed >= B.eventTap && elapsed < B.saveTap;
}

/** Whether the event has the Thesis project, with its color, Focus rhythm, and music. */
export function musicProjectPicked(elapsed: number): boolean {
  return elapsed >= B.projectPick;
}

/** The project picker's open pane: the groups after the project trigger is tapped, then the group's projects. */
function projectPane(elapsed: number): "groups" | "projects" | null {
  if (elapsed >= B.groupTap && elapsed < B.projectPick) return "projects";
  if (elapsed >= B.projectTap && elapsed < B.groupTap) return "groups";
  return null;
}

/** How far the Music section has opened, from 0 to 1. */
export function musicSectionOpen(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, B.musicTap, B.musicTap + SECTION_OPEN_MS));
}

/** The event panel at `elapsed`, with the event's `title` and `playlist`'s icon and localized name for each phase. */
export function musicEventPanelFrame(
  elapsed: number,
  title: string,
  playlist: (key: MusicScenePlaylist) => { icon: string; name: string },
): WelcomeEventPanelFrame {
  const picked = musicProjectPicked(elapsed);
  const opened = filmSpan(elapsed, B.musicTap + SCROLL_DELAY_MS, B.musicTap + SCROLL_DELAY_MS + SCROLL_MS);
  return {
    sheetRise: 1 - (1 - filmSpan(elapsed, B.eventTap, B.eventTap + FILM_SHEET_RISE_MS)) ** 3,
    sheetExpand: easeInOutCubic(filmSpan(elapsed, B.musicTap, B.musicTap + SHEET_EXPAND_MS)),
    pane: projectPane(elapsed),
    picked,
    typed: title,
    typing: false,
    caretOn: false,
    touches: {
      project: filmTap(elapsed, B.projectTap),
      group: filmTap(elapsed, B.groupTap),
      projectPick: filmTap(elapsed, B.projectPick),
      music: filmTap(elapsed, B.musicTap),
      save: filmTap(elapsed, B.saveTap),
    },
    preset: picked ? MUSIC_PROJECT_PRESET : FILM_DEFAULT_PRESET,
    music: {
      enabled: picked,
      expanded: musicSectionOpen(elapsed),
      rows: MUSIC_PROJECT_PLAYLISTS.map(({ phase, playlist: key }) => {
        const { icon, name } = playlist(key);
        return { phase, playlistIcon: icon, playlistName: name, fill: picked ? 1 : 0 };
      }),
    },
    scroll: easeInOutCubic(opened),
  };
}

/** The event's rail at this moment: its default rhythm until the project is picked, then the project's. */
export function musicEventRail(elapsed: number): FilmRail {
  return musicProjectPicked(elapsed) ? MUSIC_RAIL : MUSIC_DEFAULT_RAIL;
}

/** The current time, in minutes of the day. */
export function musicClock(elapsed: number): number {
  return monotoneCurve(CLOCK, elapsed);
}

/** The Focus phase the current time is in, or null outside the event's Focus. */
export function musicPhase(elapsed: number): WelcomeEventPanelMusicPhase | null {
  if (elapsed < B.focusStart) return null;
  const now = musicClock(elapsed);
  const pause = MUSIC_RAIL.breaks.find((span) => now >= span.start && now < span.end);
  if (pause) return pause.long ? "long-break" : "short-break";
  return now >= MUSIC_RAIL.start && now < MUSIC_RAIL.end ? "focus" : null;
}

/** The current phase's remaining and total seconds, as the Focus ring counts them; zeros outside the event's Focus. */
export function musicPhaseSeconds(elapsed: number): { remaining: number; total: number } {
  const phase = musicPhase(elapsed);
  if (!phase) return { remaining: 0, total: 0 };
  const now = musicClock(elapsed);
  const spans = phase === "focus" ? MUSIC_RAIL.focus : MUSIC_RAIL.breaks;
  const span = spans.find((entry) => now >= entry.start && now < entry.end);
  if (!span) return { remaining: 0, total: 0 };
  return { remaining: Math.max(0, Math.round((span.end - now) * 60)), total: Math.round((span.end - span.start) * 60) };
}

/** The focus the rail has recorded up to the current time, in minutes of the day, once the event's Focus runs. */
export function musicRailRecorded(elapsed: number): FilmSpan[] {
  if (elapsed < B.focusStart) return [];
  const now = Math.min(musicClock(elapsed), MUSIC_RAIL.end);
  return MUSIC_RAIL.focus.filter((span) => span.start < now).map((span) => ({ start: span.start, end: Math.min(span.end, now) }));
}

/** Whether the Focus ring's menu is open. */
export function musicRingMenuOpen(elapsed: number): boolean {
  return elapsed >= B.ringTap;
}

/** The touch on the Focus ring, from 0 to 1. */
export function musicRingTouch(elapsed: number): number {
  return filmTap(elapsed, B.ringTap);
}

/** What plays at this moment: the track, the playlist it comes from, and how far it has played. */
export interface MusicNowPlaying extends MusicSceneItem {
  playlist: MusicScenePlaylist;
  /** How far through the track it is, from 0 to 1. */
  progress: number;
}

/** The track that plays at this moment, or null before the event's Focus starts. A fresh track starts at each phase. */
export function musicNowPlaying(elapsed: number): MusicNowPlaying | null {
  if (elapsed < B.focusStart) return null;
  const [since, source, row, playlist]: [number, MusicSceneSource, number, MusicScenePlaylist] =
    elapsed >= B.breakEnd
      ? [B.breakEnd, "local", 0, "workFocus"]
      : elapsed >= B.breakStart
        ? [B.breakStart, "youtube", 1, "breakCalm"]
        : [B.focusStart, "local", 2, "workFocus"];
  return { source, row, playlist, progress: Math.min(1, (elapsed - since) * TRACK_PROGRESS_PER_MS) };
}

/** How far the review's open track has played, from 0 to 1, as its autoplay runs from the moment it opens. */
export function musicReviewProgressOfTrack(elapsed: number): number {
  const save = [...REVIEW_STEPS].reverse().find((entry) => elapsed >= entry.saveTap);
  return Math.min(1, (elapsed - (save?.saveTap ?? 0)) * TRACK_PROGRESS_PER_MS);
}
