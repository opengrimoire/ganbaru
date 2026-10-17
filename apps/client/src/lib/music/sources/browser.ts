import type {
  MusicItemListEntry,
  MusicSourceCollection,
  MusicSourceSummary,
} from "$lib/music/library/contracts";

export const LOCAL_MUSIC_SOURCE_ID = "source-group:local";
export const YOUTUBE_MUSIC_SOURCE_ID = "source-group:youtube";
export const SAVED_YOUTUBE_VIDEOS_ID = "source-collection:youtube-saved-videos";

export type MusicSourceBrowserKind = "group" | "collection" | "folder";
export type MusicSourceBrowserType = "local" | "youtube";

export interface MusicSourceBrowserNode {
  id: string;
  kind: MusicSourceBrowserKind;
  sourceType: MusicSourceBrowserType;
  name: string;
  collectionId: string | null;
  rootId: string | null;
  relativePath: string;
  directItems: MusicItemListEntry[];
  itemIds: string[];
  children: MusicSourceBrowserNode[];
  issueCount: number;
}

export interface MusicSourceBrowserNames {
  local: string;
  youtube: string;
  savedVideos: string;
  unlinkedFiles: string;
}

interface MutableNode {
  id: string;
  kind: MusicSourceBrowserKind;
  sourceType: MusicSourceBrowserType;
  name: string;
  collectionId: string | null;
  rootId: string | null;
  relativePath: string;
  directItems: MusicItemListEntry[];
  children: Map<string, MutableNode>;
  issueCount: number;
}

function mutableNode(
  id: string,
  kind: MusicSourceBrowserKind,
  sourceType: MusicSourceBrowserType,
  name: string,
  collectionId: string | null = null,
  rootId: string | null = null,
  relativePath = "",
): MutableNode {
  return {
    id,
    kind,
    sourceType,
    name,
    collectionId,
    rootId,
    relativePath,
    directItems: [],
    children: new Map(),
    issueCount: 0,
  };
}

function normalizedSegments(relativePath: string | null): string[] {
  if (!relativePath) return [];
  return relativePath.replaceAll("\\", "/").split("/").map((part) => part.trim()).filter(Boolean);
}

function collectItemIds(node: MutableNode): string[] {
  return [...new Set([
    ...node.directItems.map((item) => item.id),
    ...[...node.children.values()].flatMap(collectItemIds),
  ])];
}

function freezeTree(node: MutableNode): MusicSourceBrowserNode {
  const children = [...node.children.values()]
    .sort((left, right) => left.name.localeCompare(right.name, undefined, { sensitivity: "base" }))
    .map(freezeTree);
  const directItems = node.directItems.toSorted((left, right) =>
    left.title.localeCompare(right.title, undefined, { sensitivity: "base" }));
  return { ...node, children, directItems, itemIds: collectItemIds(node) };
}

function addLocalFolders(collectionNode: MutableNode, items: readonly MusicItemListEntry[]): void {
  for (const item of items) {
    const segments = normalizedSegments(item.relativePath);
    segments.pop();
    let parent = collectionNode;
    let path = "";
    for (const segment of segments) {
      path = path ? `${path}/${segment}` : segment;
      let child = parent.children.get(segment);
      if (!child) {
        child = mutableNode(
          `source-folder:${collectionNode.rootId ?? "unknown"}:${path}`,
          "folder",
          "local",
          segment,
          collectionNode.collectionId,
          collectionNode.rootId,
          path,
        );
        parent.children.set(segment, child);
      }
      parent = child;
    }
    parent.directItems.push(item);
  }
}

function summaryIssues(summaryById: ReadonlyMap<string, MusicSourceSummary>, collectionId: string): number {
  return summaryById.get(collectionId)?.openIssueCount ?? 0;
}

