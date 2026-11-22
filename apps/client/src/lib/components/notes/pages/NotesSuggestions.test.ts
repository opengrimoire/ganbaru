// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesSuggestionDraft } from "$lib/notes/collaboration/suggestions";
import type { NotesSuggestion } from "$lib/notes/types";
import NotesSuggestions from "./NotesSuggestions.svelte";

const draft: NotesSuggestionDraft = {
  block_id: "block-1",
  range_start: 0,
  range_end: 5,
  original_text: "Hello",
  proposed_text: "Hi",
  prefix: "",
  suffix: "",
};

const editor = vi.hoisted(() => ({
  store: {
    activeSuggestionDraft: null as NotesSuggestionDraft | null,
    suggestions: [] as NotesSuggestion[],
    suggestionsLoading: false,
    suggestionsError: null as string | null,
    suggestionsIncludeDecided: false,
    setSuggestionsIncludeDecided: (_include: boolean) => Promise.resolve(),
    blockById: (_blockId: string) => undefined,
    createSuggestion: (_proposedText: string) => Promise.resolve(),
    acceptSuggestion: (_suggestionId: string) => Promise.resolve(),
    rejectSuggestion: (_suggestionId: string) => Promise.resolve(),
    cancelSuggestionDraft: () => undefined,
  },
}));

vi.mock("$lib/components/notes/editor-context", () => ({
  getNotesEditor: () => editor.store,
}));

describe("NotesSuggestions", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    editor.store.activeSuggestionDraft = null;
    vi.restoreAllMocks();
  });

  it("saves a draft once while the first save is still in flight", async () => {
    let finish = (): void => undefined;
    const createSuggestion = vi.spyOn(editor.store, "createSuggestion")
      .mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    editor.store.activeSuggestionDraft = draft;
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesSuggestions, { target, props: { embedded: true } });
    flushSync();

    const save = [...target.querySelectorAll("button")].find((button) => button.textContent?.trim() === "Create suggestion");
    expect(save).toBeDefined();
    save?.click();
    flushSync();
    save?.click();
    flushSync();

    expect(createSuggestion).toHaveBeenCalledTimes(1);
    expect(createSuggestion).toHaveBeenCalledWith("Hi");
    expect(save?.disabled).toBe(true);

    finish();
    await tick();
    await tick();
    expect(save?.disabled).toBe(false);
  });
});
