import { readdirSync, statSync } from "node:fs";
import path from "node:path";

/**
 * Module IDs in bundle metadata are normalized relative to the client directory.
 * Only application source entries can be checked against disk; dependency entries
 * such as `/node_modules/react/` may name packages that are intentionally absent.
 */
const SOURCE_PREFIX = "src/";

/**
 * @typedef {object} BaselinePathFileSystem
 * @property {(relativePath: string) => boolean} isFile Whether a client-relative path is a file.
 * @property {(relativePath: string) => boolean} isDirectory Whether a client-relative path is a directory.
 * @property {(relativePath: string) => readonly string[]} listDirectory Entry names of a client-relative directory, or an empty list.
 */

/**
 * @typedef {object} BaselinePathEntry
 * @property {string} label Baseline field the entry came from, used in failure messages.
 * @property {string} value Module ID or module ID pattern from the baseline.
 * @property {"exact" | "pattern"} kind Exact module IDs must be files; patterns must match a path prefix.
 */

/**
 * Find baseline entries that point at application source paths that no longer exist.
 *
 * Exact entries must name an existing file. Pattern entries (substrings and prefixes)
 * ending in `/` must name an existing directory; other pattern entries must be a prefix
 * of at least one existing entry in their parent directory. Entries outside `src/` are
 * skipped because they describe dependency packages rather than repository files.
 *
 * @param {readonly BaselinePathEntry[]} entries Baseline entries to check.
 * @param {BaselinePathFileSystem} fileSystem Client-relative filesystem queries.
 * @returns {string[]} One failure message per stale entry.
 */
export function findStaleBaselinePaths(entries, fileSystem) {
  const failures = [];
  for (const { label, value, kind } of entries) {
    if (!value.startsWith(SOURCE_PREFIX)) continue;
    if (kind === "exact") {
      if (!fileSystem.isFile(value)) failures.push(`${label} names a missing file: ${value}`);
    } else if (value.endsWith("/")) {
      if (!fileSystem.isDirectory(value)) failures.push(`${label} names a missing directory: ${value}`);
    } else {
      const parent = path.posix.dirname(value);
      const namePrefix = path.posix.basename(value);
      if (!fileSystem.listDirectory(parent).some((name) => name.startsWith(namePrefix))) {
        failures.push(`${label} matches no existing path: ${value}`);
      }
    }
  }
  return failures;
}

/**
 * Build entries for one baseline field.
 *
 * @param {string} label Baseline field name.
 * @param {readonly string[]} values Entries of that field.
 * @param {"exact" | "pattern"} kind How the entries are matched against module IDs.
 * @returns {BaselinePathEntry[]} Entries ready for {@link findStaleBaselinePaths}.
 */
export function baselineEntries(label, values, kind) {
  return values.map((value) => ({ label, value, kind }));
}

/**
 * Create filesystem queries rooted at the client directory.
 *
 * @param {string} clientDir Absolute client directory.
 * @returns {BaselinePathFileSystem} Filesystem queries for {@link findStaleBaselinePaths}.
 */
export function clientFileSystem(clientDir) {
  const stat = (relativePath) => statSync(path.join(clientDir, relativePath), { throwIfNoEntry: false });
  return {
    isFile: (relativePath) => stat(relativePath)?.isFile() === true,
    isDirectory: (relativePath) => stat(relativePath)?.isDirectory() === true,
    listDirectory: (relativePath) => (
      stat(relativePath)?.isDirectory() === true ? readdirSync(path.join(clientDir, relativePath)) : []
    ),
  };
}

/**
 * Throw a single error listing every stale baseline entry.
 *
 * @param {string} baselineName Human-readable baseline name for the error.
 * @param {readonly BaselinePathEntry[]} entries Baseline entries to check.
 * @param {BaselinePathFileSystem} fileSystem Client-relative filesystem queries.
 * @returns {void}
 */
export function assertBaselinePathsExist(baselineName, entries, fileSystem) {
  const failures = findStaleBaselinePaths(entries, fileSystem);
  if (failures.length > 0) {
    throw new Error(
      `${baselineName} baseline references paths that do not exist; update or remove them:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
  }
}
