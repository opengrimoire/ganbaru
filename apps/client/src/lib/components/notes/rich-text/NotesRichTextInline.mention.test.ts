// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  notesPlainTextFromEditableRoot,
  notesTextSelectionFromEditableRoot,
  restoreNotesEditableSelection,
} from "$lib/notes/editor/selection";
import {
  createDateMentionRichText,
  createDateMentionValue,
  createLocalObjectMentionRichText,
  createUserMentionRichText,
} from "$lib/notes/rich-text/core";
import type { NotesDateMentionRichText, NotesRichText } from "$lib/notes/types";
import NotesRichTextInline from "./NotesRichTextInline.svelte";

const PROJECT_ID = "33333333-3333-4333-8333-333333333333";
const PROJECT_EMOJI = "\u{1F34E}";
const GROUP_ID = "44444444-4444-4444-8444-444444444444";

vi.mock("$lib/stores/projects.svelte", () => {
  const getProjects = () => ({
    customEmojis: [],
    projectById: (id: string) => (id === PROJECT_ID
      ? { id, groupId: GROUP_ID, name: "Eating plan", icon: `emoji:${PROJECT_EMOJI}` }
      : undefined),
    groupById: (id: string | undefined) => (id === GROUP_ID ? { id, name: "Health" } : undefined),
    taskById: () => undefined,
    selectProject: async () => undefined,
    showProjectView: () => undefined,
  });
  return { getProjects, useProjects: getProjects, getProjectsStore: getProjects };
});

const { loadTask } = vi.hoisted(() => ({
  loadTask: vi.fn<(taskId: string) => Promise<{ task: { projectId: string; title: string } }>>(),
}));
vi.mock("$lib/api/projects", () => ({ loadProjectTaskDetail: loadTask }));

const mounted: Array<{ component: ReturnType<typeof mount>; root: HTMLElement }> = [];

/** Mount mentions into an editable root the way the block editor hosts them. */
function editableRoot(
  richText: readonly NotesRichText[],
  onEditDateMention?: (item: NotesDateMentionRichText, anchor: HTMLElement) => void,
): HTMLElement {
  const root = document.createElement("div");
  root.setAttribute("contenteditable", "true");
  document.body.append(root);
  const component = mount(NotesRichTextInline, {
    target: root,
    props: { richText, commentAnchors: [], suggestionAnchors: [], onEditDateMention },
  });
  flushSync();
  mounted.push({ component, root });
  return root;
}

beforeEach(() => {
  vi.useFakeTimers();
  loadTask.mockReset().mockResolvedValue({ task: { projectId: PROJECT_ID, title: "Plan meals" } });
});

afterEach(async () => {
  for (const { component, root } of mounted.splice(0)) {
    await unmount(component);
    root.remove();
  }
  document.body.replaceChildren();
  vi.clearAllTimers();
  vi.useRealTimers();
});

/** Hover a chip long enough to open its preview and let lazy lookups settle. */
async function hoverPreview(chip: Element | null): Promise<HTMLElement> {
  const event = new MouseEvent("pointerenter", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "pointerType", { value: "mouse" });
  chip?.dispatchEvent(event);
  await vi.advanceTimersByTimeAsync(350);
  await vi.dynamicImportSettled();
  flushSync();
  const card = document.querySelector("[data-notes-mention-preview]");
  if (!(card instanceof HTMLElement)) throw new Error("Expected a mention preview");
  return card;
}

