/**
 * Selection model for the invitation space picker. Groups contain projects and projects contain channels; selecting a
 * parent means the whole subtree, so the selected set only ever holds the highest selected nodes.
 */

export type InvitationSpaceNodeKind = "group" | "project" | "channel";

export interface InvitationSpaceNode {
  readonly id: string;
  readonly kind: InvitationSpaceNodeKind;
  readonly name: string;
  readonly parentId: string | null;
}

export type InvitationSpaceSelectionState = "none" | "some" | "all";

export interface InvitationSpaceSummaryEntry {
  readonly node: InvitationSpaceNode;
  /** The node itself is selected, or an ancestor is, so the summary names the node instead of its children. */
  readonly whole: boolean;
  readonly childCount: number;
  readonly children: readonly InvitationSpaceSummaryEntry[];
}

export function childrenOf(tree: readonly InvitationSpaceNode[], parentId: string | null): InvitationSpaceNode[] {
  return tree.filter((node) => node.parentId === parentId);
}

/** Ancestors from the nearest parent up to the root. */
export function ancestorsOf(tree: readonly InvitationSpaceNode[], id: string): InvitationSpaceNode[] {
  const ancestors: InvitationSpaceNode[] = [];
  let current = tree.find((node) => node.id === id);
  while (current?.parentId) {
    const parent = tree.find((node) => node.id === current?.parentId);
    if (!parent) break;
    ancestors.push(parent);
    current = parent;
  }
  return ancestors;
}

export function descendantsOf(tree: readonly InvitationSpaceNode[], id: string): InvitationSpaceNode[] {
  const descendants: InvitationSpaceNode[] = [];
  for (const child of childrenOf(tree, id)) {
    descendants.push(child, ...descendantsOf(tree, child.id));
  }
  return descendants;
}

/** Whether the node is covered by the selection, directly or through an ancestor. */
export function isSpaceSelected(tree: readonly InvitationSpaceNode[], selected: ReadonlySet<string>, id: string): boolean {
  return selected.has(id) || ancestorsOf(tree, id).some((ancestor) => selected.has(ancestor.id));
}

export function spaceSelectionState(tree: readonly InvitationSpaceNode[], selected: ReadonlySet<string>, id: string): InvitationSpaceSelectionState {
  if (isSpaceSelected(tree, selected, id)) return "all";
  const children = childrenOf(tree, id);
  if (children.length === 0) return "none";
  const states = children.map((child) => spaceSelectionState(tree, selected, child.id));
  if (states.every((state) => state === "all")) return "all";
  return states.some((state) => state !== "none") ? "some" : "none";
}

/** Returns the selection with the node turned on or off, keeping only the highest selected nodes. */
export function toggleSpaceSelection(tree: readonly InvitationSpaceNode[], selected: ReadonlySet<string>, id: string, on: boolean): Set<string> {
  const next = new Set(selected);
  const descendants = descendantsOf(tree, id);
  if (on) {
    for (const descendant of descendants) next.delete(descendant.id);
    next.add(id);
    return next;
  }
  const ancestors = ancestorsOf(tree, id);
  const coveringIndex = ancestors.findLastIndex((ancestor) => next.has(ancestor.id));
  if (coveringIndex >= 0) {
    let child = id;
    for (const ancestor of ancestors.slice(0, coveringIndex + 1)) {
      next.delete(ancestor.id);
      for (const sibling of childrenOf(tree, ancestor.id)) {
        if (sibling.id !== child) next.add(sibling.id);
      }
      child = ancestor.id;
    }
  }
  next.delete(id);
  for (const descendant of descendants) next.delete(descendant.id);
  return next;
}

function summarizeChildren(tree: readonly InvitationSpaceNode[], selected: ReadonlySet<string>, parentId: string | null): InvitationSpaceSummaryEntry[] {
  const entries: InvitationSpaceSummaryEntry[] = [];
  for (const node of childrenOf(tree, parentId)) {
    if (spaceSelectionState(tree, selected, node.id) === "none") continue;
    const whole = isSpaceSelected(tree, selected, node.id);
    entries.push({
      node,
      whole,
      childCount: childrenOf(tree, node.id).length,
      children: whole ? [] : summarizeChildren(tree, selected, node.id),
    });
  }
  return entries;
}

/** The selection as a tree of groups, projects, and channels for the invitation summary. */
export function summarizeSpaceSelection(tree: readonly InvitationSpaceNode[], selected: ReadonlySet<string>): InvitationSpaceSummaryEntry[] {
  return summarizeChildren(tree, selected, null);
}
