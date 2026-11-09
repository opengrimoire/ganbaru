/**
 * What the welcome screen on where data lives shows: the default folder, split into its name and where it sits, and
 * the entries a new vault holds.
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

/** A folder path split into the folder's own name and the path of the folder that holds it. */
export interface VaultPathParts {
  name: string;
  /** The holding folder's path, or `null` when the path names no parent. */
  parent: string | null;
}

/**
 * Splits `path` into the folder's name and its parent's path. Both slash styles separate folders, so Windows paths split
 * as well as Unix ones, and trailing separators are ignored. A parent that is a root or a drive keeps its separator, so
 * it still reads as one.
 */
export function splitVaultPath(path: string): VaultPathParts {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  if (cut < 0) return { name: trimmed || path, parent: null };
  const name = trimmed.slice(cut + 1);
  const separator = trimmed[cut];
  const head = trimmed.slice(0, cut).replace(/[\\/]+$/, "");
  const parent = head === "" || head.endsWith(":") ? `${head}${separator}` : head;
  return { name, parent };
}
