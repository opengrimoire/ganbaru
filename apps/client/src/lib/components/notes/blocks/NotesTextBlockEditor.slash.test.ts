// @vitest-environment jsdom
import { mount, tick, unmount, type ComponentProps } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyBlockUpdate, blockEditableRichText, blockPlainText, blockWithRichText, createBlockUpdate, createBlockWrite } from "$lib/notes/blocks/factory";
import { applyRichTextLink } from "$lib/notes/rich-text/core";
import { notesTextSelectionFromEditableRoot, restoreNotesEditableSelection } from "$lib/notes/editor/selection";
import { EMPTY_NOTES_BUTTON_BLOCK_STATUS } from "$lib/notes/block-types/button";
import { EMPTY_NOTES_TEMPLATE_BLOCK_STATUS } from "$lib/notes/block-types/template";
import type { NotesBlock, NotesBlockType, NotesRichText } from "$lib/notes/types";
import NotesTextBlockEditor from "./NotesTextBlockEditor.svelte";
import { createNotesBlockSelectionController } from "./selection-controller.svelte";

const openLink = vi.hoisted(() => vi.fn(async (_url: string) => undefined));
vi.mock("$lib/notes/links/navigation", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/notes/links/navigation")>(), openNotesTextLink: openLink,
}));

let component: ReturnType<typeof mount> | undefined;
let releaseDelegates: (() => void) | undefined;
afterEach(async () => {
  releaseDelegates?.();
  releaseDelegates = undefined;
  if (component) await unmount(component);
  component = undefined;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.useRealTimers();
  openLink.mockClear();
});

/** Use a reactive block to exercise the real input, lazy menu, and conversion path. */
async function editor(blockType: NotesBlockType = "paragraph", text = "", indentationDepth = 0, initialFocusBlockId = "block") {
  const block: NotesBlock = {
    ...createBlockWrite("block", blockType, text), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null,
  } as NotesBlock;
  const blocks = writable(block);
  const current = fromStore(blocks);
  const onConvert = vi.fn();
  const onTextInput = vi.fn((_id: string, value: string) => blocks.update((block) => applyBlockUpdate(block, createBlockUpdate(block.type, value))));
  const onKeyboardAction = vi.fn();
  const onFocusBlock = vi.fn();
  const readMentionTargets = vi.fn(() => []);
  const releaseMentionTargets = vi.fn();
  const acquireMentionTargets = vi.fn(() => releaseMentionTargets);
  const onPasteRichHtml = vi.fn((_id: string, _start: number, _end: number, _html: string) => false);
  const onApplyTextLink = vi.fn((_id: string, start: number, end: number, url: string | null) => {
    blocks.update((block) => applyBlockUpdate(block, blockWithRichText(block, applyRichTextLink(blockEditableRichText(block), start, end, url))));
  });
  const onReplaceRichText = vi.fn((_id: string, richText: readonly NotesRichText[]) => {
    blocks.update((block) => applyBlockUpdate(block, blockWithRichText(block, richText)));
  });
  const props: ComponentProps<typeof NotesTextBlockEditor> = {
    get block() { return current.current; }, previousBlockType: null, isOnlyBlock: true, indentationDepth,
    focusBlockId: initialFocusBlockId, focusRequestId: 1, focusSelection: { start: 0, end: 0 },
    mentionTargets: { read: readMentionTargets, acquire: acquireMentionTargets }, commentAnchors: [], suggestionAnchors: [],
    templateStatus: EMPTY_NOTES_TEMPLATE_BLOCK_STATUS, buttonStatus: EMPTY_NOTES_BUTTON_BLOCK_STATUS,
    onTextInput,
    onConvert, onReplaceRichText, onInsertPageMention: vi.fn(), onInsertDateMention: vi.fn(),
    onInsertObjectMention: vi.fn(), onApplyTextLink, onInsertInlineEquation: vi.fn(),
    onPastePlainText: () => false, onPasteRichHtml, onApplyTextAnnotations: vi.fn(),
    onCreateInlineComment: vi.fn(), onCreateInlineSuggestion: vi.fn(), onKeyboardAction,
    onUndo: vi.fn(), onRedo: vi.fn(), onAddBelow: vi.fn(), onConvertToToggleHeading: vi.fn(),
    onColorChange: vi.fn(), onCopyLink: vi.fn(), onDuplicate: vi.fn(), onUseTemplate: vi.fn(),
    onAddTemplateChild: vi.fn(), onUseButton: vi.fn(), onAddButtonChild: vi.fn(), onButtonIconChange: vi.fn(),
    onButtonInsertPositionChange: vi.fn(), onMoveUp: vi.fn(), onMoveDown: vi.fn(), onDelete: vi.fn(),
    onToggleOpen: vi.fn(), onCodeLanguageChange: vi.fn(), onFocusBlock,
  };
  const list = document.createElement("div");
  const row = document.createElement("div");
  row.dataset.notesSelectableBlockId = "block";
  row.tabIndex = -1;
  list.append(row);
  document.body.append(list);
  const focusRow = vi.fn(() => row.focus());
  const navigationKeydown = vi.fn(() => false);
  const blockSelection = createNotesBlockSelectionController({
    hydrateSubtrees: async (ids) => ids,
    readPageId: () => "page", readListElement: () => list, readRenderedBlockIds: () => ["block"],
    readTreeState: () => ({ blocksById: {}, childIdsByParentId: {} }),
    blockIdFromEvent: () => "block",
    targetIsEditable: (target) => target instanceof Element && !!target.closest('[contenteditable="true"]'),
    targetIsSelectionZone: () => false, focusTextEditorAtEnd: () => true, focusRow,
    handleNavigationKeydown: navigationKeydown, undo: async () => false, redo: async () => false,
    pasteBlocks: async () => null, duplicateBlocks: async () => null,
    moveBlocks: async () => undefined, deleteBlocks: async () => undefined,
  });
  releaseDelegates = blockSelection.delegation(list).destroy;
  component = mount(NotesTextBlockEditor, { target: row, props });
  await tick(); await tick();
  const host = document.querySelector<HTMLElement>('[role="textbox"]')!;
  host.focus();
  const input = async (data: string, inputType = "insertText") => {
    const event = new InputEvent("beforeinput", { inputType, data, bubbles: true, cancelable: true });
    host.dispatchEvent(event);
    await tick(); await tick();
    return event;
  };
  return { host, input, onConvert, onTextInput, onKeyboardAction, onPasteRichHtml, onFocusBlock, row, blockSelection, focusRow, navigationKeydown, readMentionTargets, acquireMentionTargets, releaseMentionTargets,
    onApplyTextLink, onReplaceRichText, readBlock: () => current.current };
}

