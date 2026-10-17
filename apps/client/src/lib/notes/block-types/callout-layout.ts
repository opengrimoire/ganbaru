import { blockColor, notesCalloutBackground } from "$lib/notes/blocks/color";
import { blockPlainText } from "$lib/notes/blocks/queries";
import type { NotesBlockOutlineItem } from "$lib/notes/blocks/outline";
import type { NotesBlock, NotesBlockTreeItem, NotesColor, NotesParent } from "$lib/notes/types";

export interface NotesCalloutLayer {
  id: string;
  color: NotesColor;
  first: boolean;
  last: boolean;
}

interface CalloutLayoutRow {
  id: string;
  type: NotesBlock["type"];
  parent: NotesParent;
}

/** An empty callout with children renders its icon beside the first child. */
export function notesCalloutIsOwnTextHidden(block: NotesBlock, childCount: number): boolean {
  return block.type === "callout" && blockPlainText(block).length === 0
    && (block.has_children || childCount > 0);
}

function layersForRows(
  rows: readonly CalloutLayoutRow[],
  readBlock: (id: string) => NotesBlock | undefined,
): ReadonlyMap<string, readonly NotesCalloutLayer[]> {
  const byId = new Map(rows.map((row) => [row.id, row]));
  const paths = new Map<string, string[]>();
  const pathFor = (id: string): string[] => {
    const cached = paths.get(id);
    if (cached) return cached;
    const path: string[] = [];
    const visited = new Set<string>();
    let row = byId.get(id);
    while (row && !visited.has(row.id)) {
      visited.add(row.id);
      if (row.type === "callout") path.unshift(row.id);
      row = row.parent.type === "block_id" ? byId.get(row.parent.block_id) : undefined;
    }
    paths.set(id, path);
    return path;
  };
  const result = new Map<string, readonly NotesCalloutLayer[]>();
  for (const [index, row] of rows.entries()) {
    const path = pathFor(row.id);
    if (!path.length) continue;
    const previous = index > 0 ? pathFor(rows[index - 1].id) : [];
    const next = index + 1 < rows.length ? pathFor(rows[index + 1].id) : [];
    result.set(row.id, path.map((id) => {
      const block = readBlock(id);
      const color = block?.type === "callout" ? blockColor(block) : "gray_background";
      return { id, color, first: !previous.includes(id), last: !next.includes(id) };
    }));
  }
  return result;
}

/** Describe callout surfaces across the virtualized document outline. */
export function notesCalloutLayers(
  outlines: readonly NotesBlockOutlineItem[],
  readBlock: (id: string) => NotesBlock | undefined,
): ReadonlyMap<string, readonly NotesCalloutLayer[]> {
  return layersForRows(outlines.map(({ outline }) => outline), readBlock);
}

/** Describe callout surfaces inside one embedded column or tab panel. */
export function notesEmbeddedCalloutLayers(
  items: readonly NotesBlockTreeItem[],
): ReadonlyMap<string, readonly NotesCalloutLayer[]> {
  const blocks = new Map(items.map(({ block }) => [block.id, block]));
  return layersForRows(items.map(({ block }) => block), (id) => blocks.get(id));
}

/** Keep the background token on the complete callout surface. */
export function notesCalloutLayerStyle(color: NotesColor): string {
  return `--notes-callout-background: ${notesCalloutBackground(color)}`;
}
