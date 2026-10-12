import { describe, expect, it, vi } from "vitest";
import { createBlockWrite, createRichText } from "$lib/notes/blocks/factory";
import type { NotesBlock, NotesPage } from "$lib/notes/types";
import type { NotesUndoSnapshot } from "$lib/notes/history/undo-history";
import { NotesTreeProjectionController } from "./tree-projection.svelte";

function page(id: string, title: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-03T00:00:00.000Z",
    last_edited_time: "2026-07-04T00:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: { title: { id: "title", type: "title", title: [createRichText(title)] } },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

describe("Notes tree projection", () => {
  it.each([false, true])("marks local edits without treating native receipts as unsaved drafts (%s)", (canonical) => {
    const selectedPage = page("page", "Page");
    const block: NotesBlock = {
      ...createBlockWrite("body", "paragraph", "Saved"), object: "block",
      parent: { type: "page_id", page_id: selectedPage.id },
      created_time: selectedPage.created_time, last_edited_time: selectedPage.last_edited_time,
      has_children: false, in_trash: false, source_provider: null,
      source_object_id: null, source_last_edited_time: null,
    } as NotesBlock;
    const projection = new NotesTreeProjectionController({ readSelectedPageId: () => selectedPage.id });
    const marker = vi.fn();
    projection.setLocalChangeMarker(marker);
    projection.applyPostMutation({ blocks: [block], canonical });
    expect(projection.blocksById.body).toEqual(block);
    expect(marker.mock.calls).toEqual(canonical ? [] : [[block.id]]);
    marker.mockClear();
    projection.applyPostMutation({ removedBlockIds: [block.id], canonical });
    expect(projection.blocksById.body).toBeUndefined();
    expect(marker.mock.calls).toEqual(canonical ? [] : [[block.id]]);
  });

  it("undoes the changed payload while preserving later unrelated text and order", () => {
    const selectedPage = page("page", "Page");
    const paragraph = (id: string, text: string, revision: string): NotesBlock => ({
      ...createBlockWrite(id, "paragraph", text), object: "block",
      parent: { type: "page_id", page_id: selectedPage.id },
      created_time: selectedPage.created_time, last_edited_time: selectedPage.created_time,
      has_children: false, in_trash: false, archived: false, edit_revision: revision.repeat(64),
      source_provider: null, source_object_id: null, source_last_edited_time: null,
    } as NotesBlock);
    const before = paragraph("changed", "Before", "1");
    const after = paragraph("changed", "After", "2");
    const originalUnrelated = paragraph("unrelated", "Original", "1");
    const laterUnrelated = paragraph("unrelated", "Later external edit", "3");
    const snapshot = (blocks: NotesBlock[]): NotesUndoSnapshot => ({
      pageId: selectedPage.id, blocks, childIdsByParentId: { [selectedPage.id]: blocks.map((block) => block.id) },
      focusBlockId: null, focusSelection: null,
    });
    const projection = new NotesTreeProjectionController({ readSelectedPageId: () => selectedPage.id });
    projection.setLoadedPage({ page: selectedPage, blocks: {
      object: "list", type: "block", block: {}, results: [after, laterUnrelated], next_cursor: null, has_more: false,
    } });
    const markLocallyChanged = vi.fn();
    projection.setLocalChangeMarker(markLocallyChanged);

    projection.applyLocalUndoSnapshot(snapshot([before, originalUnrelated]), snapshot([after, originalUnrelated]));

    expect(projection.blocksById.changed).toEqual({ ...before, edit_revision: after.edit_revision });
    expect(projection.blocksById.unrelated).toEqual(laterUnrelated);
    expect(projection.childIdsByParentId[selectedPage.id]).toEqual(["changed", "unrelated"]);
    expect(markLocallyChanged.mock.calls).toEqual([["changed"]]);
  });

  it("updates a loaded parent document's child page title after a rename", () => {
    const parentPage = page("parent-page", "Parent");
    const childWrite = createBlockWrite("child-page", "child_page", "Old title");
    if (childWrite.type !== "child_page") throw new Error("expected a child page block");
    const childBlock: NotesBlock = {
      object: "block",
      parent: { type: "page_id", page_id: parentPage.id },
      created_time: parentPage.created_time,
      last_edited_time: parentPage.created_time,
      has_children: false,
      in_trash: false,
      archived: false,
      source_provider: null,
      source_object_id: null,
      source_last_edited_time: null,
      ...childWrite,
    };
    const projection = new NotesTreeProjectionController({
      readSelectedPageId: () => parentPage.id,
    });
    const markLocallyChanged = vi.fn();
    projection.setLocalChangeMarker(markLocallyChanged);
    projection.setLoadedPage({
      page: parentPage,
      blocks: {
        object: "list",
        type: "block",
        block: {},
        results: [childBlock],
        next_cursor: null,
        has_more: false,
      },
    });

    projection.applyPostMutation({ pages: [page(childBlock.id, "New title")] });

    const updated = projection.blocksById[childBlock.id];
    expect(updated?.type).toBe("child_page");
    if (updated?.type !== "child_page") return;
    expect(updated.child_page.title).toBe("New title");
    expect(markLocallyChanged).not.toHaveBeenCalled();
  });
});
