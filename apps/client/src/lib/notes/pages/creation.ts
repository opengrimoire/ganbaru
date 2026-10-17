import { createBlockWrite, createRichText } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesLoadedPage, NotesPage, NotesPageCreate } from "$lib/notes/types";

/** Build the canonical local page tree shown while a page create command commits. */
export function createProvisionalNotesPage(
  request: NotesPageCreate,
  now = new Date().toISOString(),
): NotesLoadedPage {
  const write = createBlockWrite(request.first_block_id, "paragraph");
  if (write.type !== "paragraph") throw new Error("initial Notes block must be a paragraph");
  const page: NotesPage = {
    object: "page",
    id: request.id,
    created_time: now,
    last_edited_time: now,
    parent: request.parent,
    folder_id: request.folder_id,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      ...(request.properties ?? {}),
      title: {
        id: "title",
        type: "title",
        title: request.title ? [createRichText(request.title)] : [],
      },
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
  const block: NotesBlock = {
    object: "block",
    id: write.id,
    parent: { type: "page_id", page_id: page.id },
    created_time: now,
    last_edited_time: now,
    has_children: false,
    in_trash: false,
    archived: false,
    source_provider: null,
    source_object_id: null,
    source_last_edited_time: null,
    type: "paragraph",
    paragraph: write.paragraph,
  };
  return {
    page,
    blocks: {
      object: "list",
      type: "block",
      block: {},
      results: [block],
      next_cursor: null,
      has_more: false,
    },
  };
}
