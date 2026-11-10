/**
 * What the illustration on the welcome screen about where data lives shows: the default folder's place in a path bar,
 * and the entries a new vault holds.
 */

/** What kind of entry the illustration draws, which picks its icon. */
export type DataEntryKind = "folder" | "database" | "file";

/** One top-level entry of a new vault. */
export interface DataEntry {
  name: string;
  kind: DataEntryKind;
}

/** The entries a new vault shows, folders first as file managers sort them. */
export const DATA_ENTRIES: readonly DataEntry[] = [
  { name: "assets", kind: "folder" },
  { name: "projects", kind: "folder" },
  { name: "ganbaru.sqlite", kind: "database" },
  { name: "vault.json", kind: "file" },
];

/**
 * The last `depth` folders of `path`, outermost first, for a path bar. Both slash styles separate folders, so Windows
 * paths split as well as Unix ones; a drive or root on its own is kept only when the path has nothing else.
 */
export function vaultPathTrail(path: string, depth = 2): string[] {
  const parts = path.split(/[\\/]+/).filter((part) => part.length > 0);
  return parts.slice(Math.max(0, parts.length - Math.max(1, Math.trunc(depth))));
}
