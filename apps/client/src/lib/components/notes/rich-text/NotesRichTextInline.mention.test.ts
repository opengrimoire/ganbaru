// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  notesPlainTextFromEditableRoot,
  notesTextSelectionFromEditableRoot,
  restoreNotesEditableSelection,
} from "$lib/notes/editor/selection";
import { createLocalObjectMentionRichText, createUserMentionRichText } from "$lib/notes/rich-text/core";
import type { NotesRichText } from "$lib/notes/types";
import NotesRichTextInline from "./NotesRichTextInline.svelte";

const PROJECT_ID = "33333333-3333-4333-8333-333333333333";
const PROJECT_EMOJI = "\u{1F34E}";

vi.mock("$lib/stores/projects.svelte", () => ({
  getProjects: () => ({
    customEmojis: [],
    projectById: (id: string) => (id === PROJECT_ID ? { id, icon: `emoji:${PROJECT_EMOJI}` } : undefined),
    taskById: () => undefined,
    selectProject: async () => undefined,
  }),
}));

const mounted: Array<{ component: ReturnType<typeof mount>; root: HTMLElement }> = [];

/** Mount mentions into an editable root the way the block editor hosts them. */
function editableRoot(richText: readonly NotesRichText[]): HTMLElement {
  const root = document.createElement("div");
  root.setAttribute("contenteditable", "true");
  document.body.append(root);
  const component = mount(NotesRichTextInline, {
    target: root,
    props: { richText, commentAnchors: [], suggestionAnchors: [] },
  });
  flushSync();
  mounted.push({ component, root });
  return root;
}

afterEach(async () => {
  for (const { component, root } of mounted.splice(0)) {
    await unmount(component);
    root.remove();
  }
});

describe("rich-text object mention rendering", () => {
  it("shows a project's own icon without adding it to the editable text", () => {
    const root = editableRoot([createLocalObjectMentionRichText("project", PROJECT_ID, "Eating plan")]);
    const chip = root.querySelector("[data-notes-mention-chip]");
    expect(chip?.getAttribute("data-notes-mention-type")).toBe("project");
    expect(chip?.getAttribute("role")).toBe("link");
    expect(chip?.querySelector("[data-notes-editor-sentinel='decoration']")?.textContent).toContain(PROJECT_EMOJI);
    expect(notesPlainTextFromEditableRoot(root)).toBe("Eating plan");
  });

  it("places carets at both mention edges outside the chip", () => {
    const root = editableRoot([createLocalObjectMentionRichText("project", PROJECT_ID, "Eating plan")]);
    const chip = root.querySelector("[data-notes-mention-chip]");
    for (const offset of [0, "Eating plan".length]) {
      expect(restoreNotesEditableSelection(root, { start: offset, end: offset })).toBe(true);
      const anchor = document.getSelection()?.anchorNode ?? null;
      expect(anchor !== null && chip?.contains(anchor)).toBe(false);
      expect(notesTextSelectionFromEditableRoot(root)).toEqual({ start: offset, end: offset });
    }
  });

  it("keeps people mentions as plain chips that do not navigate", () => {
    const root = editableRoot([createUserMentionRichText(PROJECT_ID, "Teammate")]);
    const chip = root.querySelector("[data-notes-mention-chip]");
    expect(chip?.getAttribute("data-notes-mention-type")).toBe("user");
    expect(chip?.hasAttribute("role")).toBe(false);
    expect(notesPlainTextFromEditableRoot(root)).toBe("Teammate");
  });
});
