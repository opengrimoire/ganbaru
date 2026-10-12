import type {
  NotesWorkingMarkdownNode,
  NotesWorkingMarkdownRoot,
} from "$lib/notes/types";

export interface NotesWorkingMarkdownTreeItem {
  key: string;
  kind: "root" | "directory" | "file";
  depth: number;
  workingFolderId: string;
  workingFolderName: string;
  relativePath: string;
  name: string;
  sourceKind: "managed" | "external";
  truncated: boolean;
  expandable: boolean;
}

/** Build visible file-backed Notes rows with stable source-aware identities. */
export function buildWorkingMarkdownTreeItems(
  roots: NotesWorkingMarkdownRoot[],
  collapsedKeys: ReadonlySet<string>,
  query: string,
): NotesWorkingMarkdownTreeItem[] {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  return roots.flatMap((root) => {
    const matchingNodes = normalizedQuery
      ? filterNodes(root.nodes, normalizedQuery)
      : root.nodes;
    if (matchingNodes.length === 0 && normalizedQuery) return [];
    const rootKey = workingMarkdownRootKey(root.workingFolderId);
    const rows: NotesWorkingMarkdownTreeItem[] = [{
      key: rootKey,
      kind: "root",
      depth: 0,
      workingFolderId: root.workingFolderId,
      workingFolderName: root.displayName,
      relativePath: "",
      name: root.displayName,
      sourceKind: root.sourceKind,
      truncated: root.truncated,
      expandable: matchingNodes.length > 0,
    }];
    if (!normalizedQuery && collapsedKeys.has(rootKey)) return rows;
    flattenNodes(rows, root, matchingNodes, 1, collapsedKeys, Boolean(normalizedQuery));
    return rows;
  });
}

export function workingMarkdownRootKey(workingFolderId: string): string {
  return `working-folder:${workingFolderId}`;
}

export function workingMarkdownNodeKey(
  workingFolderId: string,
  kind: "directory" | "file",
  relativePath: string,
): string {
  return `working-folder:${workingFolderId}:${kind}:${relativePath}`;
}

function flattenNodes(
  rows: NotesWorkingMarkdownTreeItem[],
  root: NotesWorkingMarkdownRoot,
  nodes: NotesWorkingMarkdownNode[],
  depth: number,
  collapsedKeys: ReadonlySet<string>,
  revealMatches: boolean,
): void {
  for (const node of nodes) {
    const key = workingMarkdownNodeKey(root.workingFolderId, node.kind, node.relativePath);
    rows.push({
      key,
      kind: node.kind,
      depth,
      workingFolderId: root.workingFolderId,
      workingFolderName: root.displayName,
      relativePath: node.relativePath,
      name: node.name,
      sourceKind: root.sourceKind,
      truncated: false,
      expandable: node.kind === "directory" && node.children.length > 0,
    });
    if (node.kind === "directory" && (revealMatches || !collapsedKeys.has(key))) {
      flattenNodes(rows, root, node.children, depth + 1, collapsedKeys, revealMatches);
    }
  }
}

function filterNodes(
  nodes: NotesWorkingMarkdownNode[],
  normalizedQuery: string,
): NotesWorkingMarkdownNode[] {
  const result: NotesWorkingMarkdownNode[] = [];
  for (const node of nodes) {
    const matchingChildren = node.kind === "directory"
      ? filterNodes(node.children, normalizedQuery)
      : [];
    if (node.name.toLocaleLowerCase().includes(normalizedQuery) || matchingChildren.length > 0) {
      result.push({ ...node, children: matchingChildren });
    }
  }
  return result;
}
