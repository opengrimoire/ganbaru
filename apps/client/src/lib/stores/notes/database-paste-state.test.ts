import { describe, expect, it } from "vitest";
import {
  IDLE_DATABASE_PASTE,
  transitionDatabasePaste,
  type NotesDatabasePastePrompt,
  type NotesDatabasePasteState,
} from "./database-paste-state";

const prompt: NotesDatabasePastePrompt = { kind: "copy", pageId: "page", blockId: "copy", copies: { copy: "source" } };
const offered: NotesDatabasePasteState = { phase: "offered", prompt, generation: 1 };

describe("database paste transitions", () => {
  it("offers a prompt and replaces an earlier one", () => {
    expect(transitionDatabasePaste(IDLE_DATABASE_PASTE, { type: "offered", prompt, generation: 1 })).toEqual(offered);
    const writing = transitionDatabasePaste(offered, { type: "write-started", token: 1 });
    expect(transitionDatabasePaste(writing, { type: "offered", prompt, generation: 2 })).toEqual({ ...offered, generation: 2 });
  });

  it("refuses a second choice while one writes", () => {
    const writing = transitionDatabasePaste(offered, { type: "write-started", token: 1 });
    expect(writing).toEqual({ ...offered, phase: "writing", token: 1, started: false });
    expect(transitionDatabasePaste(writing, { type: "write-started", token: 2 })).toBe(writing);
  });

  it("returns an unqueued choice to the prompt and closes a queued one when it settles", () => {
    const writing = transitionDatabasePaste(offered, { type: "write-started", token: 1 });
    expect(transitionDatabasePaste(writing, { type: "write-settled", token: 1 })).toEqual(offered);
    const queued = transitionDatabasePaste(writing, { type: "enqueued", token: 1 });
    expect(transitionDatabasePaste(queued, { type: "write-settled", token: 1 })).toBe(IDLE_DATABASE_PASTE);
  });

  it("keeps a queued failure retryable and ignores the end of another attempt", () => {
    const queued = transitionDatabasePaste(transitionDatabasePaste(offered, { type: "write-started", token: 1 }), { type: "enqueued", token: 1 });
    expect(transitionDatabasePaste(queued, { type: "write-failed", token: 2, error: "Late" })).toBe(queued);
    expect(transitionDatabasePaste(queued, { type: "write-settled", token: 2 })).toBe(queued);
    const failed = transitionDatabasePaste(queued, { type: "write-failed", token: 1, error: "Unavailable" });
    expect(failed).toEqual({ phase: "failed", prompt, generation: 1, error: "Unavailable", started: true });
    expect(transitionDatabasePaste(failed, { type: "write-started", token: 2 })).toEqual({ ...offered, phase: "writing", token: 2, started: true });
  });

  it("completes and retargets only its own prompt", () => {
    const retargeted = transitionDatabasePaste(offered, { type: "retargeted", generation: 1, blockId: "database" });
    expect(retargeted).toEqual({ ...offered, prompt: { ...prompt, blockId: "database" } });
    expect(transitionDatabasePaste(offered, { type: "retargeted", generation: 2, blockId: "database" })).toBe(offered);
    expect(transitionDatabasePaste(offered, { type: "completed", generation: 2 })).toBe(offered);
    expect(transitionDatabasePaste(offered, { type: "completed", generation: 1 })).toBe(IDLE_DATABASE_PASTE);
  });

  it("dismisses from any phase and ignores late events afterwards", () => {
    const writing = transitionDatabasePaste(offered, { type: "write-started", token: 1 });
    const dismissed = transitionDatabasePaste(writing, { type: "dismissed" });
    expect(dismissed).toBe(IDLE_DATABASE_PASTE);
    expect(transitionDatabasePaste(dismissed, { type: "write-failed", token: 1, error: "Late" })).toBe(dismissed);
    expect(transitionDatabasePaste(dismissed, { type: "enqueued", token: 1 })).toBe(dismissed);
    expect(transitionDatabasePaste(dismissed, { type: "dismissed" })).toBe(dismissed);
  });
});
