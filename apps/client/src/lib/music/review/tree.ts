import type { MusicItemListEntry } from "$lib/music/library/contracts";

export interface MusicReviewTreeNode {
  id: string;
  name: string;
  path: string;
  itemIds: string[];
  directItems: MusicItemListEntry[];
  children: MusicReviewTreeNode[];
}

export type MusicReviewTreeRow =
  | { kind: "folder"; depth: number; node: MusicReviewTreeNode }
  | { kind: "item"; depth: number; item: MusicItemListEntry };

export interface MusicReviewTreeSearchResult {
  rows: MusicReviewTreeRow[];
  matchCount: number;
}

export interface MusicReviewSelectionSummary {
  itemCount: number;
  contextLabels: string[];
  hiddenContextCount: number;
}

interface MutableNode {
  id: string;
  name: string;
  path: string;
  directItems: MusicItemListEntry[];
  children: Map<string, MutableNode>;
}

function folder(id: string, name: string, path: string): MutableNode {
  return { id, name, path, directItems: [], children: new Map() };
}

function normalizedSegments(relativePath: string | null | undefined): string[] {
  if (!relativePath) return [];
  return relativePath.replaceAll("\\", "/").split("/").map((part) => part.trim()).filter(Boolean);
}

function freezeNode(node: MutableNode): MusicReviewTreeNode {
  const children = [...node.children.values()]
    .sort((left, right) => left.name.localeCompare(right.name, undefined, { sensitivity: "base" }))
    .map(freezeNode);
  const directItems = [...node.directItems].sort((left, right) => left.title.localeCompare(right.title, undefined, { sensitivity: "base" }));
  return {
    id: node.id,
    name: node.name,
    path: node.path,
    directItems,
    children,
    itemIds: [...directItems.map((item) => item.id), ...children.flatMap((child) => child.itemIds)],
  };
}

/** Projects review items into stable local-folder and online-source trees. */
export function buildMusicReviewTree(items: readonly MusicItemListEntry[]): MusicReviewTreeNode[] {
  const local = folder("review-root:local", "Music", "");
  const online = folder("review-root:online", "Online", "online");
  for (const item of items) {
    if (item.sourceKind === "youtube-video") {
      online.directItems.push(item);
      continue;
    }
    const segments = normalizedSegments(item.relativePath);
    segments.pop();
    let parent = local;
    let path = "";
    for (const segment of segments) {
      path = path ? `${path}/${segment}` : segment;
      let child = parent.children.get(segment);
      if (!child) {
        child = folder(`review-folder:${path}`, segment, path);
        parent.children.set(segment, child);
      }
      parent = child;
    }
    parent.directItems.push(item);
  }
  return [local, online].filter((node) => node.directItems.length > 0 || node.children.size > 0).map(freezeNode);
}

/** Returns track ids in the same folder-first, title-sorted order shown by the review tree. */
export function musicReviewTreeItemIds(items: readonly MusicItemListEntry[]): string[] {
  return buildMusicReviewTree(items).flatMap((node) => node.itemIds);
}

/** Selects the first pending track in visible tree order, falling back to the first reviewed track. */
export function firstMusicReviewTreeItemId(items: readonly MusicItemListEntry[]): string | null {
  const orderedIds = musicReviewTreeItemIds(items);
  const itemsById = new Map(items.map((item) => [item.id, item]));
  return orderedIds.find((itemId) => itemsById.get(itemId)?.reviewState !== "reviewed")
    ?? orderedIds[0]
    ?? null;
}

/** Counts unique canonical items that still require review. */
export function musicUnreviewedItemCount(items: readonly MusicItemListEntry[]): number {
  return new Set(items.filter((item) => item.reviewState === "unreviewed").map((item) => item.id)).size;
}

