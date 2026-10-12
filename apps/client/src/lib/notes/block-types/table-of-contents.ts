import { blockPlainText } from "$lib/notes/blocks/factory";
import type { NotesBlockTreeItem, NotesTableOfContentsItem } from "$lib/notes/types";

export function buildNotesTableOfContents(
  items: readonly NotesBlockTreeItem[],
): NotesTableOfContentsItem[] {
  return items.flatMap((item) => {
    const level = headingLevel(item.block.type);
    if (!level) return [];
    const title = blockPlainText(item.block).trim();
    if (!title) return [];
    return [{ blockId: item.block.id, title, level }];
  });
}

function headingLevel(type: string): 1 | 2 | 3 | 4 | 5 | 6 | null {
  if (type === "heading_1") return 1;
  if (type === "heading_2") return 2;
  if (type === "heading_3") return 3;
  if (type === "heading_4") return 4;
  if (type === "heading_5") return 5;
  if (type === "heading_6") return 6;
  return null;
}