/** Deliver the actual clipboard event path rather than synthesizing an inserted URL. */
function pasteUrl(host: HTMLElement, url: string): Event {
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { getData: (type: string) => type === "text/plain" ? url : "" } });
  host.dispatchEvent(event);
  return event;
}

/** Hover linked text through the editor's delegated pointer handler. */
function hoverLink(host: HTMLElement): HTMLAnchorElement {
  const link = host.querySelector<HTMLAnchorElement>("a")!;
  link.dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
  return link;
}

describe("Notes inline links", () => {
  it("links selected text when a URL is pasted, offers hover actions, and opens on plain or modifier click", async () => {
    const h = await editor("paragraph", "Before Text after");
    restoreNotesEditableSelection(h.host, { start: 7, end: 11 });
    expect(pasteUrl(h.host, "https://example.com/tasks").defaultPrevented).toBe(true);
    await tick(); await tick();
    expect(h.onApplyTextLink).toHaveBeenCalledExactlyOnceWith("block", 7, 11, "https://example.com/tasks");
    expect(blockPlainText(h.readBlock())).toBe("Before Text after");
    expect(h.onPasteRichHtml).not.toHaveBeenCalled();
    const link = h.host.querySelector<HTMLAnchorElement>("a")!;
    expect(link.getAttribute("href")).toBe("https://example.com/tasks");
    expect(link.hasAttribute("title")).toBe(false);
    restoreNotesEditableSelection(h.host, { start: 8, end: 8 });
    hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    const panel = document.querySelector<HTMLElement>("[data-notes-link-panel]")!;
    expect(h.row.contains(panel)).toBe(false);
    expect(panel.classList.contains("fixed")).toBe(true);
    const open = panel.querySelector<HTMLButtonElement>('button[aria-label="Open link"]')!;
    open.click();
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledExactlyOnceWith("https://example.com/tasks"));
    await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledTimes(2));
    await tick();
    restoreNotesEditableSelection(h.host, { start: 8, end: 8 });
    link.click();
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledTimes(3));
  });

  it("edits the link title and destination without replacing surrounding text, and can remove the link", async () => {
    const h = await editor("paragraph", "Before Text after");
    restoreNotesEditableSelection(h.host, { start: 7, end: 11 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    restoreNotesEditableSelection(h.host, { start: 8, end: 8 });
    hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    [...document.querySelectorAll<HTMLButtonElement>("[data-notes-link-panel] button")].find((button) => button.textContent === "Edit")!.click();
    await tick(); await tick();
    const title = document.querySelector<HTMLInputElement>("#notes-link-title")!;
    const url = document.querySelector<HTMLInputElement>("#notes-link-destination")!;
    title.value = "Task list";
    title.dispatchEvent(new Event("input", { bubbles: true }));
    url.value = "https://example.com/planning";
    url.dispatchEvent(new Event("input", { bubbles: true }));
    title.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await tick(); await tick();
    expect(blockPlainText(h.readBlock())).toBe("Before Task list after");
    expect(h.host.querySelector("a")?.getAttribute("href")).toBe("https://example.com/planning");
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    restoreNotesEditableSelection(h.host, { start: 8, end: 8 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
    await tick(); await tick();
    [...document.querySelectorAll<HTMLButtonElement>("[data-notes-link-panel] button")].find((button) => button.textContent?.includes("Remove link"))!.click();
    await tick(); await tick();
    expect(blockPlainText(h.readBlock())).toBe("Before Task list after");
    expect(h.host.querySelector("a")).toBeNull();
  });

  it("closes on outside click without stealing focus and on Escape with the original selection restored", async () => {
    const h = await editor("paragraph", "Text");
    restoreNotesEditableSelection(h.host, { start: 0, end: 4 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    document.body.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    h.host.focus();
    restoreNotesEditableSelection(h.host, { start: 0, end: 4 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
    await tick(); await tick();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick(); await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    expect(notesTextSelectionFromEditableRoot(h.host)).toEqual({ start: 0, end: 4 });
    expect(h.onApplyTextLink).not.toHaveBeenCalled();
  });

  it("keeps URL paste literal in code", async () => {
    const h = await editor("code", "Text");
    restoreNotesEditableSelection(h.host, { start: 0, end: 4 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    expect(h.onApplyTextLink).not.toHaveBeenCalled();
    expect(blockPlainText(h.readBlock())).toBe("https://example.com");
  });

  it("does not take focus back when another field is focused before the link editor mounts", async () => {
    const h = await editor("paragraph", "Text");
    restoreNotesEditableSelection(h.host, { start: 0, end: 4 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "k", ctrlKey: true, bubbles: true, cancelable: true }));
    const other = document.createElement("input");
    document.body.append(other);
    other.focus();
    await tick(); await tick(); await tick();
    expect(document.activeElement).toBe(other);
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    expect(h.onApplyTextLink).not.toHaveBeenCalled();
  });

  it("replaces selected words with the literal URL when explicitly pasting as plain text", async () => {
    const h = await editor("paragraph", "Before Text after");
    restoreNotesEditableSelection(h.host, { start: 7, end: 11 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "v", ctrlKey: true, shiftKey: true, bubbles: true }));
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    expect(h.onApplyTextLink).not.toHaveBeenCalled();
    expect(blockPlainText(h.readBlock())).toBe("Before https://example.com after");
  });

  it("keeps pasted local database links functional without sending them to the system browser", async () => {
    const h = await editor("paragraph", "Tasks");
    const hash = "#notes?page=11111111-1111-4111-8111-111111111111&block=22222222-2222-4222-8222-222222222222";
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, hash);
    await tick(); await tick();
    expect(blockPlainText(h.readBlock())).toBe("Tasks");
    const link = h.host.querySelector<HTMLAnchorElement>("a")!;
    expect(link.getAttribute("href")).toBe(hash);
    link.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, metaKey: true }));
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledExactlyOnceWith(hash));
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
  });

  it("shows navigation failures in the floating actions so the destination can be corrected", async () => {
    const h = await editor("paragraph", "Tasks");
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    openLink.mockRejectedValueOnce(new Error("Target unavailable"));
    h.host.querySelector<HTMLAnchorElement>("a")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel] [role=alert]")).not.toBeNull());
    expect(document.querySelector("[data-notes-link-panel]")?.textContent).toContain("Edit");
    hoverLink(h.host);
    await tick();
    expect(document.querySelector("[data-notes-link-panel] [role=alert]")).not.toBeNull();
    warning.mockRestore();
  });

  it("does not open link actions when clicking at the end of a text selection gesture", async () => {
    const h = await editor("paragraph", "Linked text");
    restoreNotesEditableSelection(h.host, { start: 0, end: 11 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    restoreNotesEditableSelection(h.host, { start: 2, end: 8 });
    h.host.querySelector<HTMLAnchorElement>("a")!.click();
    await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    expect(openLink).not.toHaveBeenCalled();
  });

  it("dismisses the link preview when typing resumes in the note", async () => {
    const h = await editor("paragraph", "Tasks");
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    restoreNotesEditableSelection(h.host, { start: 2, end: 2 });
    hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    restoreNotesEditableSelection(h.host, { start: 2, end: 2 });
    await h.input("X");
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    expect(blockPlainText(h.readBlock())).toBe("TaXsks");
  });

  it("keeps the preview reachable across the pointer gap, closes on leaving, and pins the edit form", async () => {
    const h = await editor("paragraph", "Tasks");
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    await tick(); await tick();
    restoreNotesEditableSelection(h.host, { start: 2, end: 2 });
    const link = hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    expect(notesTextSelectionFromEditableRoot(h.host)).toEqual({ start: 2, end: 2 });
    const panel = document.querySelector<HTMLElement>("[data-notes-link-panel]")!;
    vi.useFakeTimers();
    link.dispatchEvent(new MouseEvent("pointerout", { bubbles: true, relatedTarget: document.body }));
    await vi.advanceTimersByTimeAsync(75);
    panel.dispatchEvent(new MouseEvent("pointerenter"));
    await vi.advanceTimersByTimeAsync(200);
    expect(document.querySelector("[data-notes-link-panel]")).toBe(panel);
    panel.dispatchEvent(new MouseEvent("pointerleave"));
    await vi.advanceTimersByTimeAsync(200);
    await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    hoverLink(h.host);
    await tick(); await tick();
    const editPanel = document.querySelector<HTMLElement>("[data-notes-link-panel]")!;
    [...editPanel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Edit")!.click();
    await tick(); await tick();
    editPanel.dispatchEvent(new MouseEvent("pointerleave"));
    await vi.advanceTimersByTimeAsync(200);
    expect(document.querySelector("#notes-link-title")).not.toBeNull();
  });

  it("does not show hover actions during touch or text dragging", async () => {
    const h = await editor("paragraph", "Tasks");
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    const link = h.host.querySelector<HTMLAnchorElement>("a")!;
    const touch = new MouseEvent("pointerover", { bubbles: true });
    Object.defineProperty(touch, "pointerType", { value: "touch" });
    link.dispatchEvent(touch);
    link.dispatchEvent(new MouseEvent("pointerover", { bubbles: true, buttons: 1 }));
    await tick();
    expect(document.querySelector("[data-notes-link-panel]")).toBeNull();
    expect(openLink).not.toHaveBeenCalled();
  });

  it("does not constrain a fitting preview or editor to its inner height", async () => {
    const h = await editor("paragraph", "Tasks");
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
      return this.parentElement?.hasAttribute("data-notes-link-panel") ? 120 : 0;
    });
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector("[data-notes-link-panel]")).not.toBeNull());
    const panel = document.querySelector<HTMLElement>("[data-notes-link-panel]")!;
    panel.style.borderWidth = "1px";
    window.dispatchEvent(new Event("resize"));
    await tick();
    expect(panel.style.maxHeight).toBe("");
    [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Edit")!.click();
    await tick(); await tick();
    expect(panel.style.maxHeight).toBe("");
    const apply = [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Apply")!;
    const remove = [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.includes("Remove link"))!;
    expect(apply.parentElement).toBe(remove.parentElement);
    vi.stubGlobal("innerHeight", 80);
    window.dispatchEvent(new Event("resize"));
    await tick();
    expect(Number.parseFloat(panel.style.maxHeight)).toBeLessThan(120);
  });

  it("keeps preview and edit text smaller than paragraphs while following paragraph font changes", async () => {
    const h = await editor("paragraph", "Tasks");
    h.host.style.fontSize = "15px";
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    pasteUrl(h.host, "https://example.com");
    await tick(); await tick();
    hoverLink(h.host);
    await vi.waitFor(() => expect(document.querySelector<HTMLElement>("[data-notes-link-panel]")?.style.fontSize).toBe("12px"));
    const panel = document.querySelector<HTMLElement>("[data-notes-link-panel]")!;
    [...panel.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Edit")!.click();
    await tick(); await tick();
    expect(panel.style.fontSize).toBe("12px");
    expect(h.host.style.fontSize).toBe("15px");
    h.host.style.fontSize = "20px";
    window.dispatchEvent(new Event("resize"));
    await tick();
    expect(panel.style.fontSize).toBe("16px");
  });
});

describe("Notes typed slash commands", () => {
  it("reads and acquires mention data only while the mention menu is open", async () => {
    const h = await editor();
    await h.input("Ordinary text ");
    expect(h.readMentionTargets).not.toHaveBeenCalled();
    expect(h.acquireMentionTargets).not.toHaveBeenCalled();
    await h.input("@");
    expect(h.readMentionTargets).toHaveBeenCalled();
    expect(h.acquireMentionTargets).toHaveBeenCalledOnce();
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await tick();
    expect(h.releaseMentionTargets).toHaveBeenCalledOnce();
  });

  it.each([
    ["/Note", "child_page"],
    ["/datab", "child_database"],
  ] as const)("delegates %s search clearing to creation without an extra paragraph save", async (query, blockType) => {
    const h = await editor();
    await h.input(query);
    await vi.waitFor(() => expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(1));
    h.onTextInput.mockClear();
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(h.onConvert).toHaveBeenCalledExactlyOnceWith("block", blockType, true);
    expect(h.onTextInput).not.toHaveBeenCalled();
  });

  it("reports native editor focus without requesting viewport scrolling", async () => {
    const h = await editor("paragraph", "Clicked text", 0, "another-block");
    expect(h.onFocusBlock).toHaveBeenCalledExactlyOnceWith("block", true);
  });

  it.each([false, true])("opens and filters through input, then applies a heading (software Enter: %s)", async (softwareEnter) => {
    const h = await editor();
    expect(h.host.hasAttribute("data-placeholder")).toBe(true);
    expect(h.host.hasAttribute("aria-placeholder")).toBe(false);
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
    expect(h.host.hasAttribute("data-placeholder")).toBe(false);
    await h.input("h2");
    await vi.waitFor(() => expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(2));
    if (softwareEnter) expect((await h.input("", "insertParagraph")).defaultPrevented).toBe(true);
    else h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(h.onConvert).toHaveBeenCalledExactlyOnceWith("block", "heading_2", true);
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("pauses the menu during composition and restores search after the composed query commits", async () => {
    const h = await editor();
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
    h.host.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    await tick();
    expect(document.querySelector('[role="menu"]')).toBeNull();
    h.host.replaceChildren(document.createTextNode("/h2"));
    window.getSelection()?.collapse(h.host.firstChild, 3);
    h.host.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "h2" }));
    await vi.waitFor(() => expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(2));
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(h.onConvert).toHaveBeenCalledExactlyOnceWith("block", "heading_2", true);
  });

  it("dismisses the slash panel without selecting or focusing the whole block", async () => {
    const h = await editor();
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
    for (let attempt = 0; attempt < 2; attempt += 1) {
      h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await tick();
      expect(document.querySelector('[role="menu"]')).toBeNull();
      expect(h.blockSelection.selection).toBeNull();
      expect(h.row.hasAttribute("data-notes-block-selected")).toBe(false);
      expect(document.activeElement).toBe(h.host);
      expect(h.focusRow).not.toHaveBeenCalled();
    }
    await h.input("continue");
    expect(h.host.textContent).toBe("/continue");
  });

  it("routes both arrow keys to the slash menu before block navigation", async () => {
    const h = await editor();
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
    const menu = document.querySelector<HTMLElement>('[role="menu"]')!;
    const items = menu.querySelectorAll<HTMLElement>('[role^="menuitem"]');
    expect(items.length).toBeGreaterThan(1);
    await vi.waitFor(() => expect(h.host.getAttribute("aria-activedescendant")).toBeTruthy());
    expect(items[0].dataset.active).toBe("true");

    const up = new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true });
    h.host.dispatchEvent(up);
    await tick();
    expect(up.defaultPrevented).toBe(true);
    expect(h.navigationKeydown).not.toHaveBeenCalled();
    expect([...items].findIndex((item) => item.dataset.active === "true")).toBe(items.length - 1);
    expect(document.activeElement).toBe(h.host);

    const down = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    h.host.dispatchEvent(down);
    await tick();
    expect(down.defaultPrevented).toBe(true);
    expect(h.navigationKeydown).not.toHaveBeenCalled();
    expect(items[0].dataset.active).toBe("true");
    expect(document.activeElement).toBe(h.host);
  });

  it("retains literal slash text after dismissal and reopens after deleting the trigger", async () => {
    const h = await editor();
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await h.input("h");
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(h.host.textContent).toBe("/h");
    await h.input("", "deleteContentBackward");
    await h.input("", "deleteContentBackward");
    await h.input("/");
    await vi.waitFor(() => expect(document.querySelector('[role="menu"]')).not.toBeNull());
  });
});

