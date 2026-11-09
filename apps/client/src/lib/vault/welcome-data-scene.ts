/**
 * Timing for the welcome scene on where data lives: the vault's folder opens empty, and its entries appear one after
 * another, each lit briefly as it arrives. Every moment is derived from the milliseconds elapsed since the scene
 * started, so a frame is a pure function of time and the reduced-motion still is simply the last frame.
 */

import { easeInOutCubic, filmSpan } from "./welcome-film";

/** What an entry of the vault holds, which the scene describes beside its name. */
export type DataEntryDetail = "assets" | "projects" | "database" | "marker";

/** One top-level entry of a new vault. */
export interface DataEntry {
  name: string;
  folder: boolean;
  detail: DataEntryDetail;
}

/** The entries a new vault shows, folders first as file managers sort them. */
export const DATA_ENTRIES: readonly DataEntry[] = [
  { name: "assets", folder: true, detail: "assets" },
  { name: "projects", folder: true, detail: "projects" },
  { name: "ganbaru.sqlite", folder: false, detail: "database" },
  { name: "vault.json", folder: false, detail: "marker" },
];

/** The folder's own name settles into the path first. */
const FOLDER_IN_MS = 420;
/** Each entry slides in after the one before it, and stays lit for a moment once it has arrived. */
const FIRST_ENTRY = 650;
const ENTRY_GAP = 420;
const ENTRY_IN_MS = 360;
const ENTRY_LIT_MS = 900;
const END_REST_MS = 300;

/** When the entry at `index` starts to appear, in milliseconds from the scene's start. */
export function dataEntryStart(index: number): number {
  return FIRST_ENTRY + index * ENTRY_GAP;
}

const LAST_ENTRY_SETTLED = dataEntryStart(DATA_ENTRIES.length - 1) + ENTRY_IN_MS + ENTRY_LIT_MS;

/** Moments of the scene, in milliseconds from its start. */
export const DATA_BEATS = {
  folderIn: FOLDER_IN_MS,
  firstEntry: FIRST_ENTRY,
  end: LAST_ENTRY_SETTLED + END_REST_MS,
} as const;

/** How far the folder's own name has settled into the path, from 0 to 1. */
export function dataFolderReveal(elapsed: number): number {
  return easeInOutCubic(filmSpan(elapsed, 0, FOLDER_IN_MS));
}

/** How far the entry at `index` has appeared, from 0 to 1. */
export function dataEntryReveal(elapsed: number, index: number): number {
  const start = dataEntryStart(index);
  return easeInOutCubic(filmSpan(elapsed, start, start + ENTRY_IN_MS));
}

/** How lit the entry at `index` is, from 0 to 1: it lights as it arrives and fades once it has settled. */
export function dataEntryLit(elapsed: number, index: number): number {
  const start = dataEntryStart(index);
  const arrived = start + ENTRY_IN_MS;
  if (elapsed < arrived) return filmSpan(elapsed, start, arrived);
  return 1 - easeInOutCubic(filmSpan(elapsed, arrived, arrived + ENTRY_LIT_MS));
}

/**
 * The last `depth` folders of `path`, outermost first, for a path bar. Both slash styles separate folders, so Windows
 * paths split as well as Unix ones; a drive or root on its own is kept only when the path has nothing else.
 */
export function vaultPathTrail(path: string, depth = 2): string[] {
  const parts = path.split(/[\\/]+/).filter((part) => part.length > 0);
  return parts.slice(Math.max(0, parts.length - Math.max(1, Math.trunc(depth))));
}
