import { describe, expect, it } from "vitest";
import {
  CLEAN_BLOCK_SAVE,
  INITIAL_SAVE_QUEUE,
  NO_EDITOR_RECOVERY,
  notesEditorBanner,
  transitionBlockSave,
  transitionEditorRecovery,
  transitionSaveQueue,
  type NotesBlockEditConflict,
  type NotesBlockSaveState,
} from "./block-save-state";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import type { NotesBlockUpdate } from "$lib/notes/types";

function text(value: string): NotesBlockUpdate {
  const { id: _id, ...update } = createBlockWrite("b", "paragraph", value);
  return update;
}

const conflict: NotesBlockEditConflict = { blockId: "b", pageId: "p", kind: "text", mine: text("mine"), theirs: null };

function dirty(revision: number, scheduled: { update: NotesBlockUpdate; revision: number } | null = null): NotesBlockSaveState {
  return { phase: "dirty", base: text("base"), revision, scheduled };
}

function held(revision: number): NotesBlockSaveState {
  return { phase: "held", base: text("base"), revision, conflict };
}

describe("block save transitions", () => {
  it("marks a clean block dirty from the content before the edit", () => {
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "edited", base: text("base"), revision: 1 })).toEqual(dirty(1));
  });

  it("bumps the revision of a dirty or held block without replacing its base", () => {
    expect(transitionBlockSave(dirty(1), { type: "edited", base: text("other"), revision: 4 })).toEqual(dirty(4));
    expect(transitionBlockSave(held(1), { type: "edited", base: text("other"), revision: 4 })).toEqual(held(4));
  });

  it("schedules a save on a dirty block and replaces an earlier scheduled save", () => {
    const first = transitionBlockSave(dirty(2), { type: "scheduled", update: text("a"), revision: 2 });
    expect(first).toEqual(dirty(2, { update: text("a"), revision: 2 }));
    expect(transitionBlockSave(first, { type: "scheduled", update: text("ab"), revision: 3 })).toEqual(
      dirty(2, { update: text("ab"), revision: 3 }),
    );
  });

  it("treats a save scheduled on a clean block as unsaved without a known base", () => {
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "scheduled", update: text("a"), revision: 0 })).toEqual({
      phase: "dirty",
      base: null,
      revision: 0,
      scheduled: { update: text("a"), revision: 0 },
    });
  });

  it("turns typing in a held block into the held text instead of a save", () => {
    const next = transitionBlockSave(held(3), { type: "scheduled", update: text("later"), revision: 3 });
    expect(next).toEqual({ ...held(3), conflict: { ...conflict, mine: text("later") } });
  });

  it("clears the scheduled save once it leaves for the write queue and ignores a flush without one", () => {
    const scheduled = dirty(2, { update: text("a"), revision: 2 });
    expect(transitionBlockSave(scheduled, { type: "flushed" })).toEqual(dirty(2));
    const unscheduled = dirty(2);
    expect(transitionBlockSave(unscheduled, { type: "flushed" })).toBe(unscheduled);
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "flushed" })).toBe(CLEAN_BLOCK_SAVE);
  });

  it("cleans a block when its latest revision is saved", () => {
    expect(transitionBlockSave(dirty(5), { type: "saved", revision: 5, saved: text("s") })).toBe(CLEAN_BLOCK_SAVE);
  });

  it("keeps a block dirty on the saved content when a later edit exists", () => {
    const later = dirty(6, { update: text("later"), revision: 6 });
    expect(transitionBlockSave(later, { type: "saved", revision: 5, saved: text("s") })).toEqual({
      ...later,
      base: text("s"),
    });
  });

  it("ignores a save acknowledgement for a clean or held block", () => {
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "saved", revision: 1, saved: text("s") })).toBe(CLEAN_BLOCK_SAVE);
    const state = held(2);
    expect(transitionBlockSave(state, { type: "saved", revision: 2, saved: text("s") })).toBe(state);
  });

  it("cleans a block captured by a committed structural edit only when nothing newer waits", () => {
    expect(transitionBlockSave(dirty(3), { type: "committed", revision: 3 })).toBe(CLEAN_BLOCK_SAVE);
    const newer = dirty(4);
    expect(transitionBlockSave(newer, { type: "committed", revision: 3 })).toBe(newer);
    const scheduled = dirty(3, { update: text("a"), revision: 3 });
    expect(transitionBlockSave(scheduled, { type: "committed", revision: 3 })).toBe(scheduled);
    const state = held(3);
    expect(transitionBlockSave(state, { type: "committed", revision: 3 })).toBe(state);
  });

  it("holds an edit with its base and revision, dropping the scheduled save", () => {
    const scheduled = dirty(3, { update: text("a"), revision: 3 });
    expect(transitionBlockSave(scheduled, { type: "held", conflict })).toEqual(held(3));
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "held", conflict })).toEqual({
      phase: "held",
      base: null,
      revision: 0,
      conflict,
    });
  });

  it("abandons only a dirty block, never a held one", () => {
    expect(transitionBlockSave(dirty(1, { update: text("a"), revision: 1 }), { type: "abandoned" })).toBe(CLEAN_BLOCK_SAVE);
    const state = held(1);
    expect(transitionBlockSave(state, { type: "abandoned" })).toBe(state);
    expect(transitionBlockSave(CLEAN_BLOCK_SAVE, { type: "abandoned" })).toBe(CLEAN_BLOCK_SAVE);
  });

  it("settles only a held block", () => {
    expect(transitionBlockSave(held(1), { type: "settled" })).toBe(CLEAN_BLOCK_SAVE);
    const state = dirty(1);
    expect(transitionBlockSave(state, { type: "settled" })).toBe(state);
  });
});