describe("Notes list input", () => {
  it.each(["paragraph", "bulleted_list_item", "numbered_list_item"] as const)("outdents %s before removing markers or text with hardware and software Backspace", async (type) => {
    const h = await editor(type, "Keep text", 3);
    restoreNotesEditableSelection(h.host, { start: 0, end: 0 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true }));
    expect(h.onKeyboardAction).toHaveBeenLastCalledWith("block", { type: "outdent", preventDefault: true, selection: { start: 0, end: 0 } });
    await h.input("", "deleteContentBackward");
    expect(h.onKeyboardAction).toHaveBeenLastCalledWith("block", { type: "outdent", preventDefault: true, selection: { start: 0, end: 0 } });
    expect(h.host.textContent).toBe("Keep text");
  });

  it.each(["Tab", "Unidentified"])("indents and unindents code text without moving its block (key: %s)", async (key) => {
    const h = await editor("code", "value");
    restoreNotesEditableSelection(h.host, { start: 0, end: 0 });
    h.host.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true }));
    await tick();
    expect(h.host.textContent).toBe("\tvalue");
    const outdent = new KeyboardEvent("keydown", { key, code: "Tab", shiftKey: true, bubbles: true, cancelable: true });
    h.host.dispatchEvent(outdent);
    expect(outdent.defaultPrevented).toBe(true);
    await tick();
    expect(h.host.textContent).toBe("value");
    expect(h.onKeyboardAction).not.toHaveBeenCalled();
  });

  it.each([false, true])("handles Tab with Shift=%s without browser focus navigation", async (shiftKey) => {
    const h = await editor("bulleted_list_item", "Keep this text");
    restoreNotesEditableSelection(h.host, { start: 2, end: 5 });
    const event = new KeyboardEvent("keydown", { key: "Tab", shiftKey, bubbles: true, cancelable: true });
    h.host.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(h.onKeyboardAction).toHaveBeenLastCalledWith("block", {
      type: shiftKey ? "outdent" : "nest", preventDefault: true, selection: { start: 2, end: 5 },
    });
    expect(document.activeElement).toBe(h.host);
    expect(h.focusRow).not.toHaveBeenCalled();
  });

  it.each([
    { type: "paragraph", depth: 0 },
    { type: "paragraph", depth: 3 },
    { type: "bulleted_list_item", depth: 0 },
    { type: "bulleted_list_item", depth: 3 },
    { type: "numbered_list_item", depth: 3 },
  ] as const)("routes WebKitGTK Shift+Tab in $type at depth $depth without moving the caret", async ({ type, depth }) => {
    const h = await editor(type, "Keep this text", depth);
    const selection = { start: 2, end: 5 };
    restoreNotesEditableSelection(h.host, selection);
    const event = new KeyboardEvent("keydown", {
      key: "Unidentified", code: "Tab", shiftKey: true, bubbles: true, cancelable: true,
    });
    h.host.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(h.onKeyboardAction).toHaveBeenCalledExactlyOnceWith("block", {
      type: "outdent", preventDefault: true, selection,
    });
    expect(notesTextSelectionFromEditableRoot(h.host)).toEqual(selection);
    expect(document.activeElement).toBe(h.host);
    expect(h.focusRow).not.toHaveBeenCalled();
    expect(h.host.textContent).toBe("Keep this text");
  });

  it.each(["numbered_list_item", "bulleted_list_item", "to_do"] as const)(
    "removes %s formatting through hardware and software Backspace without deleting text", async (type) => {
      const h = await editor(type, "Keep this text");
      restoreNotesEditableSelection(h.host, { start: 0, end: 0 });
      const key = new KeyboardEvent("keydown", { key: "Backspace", bubbles: true, cancelable: true });
      h.host.dispatchEvent(key);
      expect(key.defaultPrevented).toBe(true);
      expect(h.onKeyboardAction).toHaveBeenLastCalledWith("block", { type: "remove_block_format", preventDefault: true });
      h.onKeyboardAction.mockClear();
      const input = await h.input("", "deleteContentBackward");
      expect(input.defaultPrevented).toBe(true);
      expect(h.onKeyboardAction).toHaveBeenCalledExactlyOnceWith("block", { type: "remove_block_format", preventDefault: true });
      expect(h.host.textContent).toBe("Keep this text");
    },
  );

  it("deletes selected list text without stripping its list formatting", async () => {
    const h = await editor("numbered_list_item", "Keep this text");
    restoreNotesEditableSelection(h.host, { start: 0, end: 5 });
    await h.input("", "deleteContentBackward");
    expect(h.onKeyboardAction).not.toHaveBeenCalled();
    expect(h.host.textContent).toBe("this text");
  });
});


