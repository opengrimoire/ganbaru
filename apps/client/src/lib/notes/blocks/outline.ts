import { blockIndent } from "./queries";
import { blockChildrenAreVisible } from "./tree";
import type { NotesBlock, NotesBlockOutline, NotesBlockTreeItem } from "$lib/notes/types";

export interface NotesBlockOutlineItem {
  outline: NotesBlockOutline;
  depth: number;
}

/** Build a lightweight outline from hydrated blocks for authoritative local mutations. */
export function notesBlockOutlineFromBlock(
  block: NotesBlock,
  pageId: string,
  sortOrder: number,
): NotesBlockOutline {
  return {
    id: block.id,
    page_id: pageId,
    parent: block.parent.type === "block_id" || block.parent.type === "page_id"
      ? block.parent
      : { type: "page_id", page_id: pageId },
    type: block.type,
    sort_order: sortOrder,
    has_children: block.has_children,
    ganbaru_indent: blockIndent(block),
    retained_height: notesEstimatedBlockHeight(block.type),
  };
}

/** Return a stable height estimate for an unloaded block body. */
export function notesEstimatedBlockHeight(type: NotesBlock["type"]): number {
  if (["image", "video", "pdf", "bookmark", "link_preview", "embed"].includes(type)) return 240;
  if (["child_database", "table", "column_list", "tab"].includes(type)) return 180;
  if (type === "code" || type === "callout") return 72;
  if (["heading_1", "heading_2", "heading_3", "heading_4", "heading_5", "heading_6"].includes(type)) return 48;
  return 36;
}

/** Flatten page outlines in sibling order, omitting children of known hidden blocks. */
export function flattenNotesBlockOutlines(
  outlines: readonly NotesBlockOutline[],
  pageId: string,
  blocksById: Readonly<Record<string, NotesBlock>> = {},
  hiddenChildBlockIds?: ReadonlySet<string>,
): NotesBlockOutlineItem[] {
  const children = new Map<string, NotesBlockOutline[]>();
  for (const outline of outlines) {
    const parentId = outline.parent.type === "page_id"
      ? outline.parent.page_id
      : outline.parent.block_id;
    const siblings = children.get(parentId) ?? [];
    siblings.push(outline);
    children.set(parentId, siblings);
  }
  for (const siblings of children.values()) {
    siblings.sort((left, right) => left.sort_order - right.sort_order || left.id.localeCompare(right.id));
  }
  const result: NotesBlockOutlineItem[] = [];
  const visited = new Set<string>();
  const append = (parentId: string, depth: number): void => {
    for (const outline of children.get(parentId) ?? []) {
      if (visited.has(outline.id)) continue;
      visited.add(outline.id);
      const itemDepth = depth + (outline.ganbaru_indent ?? 0);
      result.push({ outline, depth: itemDepth });
      const block = blocksById[outline.id];
      if (block ? blockChildrenAreVisible(block) : !hiddenChildBlockIds?.has(outline.id)) {
        append(outline.id, itemDepth + 1);
      }
    }
  };
  append(pageId, 0);
  return result;
}

/** Return complete outline subtrees, including children hidden by collapsed blocks. */
export function notesOutlineSubtreeIds(
  outlines: readonly NotesBlockOutline[],
  pageId: string,
  rootBlockIds: readonly string[],
): string[] {
  const roots = new Set(rootBlockIds);
  const outlinesById = new Map(outlines.map((outline) => [outline.id, outline]));
  const included: string[] = [];
  for (const { outline } of flattenNotesBlockOutlines(outlines, pageId)) {
    let current: NotesBlockOutline | undefined = outline;
    const visitedAncestors = new Set<string>();
    while (current && !visitedAncestors.has(current.id)) {
      visitedAncestors.add(current.id);
      if (roots.has(current.id)) {
        included.push(outline.id);
        break;
      }
      current = current.parent.type === "block_id"
        ? outlinesById.get(current.parent.block_id)
        : undefined;
    }
  }
  return included;
}

/** Map hydrated tree items to the order established by the lightweight outline. */
export function notesHydratedItemsByOutline(
  outlines: readonly NotesBlockOutlineItem[],
  hydratedItems: readonly NotesBlockTreeItem[],
): Map<string, NotesBlockTreeItem> {
  const hydrated = new Map(hydratedItems.map((item) => [item.block.id, item]));
  return new Map(outlines.flatMap((item) => {
    const block = hydrated.get(item.outline.id);
    return block ? [[item.outline.id, { ...block, depth: item.depth }] as const] : [];
  }));
}