/** Projects canonical library items into the two stable Sources roots. */
export function buildMusicSourceBrowser(
  items: readonly MusicItemListEntry[],
  collections: readonly MusicSourceCollection[],
  summaries: readonly MusicSourceSummary[],
  names: MusicSourceBrowserNames,
): MusicSourceBrowserNode[] {
  const summaryById = new Map(summaries.map((summary) => [summary.id, summary]));
  const localRoot = mutableNode(LOCAL_MUSIC_SOURCE_ID, "group", "local", names.local);
  const youtubeRoot = mutableNode(YOUTUBE_MUSIC_SOURCE_ID, "group", "youtube", names.youtube);
  const localItems = items.filter((item) => item.sourceKind === "local-file");
  const youtubeItems = items.filter((item) => item.sourceKind === "youtube-video");

  for (const collection of collections.filter((entry) => entry.kind === "local-root")) {
    const rootId = collection.localRootId;
    if (!rootId) continue;
    const node = mutableNode(
      `source-collection:${collection.id}`,
      "collection",
      "local",
      collection.name,
      collection.id,
      rootId,
    );
    node.issueCount = summaryIssues(summaryById, collection.id);
    addLocalFolders(node, localItems.filter((item) => item.localRootId === rootId));
    localRoot.children.set(collection.id, node);
  }

  const knownLocalRootIds = new Set(collections.flatMap((collection) =>
    collection.kind === "local-root" && collection.localRootId ? [collection.localRootId] : []));
  const unlinkedLocalItems = localItems.filter((item) => !item.localRootId || !knownLocalRootIds.has(item.localRootId));
  if (unlinkedLocalItems.length > 0) {
    const node = mutableNode("source-collection:local-unlinked", "collection", "local", names.unlinkedFiles);
    addLocalFolders(node, unlinkedLocalItems);
    localRoot.children.set(node.id, node);
  }

  const youtubeCollections = collections.filter((entry) => entry.kind === "youtube-playlist");
  const youtubeCollectionIds = new Set(youtubeCollections.map((collection) => collection.id));
  for (const collection of youtubeCollections) {
    const node = mutableNode(
      `source-collection:${collection.id}`,
      "collection",
      "youtube",
      collection.name,
      collection.id,
    );
    node.issueCount = summaryIssues(summaryById, collection.id);
    node.directItems.push(...youtubeItems.filter((item) => item.sourceCollectionIds.includes(collection.id)));
    youtubeRoot.children.set(collection.id, node);
  }

  const savedVideos = youtubeItems.filter((item) =>
    !item.sourceCollectionIds.some((collectionId) => youtubeCollectionIds.has(collectionId)));
  if (savedVideos.length > 0 || youtubeCollections.length === 0) {
    const node = mutableNode(
      SAVED_YOUTUBE_VIDEOS_ID,
      "collection",
      "youtube",
      names.savedVideos,
    );
    node.directItems.push(...savedVideos);
    youtubeRoot.children.set(node.id, node);
  }

  localRoot.directItems = localItems.filter((item) => !localRoot.children.size);
  youtubeRoot.directItems = youtubeItems.filter((item) => !youtubeRoot.children.size);
  localRoot.issueCount = summaries.filter((summary) => summary.kind === "local-root")
    .reduce((total, summary) => total + summary.openIssueCount, 0);
  youtubeRoot.issueCount = summaries.filter((summary) => summary.kind === "youtube-playlist")
    .reduce((total, summary) => total + summary.openIssueCount, 0);
  return [freezeTree(localRoot), freezeTree(youtubeRoot)];
}

/** Finds one node in the stable source tree. */
export function findMusicSourceBrowserNode(
  nodes: readonly MusicSourceBrowserNode[],
  nodeId: string | null,
): MusicSourceBrowserNode | null {
  if (!nodeId) return null;
  for (const node of nodes) {
    if (node.id === nodeId) return node;
    const child = findMusicSourceBrowserNode(node.children, nodeId);
    if (child) return child;
  }
  return null;
}

/** Returns the selected node path from root to leaf. */
export function musicSourceBrowserPath(
  nodes: readonly MusicSourceBrowserNode[],
  nodeId: string,
): MusicSourceBrowserNode[] {
  for (const node of nodes) {
    if (node.id === nodeId) return [node];
    const childPath = musicSourceBrowserPath(node.children, nodeId);
    if (childPath.length > 0) return [node, ...childPath];
  }
  return [];
}

/** Returns the unique tracks contained by one source node and all descendants. */
export function musicSourceBrowserItems(node: MusicSourceBrowserNode): MusicItemListEntry[] {
  const byId = new Map<string, MusicItemListEntry>();
  const collect = (current: MusicSourceBrowserNode): void => {
    current.directItems.forEach((item) => byId.set(item.id, item));
    current.children.forEach(collect);
  };
  collect(node);
  return [...byId.values()];
}

/** Returns every unique track in the two stable source roots. */
export function allMusicSourceBrowserItems(nodes: readonly MusicSourceBrowserNode[]): MusicItemListEntry[] {
  const byId = new Map<string, MusicItemListEntry>();
  nodes.flatMap(musicSourceBrowserItems).forEach((item) => byId.set(item.id, item));
  return [...byId.values()];
}