describe("Notes native clipboard events", () => {
  it.each(["copy", "cut"])("exports a partial heading as semantic HTML on %s", async (type) => {
    const h = await editor("heading_2", "before selected after");
    restoreNotesEditableSelection(h.host, { start: 7, end: 15 });
    const setData = vi.fn();
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData } });
    h.host.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(true);
    expect(setData).toHaveBeenCalledWith("text/plain", "## selected");
    expect(setData).toHaveBeenCalledWith("text/html", "<h2>selected</h2>");
    expect(h.host.textContent).toBe(type === "cut" ? "before  after" : "before selected after");
  });

  it("retains text when a native cut cannot write the clipboard", async () => {
    const h = await editor("paragraph", "keep this");
    restoreNotesEditableSelection(h.host, { start: 0, end: 9 });
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    try {
      const event = new Event("cut", { bubbles: true, cancelable: true });
      h.host.dispatchEvent(event);
      await tick();
      expect(event.defaultPrevented).toBe(true);
      expect(h.host.textContent).toBe("keep this");
      expect(warning).toHaveBeenCalled();
    } finally { warning.mockRestore(); }
  });
});


describe("Notes clipboard format routing", () => {
  it("reconciles paired Markdown and HTML headings before passing native paste to the store", async () => {
    const h = await editor();
    h.onPasteRichHtml.mockReturnValue(true);
    restoreNotesEditableSelection(h.host, { start: 0, end: 0 });
    const event = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: {
      getData: (type: string) => type === "text/plain" ? "# Title" : "<h2>Title</h2>",
    } });
    h.host.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(true);
    expect(h.onPasteRichHtml).toHaveBeenCalledExactlyOnceWith("block", 0, 0, "<h1>Title</h1>");
  });
});
