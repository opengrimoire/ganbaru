import type { ProjectWorkingFolderFileEntry } from "$lib/chat/contracts";

export interface ChatFileTreeRow {
  entry: ProjectWorkingFolderFileEntry;
  depth: number;
  expanded: boolean;
}

export interface ChatVirtualRange {
  start: number;
  end: number;
  offset: number;
  totalSize: number;
}

/**
 * Flattens only expanded, already loaded workspace directories.
 *
 * @param rootEntries Entries at the workspace root.
 * @param childrenByDirectory Loaded child entries keyed by relative directory path.
 * @param expandedPaths Expanded directory paths.
 * @returns Visible rows in stable tree order.
 */
export function flattenChatFileTree(
  rootEntries: readonly ProjectWorkingFolderFileEntry[],
  childrenByDirectory: Readonly<Record<string, readonly ProjectWorkingFolderFileEntry[]>>,
  expandedPaths: readonly string[],
): ChatFileTreeRow[] {
  const expandedPathSet = new Set(expandedPaths);
  const rows: ChatFileTreeRow[] = [];

  function append(entries: readonly ProjectWorkingFolderFileEntry[], depth: number): void {
    for (const entry of entries) {
      const isExpanded = entry.kind === "directory" && expandedPathSet.has(entry.relativePath);
      rows.push({ entry, depth, expanded: isExpanded });
      if (isExpanded) append(childrenByDirectory[entry.relativePath] ?? [], depth + 1);
    }
  }

  append(rootEntries, 0);
  return rows;
}

/**
 * Calculates a bounded fixed-row render window for large file trees and code files.
 *
 * @param itemCount Total row count.
 * @param scrollOffset Current scroll offset in pixels.
 * @param viewportSize Visible viewport size in pixels.
 * @param itemSize Fixed row size in pixels.
 * @param overscan Extra rows rendered above and below the viewport.
 * @returns Slice boundaries and spacer measurements.
 */
export function chatVirtualRange(
  itemCount: number,
  scrollOffset: number,
  viewportSize: number,
  itemSize: number,
  overscan = 8,
): ChatVirtualRange {
  const count = Math.max(0, Math.floor(itemCount));
  const size = Math.max(1, itemSize);
  const visibleCount = Math.max(1, Math.ceil(Math.max(size, viewportSize) / size));
  // A stale offset past the content, such as after rows shrink, keeps the final rows rendered.
  const lastStart = Math.max(0, count - visibleCount - overscan);
  const start = Math.min(
    lastStart,
    Math.max(0, Math.floor(Math.max(0, scrollOffset) / size) - overscan),
  );
  const end = Math.min(count, start + visibleCount + overscan * 2);
  return {
    start,
    end,
    offset: start * size,
    totalSize: count * size,
  };
}