describe("save queue transitions", () => {
  it("pauses on a failure of the current generation and reports the latest error", () => {
    const failed = transitionSaveQueue(INITIAL_SAVE_QUEUE, { type: "write-failed", generation: 0, error: "Disk full" });
    expect(failed).toEqual({ phase: "failed", generation: 0, error: "Disk full" });
    expect(transitionSaveQueue(failed, { type: "write-failed", generation: 0, error: "Locked" })).toEqual({
      phase: "failed",
      generation: 0,
      error: "Locked",
    });
  });

  it("ignores a failure from discarded writes", () => {
    const discarded = transitionSaveQueue(INITIAL_SAVE_QUEUE, { type: "discarded" });
    expect(transitionSaveQueue(discarded, { type: "write-failed", generation: 0, error: "Stale" })).toBe(discarded);
  });

  it("runs again on retry in the same generation and ignores a retry while running", () => {
    const failed = transitionSaveQueue(INITIAL_SAVE_QUEUE, { type: "write-failed", generation: 0, error: "Disk full" });
    expect(transitionSaveQueue(failed, { type: "retried" })).toEqual({ phase: "running", generation: 0 });
    expect(transitionSaveQueue(INITIAL_SAVE_QUEUE, { type: "retried" })).toBe(INITIAL_SAVE_QUEUE);
  });

  it("starts a new running generation when writes are discarded, even after a failure", () => {
    const failed = transitionSaveQueue(INITIAL_SAVE_QUEUE, { type: "write-failed", generation: 0, error: "Disk full" });
    expect(transitionSaveQueue(failed, { type: "discarded" })).toEqual({ phase: "running", generation: 1 });
  });
});

describe("editor recovery transitions", () => {
  it("shows the changed elsewhere notice after a refused write until dismissed", () => {
    const refused = transitionEditorRecovery(NO_EDITOR_RECOVERY, { type: "write-refused" });
    expect(refused).toEqual({ kind: "changed-elsewhere" });
    expect(transitionEditorRecovery(refused, { type: "dismissed" })).toBe(NO_EDITOR_RECOVERY);
  });

  it("keeps a failed reload until a reload succeeds, ignoring dismiss and later refusals", () => {
    const failed = transitionEditorRecovery(NO_EDITOR_RECOVERY, { type: "reload-failed", error: "Offline" });
    expect(transitionEditorRecovery(failed, { type: "dismissed" })).toBe(failed);
    expect(transitionEditorRecovery(failed, { type: "write-refused" })).toBe(failed);
    expect(transitionEditorRecovery(failed, { type: "reloaded" })).toBe(NO_EDITOR_RECOVERY);
  });

  it("keeps the changed elsewhere notice across a successful reload", () => {
    const refused = transitionEditorRecovery(NO_EDITOR_RECOVERY, { type: "write-refused" });
    expect(transitionEditorRecovery(refused, { type: "reloaded" })).toBe(refused);
  });

  it("clears every notice when another page opens", () => {
    const failed = transitionEditorRecovery(NO_EDITOR_RECOVERY, { type: "reload-failed", error: "Offline" });
    expect(transitionEditorRecovery(failed, { type: "page-opened" })).toBe(NO_EDITOR_RECOVERY);
  });
});

describe("editor banner", () => {
  it("shows a paused write queue before any recovery notice", () => {
    const failed = { phase: "failed", generation: 0, error: "Disk full" } as const;
    expect(notesEditorBanner(failed, { kind: "reload-failed", error: "Offline" })).toEqual({
      kind: "save-failed",
      error: "Disk full",
    });
  });

  it("shows the recovery notice while the queue runs", () => {
    expect(notesEditorBanner(INITIAL_SAVE_QUEUE, { kind: "changed-elsewhere" })).toEqual({ kind: "changed-elsewhere" });
    expect(notesEditorBanner(INITIAL_SAVE_QUEUE, NO_EDITOR_RECOVERY)).toEqual({ kind: "none" });
  });
});
