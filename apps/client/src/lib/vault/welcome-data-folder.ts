/**
 * What the illustration on the welcome screen about where data lives shows: the default folder's place in a path bar,
 * and the entries a new vault holds.
 */

/** What an entry of the vault holds, which the illustration describes beside its name. */
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

/**
 * The last `depth` folders of `path`, outermost first, for a path bar. Both slash styles separate folders, so Windows
 * paths split as well as Unix ones; a drive or root on its own is kept only when the path has nothing else.
 */
export function vaultPathTrail(path: string, depth = 2): string[] {
  const parts = path.split(/[\\/]+/).filter((part) => part.length > 0);
  return parts.slice(Math.max(0, parts.length - Math.max(1, Math.trunc(depth))));
}