function previewParts(card: HTMLElement): { route: string | undefined; title: string | undefined } {
  return {
    route: card.querySelector(".sr-only")?.textContent ?? undefined,
    title: card.querySelector("[data-notes-mention-preview-title]")?.textContent ?? undefined,
  };
}

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

  it("opens the date editor from a date chip with the stored item, but not while text is selected", () => {
    const onEdit = vi.fn<(item: NotesDateMentionRichText, anchor: HTMLElement) => void>();
    const date = createDateMentionRichText(createDateMentionValue("2026-10-12", false), "Tomorrow");
    const root = editableRoot([date, createUserMentionRichText(PROJECT_ID, "Teammate")], onEdit);
    const [dateChip, userChip] = root.querySelectorAll<HTMLElement>("[data-notes-mention-chip]");
    expect(dateChip.getAttribute("aria-haspopup")).toBe("dialog");
    expect(userChip.hasAttribute("aria-haspopup")).toBe(false);
    userChip.click();
    expect(onEdit).not.toHaveBeenCalled();
    dateChip.click();
    expect(onEdit).toHaveBeenCalledExactlyOnceWith(date, dateChip);
    const range = document.createRange();
    range.selectNodeContents(root);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    dateChip.click();
    expect(onEdit).toHaveBeenCalledOnce();
  });

  it("leaves date chips inert in hosts that do not edit dates", () => {
    const root = editableRoot([createDateMentionRichText(createDateMentionValue("2026-10-12", false), "Tomorrow")]);
    const chip = root.querySelector<HTMLElement>("[data-notes-mention-chip]");
    expect(chip?.hasAttribute("aria-haspopup")).toBe(false);
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    chip?.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(false);
  });
});

describe("rich-text mention previews", () => {
  it("shows a project's group above its name", async () => {
    const root = editableRoot([createLocalObjectMentionRichText("project", PROJECT_ID, "Old name")]);
    const card = await hoverPreview(root.querySelector("[data-notes-mention-chip]"));
    expect(previewParts(card)).toEqual({ route: "Health", title: "Eating plan" });
    expect(root.contains(card)).toBe(false);
    expect(root.querySelector("[data-notes-mention-chip]")?.getAttribute("aria-describedby")).toBe(card.id);
  });

  it("loads an unloaded task once and shows its group and project above its title", async () => {
    const root = editableRoot([createLocalObjectMentionRichText("project_task", "task-1", "Meals")]);
    const card = await hoverPreview(root.querySelector("[data-notes-mention-chip]"));
    expect(loadTask).toHaveBeenCalledExactlyOnceWith("task-1");
    expect(previewParts(card)).toEqual({ route: "Health / Eating plan", title: "Plan meals" });
  });

  it("previews non-navigable people mentions without a route", async () => {
    const root = editableRoot([createUserMentionRichText(PROJECT_ID, "Teammate")]);
    const card = await hoverPreview(root.querySelector("[data-notes-mention-chip]"));
    expect(previewParts(card)).toEqual({ route: undefined, title: "Teammate" });
  });

  it("spells out a date mention and notes its reminder", async () => {
    const root = editableRoot([createDateMentionRichText(createDateMentionValue("2026-10-12", true), "Tomorrow")]);
    const card = await hoverPreview(root.querySelector("[data-notes-mention-chip]"));
    expect(previewParts(card).title).toBe("Tomorrow");
    expect(card.textContent).toContain("Oct 12, 2026");
    expect(card.textContent).toContain("Reminder");
  });

  it("names how early a date mention reminds", async () => {
    const value = { ...createDateMentionValue("2026-10-12T15:00", true), ganbaru_reminder: { enabled: true, minutes_before: 30 } };
    const root = editableRoot([createDateMentionRichText(value, "Remind Oct 12")]);
    const card = await hoverPreview(root.querySelector("[data-notes-mention-chip]"));
    expect(card.textContent).toContain("Reminder, 30 minutes before");
  });

  it("does not open for touch and closes on Escape", async () => {
    const root = editableRoot([createLocalObjectMentionRichText("project", PROJECT_ID, "Eating plan")]);
    const chip = root.querySelector("[data-notes-mention-chip]");
    const touch = new MouseEvent("pointerenter", { bubbles: true });
    Object.defineProperty(touch, "pointerType", { value: "touch" });
    chip?.dispatchEvent(touch);
    await vi.advanceTimersByTimeAsync(350);
    expect(document.querySelector("[data-notes-mention-preview]")).toBeNull();
    await hoverPreview(chip);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    flushSync();
    expect(document.querySelector("[data-notes-mention-preview]")).toBeNull();
  });
});