/** Finds the next unreviewed track in tree order, wrapping once and honoring session skips. */
export function nextPendingMusicReviewTreeItemId(
  items: readonly MusicItemListEntry[],
  currentItemId: string,
  skippedItemIds: ReadonlySet<string>,
): string | null {
  const orderedIds = musicReviewTreeItemIds(items);
  const currentIndex = orderedIds.indexOf(currentItemId);
  const itemsById = new Map(items.map((item) => [item.id, item]));
  for (let offset = 1; offset <= orderedIds.length; offset += 1) {
    const index = currentIndex >= 0 ? (currentIndex + offset) % orderedIds.length : offset - 1;
    const itemId = orderedIds[index];
    if (!itemId || skippedItemIds.has(itemId)) continue;
    if (itemsById.get(itemId)?.reviewState !== "reviewed") return itemId;
  }
  return null;
}

/** Finds the next pending track after a grouped Review selection. */
export function nextPendingMusicReviewSelectionItemId(
  items: readonly MusicItemListEntry[],
  selectedItemIds: ReadonlySet<string>,
): string | null {
  const orderedIds = musicReviewTreeItemIds(items);
  let lastSelectedIndex = -1;
  for (const itemId of selectedItemIds) {
    lastSelectedIndex = Math.max(lastSelectedIndex, orderedIds.indexOf(itemId));
  }
  const anchorId = orderedIds[lastSelectedIndex] ?? "";
  return nextPendingMusicReviewTreeItemId(items, anchorId, selectedItemIds);
}

/** Summarizes a tree selection without repeating descendant folder names. */
export function summarizeMusicReviewSelection(
  items: readonly MusicItemListEntry[],
  selectedItemIds: ReadonlySet<string>,
  selectedFolderIds: ReadonlySet<string>,
  contextLimit = 2,
): MusicReviewSelectionSummary {
  return summarizeMusicReviewTreeSelection(
    buildMusicReviewTree(items),
    items,
    selectedItemIds,
    selectedFolderIds,
    contextLimit,
  );
}

/** Summarizes a selection from an existing Review tree projection. */
export function summarizeMusicReviewTreeSelection(
  tree: readonly MusicReviewTreeNode[],
  items: readonly MusicItemListEntry[],
  selectedItemIds: ReadonlySet<string>,
  selectedFolderIds: ReadonlySet<string>,
  contextLimit = 2,
): MusicReviewSelectionSummary {
  const selectedItems = items.filter((item) => selectedItemIds.has(item.id));
  const coveredItemIds = new Set<string>();
  const folderLabels: string[] = [];

  const collectFolders = (nodes: readonly MusicReviewTreeNode[], ancestorSelected: boolean): void => {
    for (const node of nodes) {
      const selected = selectedFolderIds.has(node.id);
      if (selected && !ancestorSelected) {
        folderLabels.push(node.name);
        for (const itemId of node.itemIds) coveredItemIds.add(itemId);
      }
      collectFolders(node.children, ancestorSelected || selected);
    }
  };
  collectFolders(tree, false);

  const itemsById = new Map(items.map((item) => [item.id, item]));
  const standaloneTrackLabels = tree.flatMap((node) => node.itemIds)
    .filter((itemId) => selectedItemIds.has(itemId) && !coveredItemIds.has(itemId))
    .flatMap((itemId) => {
      const item = itemsById.get(itemId);
      return item ? [item.title] : [];
    });
  const labels = [...folderLabels, ...standaloneTrackLabels];
  const limit = Math.max(0, contextLimit);
  return {
    itemCount: selectedItems.length,
    contextLabels: labels.slice(0, limit),
    hiddenContextCount: Math.max(0, labels.length - limit),
  };
}

/** Returns every folder id so the initial Review tree can open completely. */
export function musicReviewTreeFolderIds(
  nodes: readonly MusicReviewTreeNode[],
): Set<string> {
  const ids = new Set<string>();
  const visit = (entries: readonly MusicReviewTreeNode[]): void => {
    for (const node of entries) {
      ids.add(node.id);
      visit(node.children);
    }
  };
  visit(nodes);
  return ids;
}

/** Returns the root-to-leaf folder path containing one review item. */
export function musicReviewTreeAncestorFolderIds(
  nodes: readonly MusicReviewTreeNode[],
  itemId: string,
): string[] {
  for (const node of nodes) {
    if (!node.itemIds.includes(itemId)) continue;
    const childPath = musicReviewTreeAncestorFolderIds(node.children, itemId);
    return [node.id, ...childPath];
  }
  return [];
}

