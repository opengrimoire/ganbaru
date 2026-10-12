import { describe, expect, it } from "vitest";
import { blockPlainText, createBlockWrite } from "$lib/notes/blocks/factory";
import { buildNotesChildIdsByParent, type NotesTreeState } from "$lib/notes/blocks/tree";
import {
  createNotesUndoSnapshot,
  createNotesUndoSnapshotForBlocks,
  notesUndoStateByteLength,
  notesUndoShortcutAction,
  notesTextChangeUndoSelections,
  parseNotesUndoStateJson,
  recordNotesUndoEntry,
  serializeNotesUndoState,
  trimNotesUndoStateToByteLimit,
  type NotesUndoEntry,
  type NotesUndoSnapshot,
  type NotesUndoState,
} from "./undo-history";
import type { NotesBlock, NotesBlockWrite, NotesParent } from "$lib/notes/types";

const now = "2026-07-01T01:00:00.000Z";
const pageId = "11111111-1111-4111-8111-111111111111";
const otherPageId = "22222222-2222-4222-8222-222222222222";
const blockA = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const blockB = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const blockC = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

function blockFromWrite(write: NotesBlockWrite, parent: NotesParent): NotesBlock {
  if (write.type !== "paragraph") throw new Error("fixture only supports paragraph blocks");
  return {
    object: "block",
    id: write.id,
    parent,
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
}

function paragraph(id: string, parent: NotesParent, text: string): NotesBlock {
  return blockFromWrite(createBlockWrite(id, "paragraph", text), parent);
}

function state(blocks: readonly NotesBlock[]): NotesTreeState {
  return {
    blocksById: Object.fromEntries(blocks.map((block) => [block.id, block])),
    childIdsByParentId: buildNotesChildIdsByParent(blocks),
  };
}

function pageSnapshot(text: string): NotesUndoSnapshot {
  const snapshot = createNotesUndoSnapshot(
    pageId,
    state([paragraph(blockA, { type: "page_id", page_id: pageId }, text)]),
    blockA,
  );
  if (!snapshot) throw new Error("snapshot fixture must create a snapshot");
  return snapshot;
}

function entry(
  before: NotesUndoSnapshot,
  after: NotesUndoSnapshot,
  id = "entry",
): NotesUndoEntry {
  return {
    id,
    kind: "typing",
    groupKey: "typing:block",
    before,
    after,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("notes undo history", () => {
  it("clones and persists directional document selection alongside block-local history", () => {
    const before = pageSnapshot("before");
    const selection = { anchor: { blockId: blockB, offset: Number.MAX_SAFE_INTEGER }, focus: { blockId: blockA, offset: 2 } };
    before.documentSelection = selection;
    const history = recordNotesUndoEntry({ undo: [], redo: [] }, { id: "range", kind: "delete", before, after: pageSnapshot("after") });
    selection.focus.offset = 3;
    const restored = parseNotesUndoStateJson(serializeNotesUndoState(history));
    expect(restored.undo[0].before.documentSelection).toEqual({ anchor: selection.anchor, focus: { blockId: blockA, offset: 2 } });
    expect(restored.undo[0].after.documentSelection).toBeUndefined();
  });

  it.each([-1, 0.5, Number.MAX_SAFE_INTEGER + 1])("rejects an invalid persisted document offset %s", (offset) => {
    const before = pageSnapshot("before");
    before.documentSelection = { anchor: { blockId: blockA, offset }, focus: { blockId: blockB, offset: 0 } };
    expect(() => parseNotesUndoStateJson(serializeNotesUndoState({ undo: [entry(before, pageSnapshot("after"))], redo: [] }))).toThrow("offset");
  });

  it("tracks the caret before and after controlled text changes", () => {
    expect(
      notesTextChangeUndoSelections(
        "Hello ",
        "Hello world",
        { start: 11, end: 11 },
      ),
    ).toEqual({
      before: { start: 6, end: 6 },
      after: { start: 11, end: 11 },
    });
    expect(
      notesTextChangeUndoSelections(
        "Hello world",
        "Hello there",
        { start: 11, end: 11 },
      ),
    ).toEqual({
      before: { start: 6, end: 11 },
      after: { start: 11, end: 11 },
    });
  });

  it("snapshots proxy-wrapped blocks from reactive state", () => {
    const block = new Proxy(
      paragraph(blockA, { type: "page_id", page_id: pageId }, "Reactive"),
      {},
    );
    const snapshot = createNotesUndoSnapshot(pageId, state([block]), blockA);

    expect(snapshot?.blocks).toHaveLength(1);
    expect(snapshot?.blocks[0]).not.toBe(block);
    expect(blockPlainText(snapshot?.blocks[0] ?? block)).toBe("Reactive");
  });

  it("captures only affected editor blocks for frequent local operations", () => {
    const first = paragraph(blockA, { type: "page_id", page_id: pageId }, "First");
    const second = paragraph(blockB, { type: "page_id", page_id: pageId }, "Second");
    const snapshot = createNotesUndoSnapshotForBlocks(
      pageId,
      state([first, second]),
      blockA,
      [blockA],
    );

    expect(snapshot?.blocks.map((block) => block.id)).toEqual([blockA]);
    expect(snapshot?.childIdsByParentId).toEqual({ [pageId]: [blockA, blockB] });
  });

  it("groups typing by block within the typing window", () => {
    const initial: NotesUndoState = { undo: [], redo: [] };
    const first = recordNotesUndoEntry(initial, {
      id: "one",
      kind: "typing",
      groupKey: `typing:${blockA}`,
      before: pageSnapshot("a"),
      after: pageSnapshot("ab"),
      now: 1000,
    });
    const second = recordNotesUndoEntry(first, {
      id: "two",
      kind: "typing",
      groupKey: `typing:${blockA}`,
      before: pageSnapshot("ab"),
      after: pageSnapshot("abc"),
      now: 1600,
    });

    expect(second.undo).toHaveLength(1);
    expect(blockPlainText(second.undo[0].before.blocks[0])).toBe("a");
    expect(blockPlainText(second.undo[0].after.blocks[0])).toBe("abc");
    expect(second.redo).toEqual([]);
  });

  it("does not record focus or selection-only changes", () => {
    const before = pageSnapshot("Same text");
    const after: NotesUndoSnapshot = {
      ...pageSnapshot("Same text"),
      focusBlockId: blockB,
      focusSelection: { start: 4, end: 4 },
    };
    const next = recordNotesUndoEntry({ undo: [], redo: [] }, {
      id: "selection-only",
      kind: "typing",
      groupKey: `typing:${blockA}`,
      before,
      after,
      now: 1000,
    });

    expect(next).toEqual({ undo: [], redo: [] });
  });

  it("groups consecutive Enter rows into one cumulative undo entry", () => {
    const parent = { type: "page_id", page_id: pageId } as const;
    const firstBlock = paragraph(blockA, parent, "First");
    const secondBlock = paragraph(blockB, parent, "");
    const thirdBlock = paragraph(blockC, parent, "");
    const beforeFirst = createNotesUndoSnapshotForBlocks(
      pageId,
      state([firstBlock]),
      blockA,
      [blockA],
    );
    const afterFirst = createNotesUndoSnapshotForBlocks(
      pageId,
      state([firstBlock, secondBlock]),
      blockB,
      [blockA, blockB],
    );
    const beforeSecond = createNotesUndoSnapshotForBlocks(
      pageId,
      state([firstBlock, secondBlock]),
      blockB,
      [blockB],
    );
    const afterSecond = createNotesUndoSnapshotForBlocks(
      pageId,
      state([firstBlock, secondBlock, thirdBlock]),
      blockC,
      [blockB, blockC],
    );
    if (!beforeFirst || !afterFirst || !beforeSecond || !afterSecond) {
      throw new Error("expected Enter snapshots");
    }
    const first = recordNotesUndoEntry({ undo: [], redo: [] }, {
      id: "first-enter",
      kind: "create",
      groupKey: `create:enter:${pageId}`,
      before: beforeFirst,
      after: afterFirst,
      now: 1000,
    });
    const second = recordNotesUndoEntry(first, {
      id: "second-enter",
      kind: "create",
      groupKey: `create:enter:${pageId}`,
      before: beforeSecond,
      after: afterSecond,
      now: 1100,
    });

    expect(second.undo).toHaveLength(1);
    expect(second.undo[0].before.blocks.map((block) => block.id)).toEqual([blockA]);
    expect(second.undo[0].after.blocks.map((block) => block.id)).toEqual([
      blockA,
      blockB,
      blockC,
    ]);
    expect(second.undo[0].after.focusBlockId).toBe(blockC);
  });

  it("keeps discrete operations as separate undo entries", () => {
    const first = recordNotesUndoEntry({ undo: [], redo: [] }, {
      id: "one",
      kind: "formatting",
      before: pageSnapshot("a"),
      after: pageSnapshot("ab"),
      now: 1000,
    });
    const second = recordNotesUndoEntry(first, {
      id: "two",
      kind: "link",
      before: pageSnapshot("ab"),
      after: pageSnapshot("abc"),
      now: 1100,
    });

    expect(second.undo.map((undoEntry) => undoEntry.kind)).toEqual(["formatting", "link"]);
  });

  it("clears redo when a new undo entry is recorded", () => {
    const before = pageSnapshot("a");
    const after = pageSnapshot("ab");
    const stateWithRedo: NotesUndoState = {
      undo: [],
      redo: [entry(before, after)],
    };
    const next = recordNotesUndoEntry(stateWithRedo, {
      id: "new",
      kind: "typing",
      groupKey: `typing:${blockA}`,
      before,
      after,
      now: 1000,
    });

    expect(next.undo).toHaveLength(1);
    expect(next.redo).toEqual([]);
  });

  it("recognizes standard undo and redo shortcuts", () => {
    expect(notesUndoShortcutAction({
      key: "z",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
    })).toBe("undo");
    expect(notesUndoShortcutAction({
      key: "Z",
      ctrlKey: false,
      metaKey: true,
      shiftKey: true,
      altKey: false,
    })).toBe("redo");
    expect(notesUndoShortcutAction({
      key: "y",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
    })).toBe("redo");
    expect(notesUndoShortcutAction({
      key: "z",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: true,
    })).toBeNull();
  });

  it("round trips serialized undo state and rejects unknown operation kinds", () => {
    const undoState: NotesUndoState = {
      undo: [entry(pageSnapshot("a"), pageSnapshot("ab"))],
      redo: [],
    };
    const parsed = parseNotesUndoStateJson(serializeNotesUndoState(undoState));
    expect(parsed.undo).toHaveLength(1);
    expect(blockPlainText(parsed.undo[0].after.blocks[0])).toBe("ab");

    const invalid = JSON.parse(serializeNotesUndoState(undoState)) as Record<string, unknown>;
    const undo = invalid.undo;
    if (!Array.isArray(undo) || typeof undo[0] !== "object" || undo[0] === null) {
      throw new Error("serialized undo fixture must include an entry");
    }
    (undo[0] as Record<string, unknown>).kind = "unknown";
    expect(() => parseNotesUndoStateJson(JSON.stringify(invalid))).toThrow(
      "undo[0].kind is not a supported undo kind",
    );
  });

  it("drops oldest undo entries until recovery state fits its byte limit", () => {
    const first = entry(pageSnapshot("a".repeat(256)), pageSnapshot("b".repeat(256)), "first");
    const second = {
      ...entry(pageSnapshot("c".repeat(256)), pageSnapshot("d".repeat(256)), "second"),
      createdAt: 2,
      updatedAt: 2,
    };
    const newestOnly = { undo: [second], redo: [] };
    const trimmed = trimNotesUndoStateToByteLimit(
      { undo: [first, second], redo: [] },
      notesUndoStateByteLength(newestOnly),
    );

    expect(trimmed.undo.map((undoEntry) => undoEntry.id)).toEqual(["second"]);
    expect(trimmed.redo).toEqual([]);
  });

  it("drops a single entry when one page snapshot cannot fit", () => {
    const oversized = {
      undo: [entry(pageSnapshot("x".repeat(1024)), pageSnapshot("y".repeat(1024)))],
      redo: [],
    };
    const emptyState = { undo: [], redo: [] };

    expect(
      trimNotesUndoStateToByteLimit(oversized, notesUndoStateByteLength(emptyState)),
    ).toEqual(emptyState);
  });

  it("measures undo recovery state as UTF-8 bytes", () => {
    const stateWithUnicode = {
      undo: [entry(pageSnapshot("努力"), pageSnapshot("頑張る"))],
      redo: [],
    };

    expect(notesUndoStateByteLength(stateWithUnicode))
      .toBeGreaterThan(serializeNotesUndoState(stateWithUnicode).length);
  });

  it("includes moved extra blocks outside the selected page tree", () => {
    const moved = paragraph(blockB, { type: "page_id", page_id: otherPageId }, "Moved");
    const snapshot = createNotesUndoSnapshot(
      pageId,
      state([paragraph(blockA, { type: "page_id", page_id: pageId }, "A")]),
      null,
      [moved],
    );

    expect(snapshot?.blocks.map((block) => block.id)).toEqual([blockA, blockB]);
    expect(snapshot?.childIdsByParentId[otherPageId]).toEqual([blockB]);
  });
});