/** Centers an active row only when it is outside the visible tree viewport. */
export function musicReviewTreeRevealScrollTop(
  currentScrollTop: number,
  viewportHeight: number,
  rowTop: number,
  rowHeight: number,
): number | null {
  if (rowTop >= 0 && rowTop + rowHeight <= viewportHeight) return null;
  return Math.max(0, currentScrollTop + rowTop - (viewportHeight - rowHeight) / 2);
}

/** Flattens expanded tree nodes into keyboard-friendly visual rows. */
export function flattenMusicReviewTree(
  nodes: readonly MusicReviewTreeNode[],
  expandedIds: ReadonlySet<string>,
  depth = 0,
): MusicReviewTreeRow[] {
  const rows: MusicReviewTreeRow[] = [];
  for (const node of nodes) {
    rows.push({ kind: "folder", depth, node });
    if (!expandedIds.has(node.id)) continue;
    rows.push(...node.directItems.map((item) => ({ kind: "item" as const, depth: depth + 1, item })));
    rows.push(...flattenMusicReviewTree(node.children, expandedIds, depth + 1));
  }
  return rows;
}

function reviewTreeItemMatches(item: MusicItemListEntry, query: string): boolean {
  return [item.title, item.artist, item.album, item.relativePath ?? ""]
    .some((value) => value.toLocaleLowerCase().includes(query));
}

function reviewTreeMatchCount(node: MusicReviewTreeNode, query: string): number {
  return Number(node.name.toLocaleLowerCase().includes(query))
    + node.directItems.filter((item) => reviewTreeItemMatches(item, query)).length
    + node.children.reduce((total, child) => total + reviewTreeMatchCount(child, query), 0);
}

function searchReviewTreeNode(
  node: MusicReviewTreeNode,
  query: string,
  depth: number,
): MusicReviewTreeSearchResult {
  if (node.name.toLocaleLowerCase().includes(query)) {
    return {
      rows: flattenMusicReviewTree([node], musicReviewTreeFolderIds([node]), depth),
      matchCount: reviewTreeMatchCount(node, query),
    };
  }

  const matchingItems = node.directItems.filter((item) => reviewTreeItemMatches(item, query));
  const childResults = node.children.map((child) => searchReviewTreeNode(child, query, depth + 1));
  const matchingChildren = childResults.filter((result) => result.rows.length > 0);
  if (matchingItems.length === 0 && matchingChildren.length === 0) return { rows: [], matchCount: 0 };
  return {
    rows: [
      { kind: "folder", depth, node },
      ...matchingItems.map((item) => ({ kind: "item" as const, depth: depth + 1, item })),
      ...matchingChildren.flatMap((result) => result.rows),
    ],
    matchCount: matchingItems.length
      + matchingChildren.reduce((total, result) => total + result.matchCount, 0),
  };
}

/** Searches folders and track metadata while preserving ancestor rows as context. */
export function searchMusicReviewTree(
  nodes: readonly MusicReviewTreeNode[],
  search: string,
): MusicReviewTreeSearchResult {
  const query = search.trim().toLocaleLowerCase();
  if (!query) return { rows: [], matchCount: 0 };
  const results = nodes.map((node) => searchReviewTreeNode(node, query, 0));
  return {
    rows: results.flatMap((result) => result.rows),
    matchCount: results.reduce((total, result) => total + result.matchCount, 0),
  };
}

/** Toggles every descendant of a folder while preserving unrelated selections. */
export function toggleMusicReviewTreeSelection(
  selectedIds: ReadonlySet<string>,
  itemIds: readonly string[],
): Set<string> {
  const next = new Set(selectedIds);
  const allSelected = itemIds.length > 0 && itemIds.every((itemId) => next.has(itemId));
  for (const itemId of itemIds) {
    if (allSelected) next.delete(itemId);
    else next.add(itemId);
  }
  return next;
}
