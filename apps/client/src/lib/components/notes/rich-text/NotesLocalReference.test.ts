// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDatabaseMentionRichText, createPageMentionRichText } from "$lib/notes/rich-text/core";
import type { NotesDatabaseMentionRichText, NotesDatabaseReference, NotesPageMentionRichText } from "$lib/notes/types";
import NotesLocalReference from "./NotesLocalReference.svelte";

const PAGE_ID = "11111111-1111-4111-8111-111111111111";
const DATABASE_ID = "33333333-3333-4333-8333-333333333333";
const SOURCE_BLOCK_ID = "44444444-4444-4444-8444-444444444444";
const SOURCE_PAGE_ID = "55555555-5555-4555-8555-555555555555";
const PAGE_URL = `#notes?page=${PAGE_ID}`;
const DATABASE_URL = `${PAGE_URL}&block=${DATABASE_ID}`;
const DATABASE_METADATA: NotesDatabaseReference = {
  block_id: DATABASE_ID,
  page_id: PAGE_ID,
  title: "Project tasks",
  source_block_id: SOURCE_BLOCK_ID,
  source_page_id: SOURCE_PAGE_ID,
  is_linked: true,
  owned_data_source_count: 0,
  editing_locked: false,
};
const PAGE_LOCATION = ["Personal", "Website", "Plans", "", "Current project"];

const { getDatabase, getLocation, openLink } = vi.hoisted(() => ({
  getDatabase: vi.fn<(blockId: string) => Promise<NotesDatabaseReference>>(),
  getLocation: vi.fn<(pageId: string) => Promise<string[]>>(),
  openLink: vi.fn<(url: string) => Promise<void>>(),
}));
vi.mock("$lib/api/notes", () => ({ getNotesDatabaseReference: getDatabase, getNotesPageLocation: getLocation }));
vi.mock("$lib/notes/links/navigation", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/notes/links/navigation")>(), openNotesTextLink: openLink,
}));

let component: ReturnType<typeof mount> | undefined;

beforeEach(() => {
  vi.useFakeTimers();
  getDatabase.mockReset().mockResolvedValue(DATABASE_METADATA);
  getLocation.mockReset().mockResolvedValue(PAGE_LOCATION);
  openLink.mockReset().mockResolvedValue(undefined);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  document.getSelection()?.removeAllRanges();
  document.body.replaceChildren();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** Mount a local reference in a preview or an editor with the same semantic anchor. */
async function mountReference(
  reference: NotesPageMentionRichText | NotesDatabaseMentionRichText = createPageMentionRichText(PAGE_ID, "Project"),
  editable = false,
): Promise<{ host: HTMLDivElement; anchor: HTMLAnchorElement }> {
  const host = document.createElement("div");
  if (editable) { host.setAttribute("contenteditable", "true"); host.setAttribute("data-notes-link-actions", ""); }
  document.body.append(host);
  component = mount(NotesLocalReference, { target: host, props: { reference, text: reference.plain_text } });
  await tick();
  const anchor = host.querySelector("a");
  if (!(anchor instanceof HTMLAnchorElement)) throw new Error("Expected a local reference anchor");
  return { host, anchor };
}

/** Supply pointer intent in jsdom without requiring a native PointerEvent implementation. */
function pointerEvent(type: string, pointerType = "mouse", options: MouseEventInit = {}): MouseEvent {
  const event = new MouseEvent(type, { bubbles: true, cancelable: true, ...options });
  Object.defineProperty(event, "pointerType", { value: pointerType });
  return event;
}

/** Finish metadata work that may be started by a lazily imported API module. */
async function settleMetadata(): Promise<void> {
  await vi.dynamicImportSettled();
  await tick();
}

async function hoverReference(anchor: HTMLAnchorElement): Promise<void> {
  anchor.dispatchEvent(pointerEvent("pointerenter"));
  await vi.advanceTimersByTimeAsync(350);
  await settleMetadata();
}

function previewCard(): HTMLDivElement {
  const card = document.querySelector("[data-notes-mention-preview]");
  if (!(card instanceof HTMLDivElement)) throw new Error("Expected a reference preview");
  return card;
}

/** The muted route above a preview's title, read from its full screen reader copy. */
function previewRoute(card: HTMLElement): string | undefined {
  return card.querySelector(".sr-only")?.textContent ?? undefined;
}

function previewTitle(card: HTMLElement): string | undefined {
  return card.querySelector("[data-notes-mention-preview-title]")?.textContent ?? undefined;
}

describe("inline local reference navigation", () => {
  it("builds page links from identity and navigates once without eager metadata or parent link actions", async () => {
    const { host, anchor } = await mountReference(createPageMentionRichText(PAGE_ID, "Project", "https://example.com/stale"));
    const parentClick = vi.fn();
    host.addEventListener("click", parentClick);
    expect(anchor.getAttribute("href")).toBe(PAGE_URL);
    expect(anchor.dataset.notesReferenceId).toBe(PAGE_ID);
    expect(anchor.hasAttribute("data-notes-link-url")).toBe(false);
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    expect(getDatabase).not.toHaveBeenCalled();
    expect(getLocation).not.toHaveBeenCalled();
    anchor.click();
    await settleMetadata();
    expect(openLink).toHaveBeenCalledExactlyOnceWith(PAGE_URL);
    expect(parentClick).not.toHaveBeenCalled();
    expect(getDatabase).not.toHaveBeenCalled();
    expect(getLocation).not.toHaveBeenCalled();
  });

  it("opens a database's provided owner link without loading preview metadata", async () => {
    const mention = createDatabaseMentionRichText(DATABASE_ID, "Tasks");
    mention.href = DATABASE_URL;
    const { anchor } = await mountReference(mention, true);
    expect(anchor.getAttribute("href")).toBe(DATABASE_URL);
    anchor.click();
    await settleMetadata();
    expect(openLink).toHaveBeenCalledExactlyOnceWith(DATABASE_URL);
    expect(getDatabase).not.toHaveBeenCalled();
    expect(getLocation).not.toHaveBeenCalled();
  });

  it("follows touch taps with finger movement and resolves the referenced database view without a hover card", async () => {
    const { anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"));
    expect(anchor.getAttribute("href")).toBe(`#notes?block=${DATABASE_ID}`);
    anchor.dispatchEvent(pointerEvent("pointerenter", "touch"));
    await vi.advanceTimersByTimeAsync(350);
    expect(getDatabase).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    anchor.dispatchEvent(pointerEvent("pointerdown", "touch", { clientX: 20, clientY: 30 }));
    anchor.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: 28, clientY: 34 }));
    await settleMetadata();
    expect(getDatabase).toHaveBeenCalledExactlyOnceWith(DATABASE_ID);
    expect(openLink).toHaveBeenCalledExactlyOnceWith(DATABASE_URL);
    expect(getLocation).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
  });

  it("handles focused Enter before an editor's capture handler and preserves ordinary editor keys", async () => {
    const { host, anchor } = await mountReference(undefined, true);
    const parentKeydown = vi.fn();
    host.addEventListener("keydown", parentKeydown, true);
    anchor.focus();
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    anchor.dispatchEvent(enter);
    await settleMetadata();
    expect(enter.defaultPrevented).toBe(true);
    expect(parentKeydown).not.toHaveBeenCalled();
    expect(openLink).toHaveBeenCalledExactlyOnceWith(PAGE_URL);
    anchor.blur();
    host.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    expect(parentKeydown).toHaveBeenCalledTimes(1);
    expect(openLink).toHaveBeenCalledTimes(1);
  });

  it("preserves native text dragging and does not navigate or preview selected text", async () => {
    const { anchor } = await mountReference(undefined, true);
    const selection = document.getSelection();
    const range = document.createRange();
    range.selectNodeContents(anchor);
    selection?.addRange(range);
    anchor.dispatchEvent(pointerEvent("pointerdown", "mouse", { clientX: 10, clientY: 10 }));
    anchor.dispatchEvent(pointerEvent("pointermove", "mouse", { clientX: 30, clientY: 10, buttons: 1 }));
    const click = new MouseEvent("click", { bubbles: true, cancelable: true, clientX: 30, clientY: 10 });
    anchor.dispatchEvent(click);
    await hoverReference(anchor);
    expect(click.defaultPrevented).toBe(true);
    expect(selection?.toString()).toBe("Project");
    expect(openLink).not.toHaveBeenCalled();
    expect(getLocation).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
  });

  it("does not follow a pointer drag even if the browser leaves selection collapsed", async () => {
    const { anchor } = await mountReference();
    anchor.dispatchEvent(pointerEvent("pointerdown", "mouse", { clientX: 10, clientY: 10 }));
    anchor.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: 40, clientY: 10 }));
    await settleMetadata();
    expect(openLink).not.toHaveBeenCalled();
    expect(getDatabase).not.toHaveBeenCalled();
  });

  it("reports a missing database destination and allows the next click to retry", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    getDatabase.mockRejectedValueOnce(new Error("Database was deleted"));
    const { anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"));
    anchor.click();
    await settleMetadata();
    expect(previewCard().querySelector("[role='status']")?.textContent).toContain("This link could not be opened");
    expect(openLink).not.toHaveBeenCalled();
    anchor.click();
    await settleMetadata();
    expect(getDatabase).toHaveBeenCalledTimes(2);
    expect(openLink).toHaveBeenCalledExactlyOnceWith(DATABASE_URL);
  });
});

describe("local reference previews", () => {
  it("loads database title and owner location after hover intent, outside the editor", async () => {
    const { host, anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"), true);
    anchor.dispatchEvent(pointerEvent("pointerenter"));
    await vi.advanceTimersByTimeAsync(250);
    expect(getDatabase).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    await vi.advanceTimersByTimeAsync(100);
    await settleMetadata();
    const card = previewCard();
    expect(getDatabase).toHaveBeenCalledExactlyOnceWith(DATABASE_ID);
    expect(getLocation).toHaveBeenCalledExactlyOnceWith(PAGE_ID);
    expect(previewTitle(card)).toBe("Project tasks");
    expect(previewRoute(card)).toBe("Personal / Website / Plans / Untitled / Current project");
    expect(card.textContent).not.toContain("Database");
    expect(card.querySelector(".notes-reference-database-thumbnail")?.getAttribute("aria-hidden")).toBe("true");
    expect(card.querySelector("table")).toBeNull();
    expect(host.contains(card)).toBe(false);
    expect(anchor.getAttribute("aria-describedby")).toBe(card.id);
    expect(anchor.getAttribute("href")).toBe(DATABASE_URL);
    expect(anchor.textContent).toBe("Tasks");
  });

  it("splits a page title from its location without fetching database data", async () => {
    const { anchor } = await mountReference();
    await hoverReference(anchor);
    const card = previewCard();
    expect(getLocation).toHaveBeenCalledExactlyOnceWith(PAGE_ID);
    expect(getDatabase).not.toHaveBeenCalled();
    expect(previewTitle(card)).toBe("Current project");
    expect(previewRoute(card)).toBe("Personal / Website / Plans / Untitled");
    expect(card.textContent).not.toContain("Note");
    const title = card.querySelector("[data-notes-mention-preview-title]");
    expect(title && card.querySelector(".sr-only")?.compareDocumentPosition(title)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(card.querySelector(".notes-reference-database-thumbnail")).toBeNull();
    expect(anchor.textContent).toBe("Project");
  });

  it("keeps the preview available while crossing the gap to the card and caches its metadata", async () => {
    const { anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"));
    await hoverReference(anchor);
    anchor.dispatchEvent(pointerEvent("pointerleave"));
    previewCard().dispatchEvent(pointerEvent("pointerenter"));
    await vi.advanceTimersByTimeAsync(200);
    const card = previewCard();
    card.dispatchEvent(pointerEvent("pointerleave"));
    await vi.advanceTimersByTimeAsync(200);
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    await hoverReference(anchor);
    expect(previewCard().textContent).toContain("Project tasks");
    expect(getDatabase).toHaveBeenCalledTimes(1);
    expect(getLocation).toHaveBeenCalledTimes(1);
  });

  it("shares a pending hover lookup with click navigation", async () => {
    let resolveMetadata: ((value: NotesDatabaseReference) => void) | undefined;
    getDatabase.mockImplementation(() => new Promise<NotesDatabaseReference>((resolve) => { resolveMetadata = resolve; }));
    const { anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"));
    await hoverReference(anchor);
    expect(getDatabase).toHaveBeenCalledTimes(1);
    anchor.click();
    await settleMetadata();
    expect(getDatabase).toHaveBeenCalledTimes(1);
    resolveMetadata?.(DATABASE_METADATA);
    await settleMetadata();
    expect(openLink).toHaveBeenCalledExactlyOnceWith(DATABASE_URL);
  });

  it("cancels a brief hover and releases a pending hover timeout on unmount", async () => {
    const { anchor } = await mountReference();
    anchor.dispatchEvent(pointerEvent("pointerenter"));
    anchor.dispatchEvent(pointerEvent("pointerleave"));
    await vi.advanceTimersByTimeAsync(400);
    expect(getLocation).not.toHaveBeenCalled();
    anchor.dispatchEvent(pointerEvent("pointerenter"));
    if (component) await unmount(component);
    component = undefined;
    await vi.advanceTimersByTimeAsync(400);
    expect(getLocation).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
  });

  it("discards late metadata after unmount without loading another page", async () => {
    let resolveMetadata: ((value: NotesDatabaseReference) => void) | undefined;
    getDatabase.mockImplementation(() => new Promise<NotesDatabaseReference>((resolve) => { resolveMetadata = resolve; }));
    const { anchor } = await mountReference(createDatabaseMentionRichText(DATABASE_ID, "Tasks"));
    await hoverReference(anchor);
    if (component) await unmount(component);
    component = undefined;
    resolveMetadata?.(DATABASE_METADATA);
    await settleMetadata();
    expect(getLocation).not.toHaveBeenCalled();
    expect(openLink).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
  });

  it("dismisses on Escape without forwarding the key to the editor", async () => {
    const { host, anchor } = await mountReference(undefined, true);
    const parentKeydown = vi.fn();
    host.addEventListener("keydown", parentKeydown, true);
    anchor.focus();
    await hoverReference(anchor);
    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    anchor.dispatchEvent(escape);
    await tick();
    expect(escape.defaultPrevented).toBe(true);
    expect(parentKeydown).not.toHaveBeenCalled();
    expect(document.querySelector("[role='tooltip']")).toBeNull();
    expect(anchor.hasAttribute("aria-describedby")).toBe(false);
    expect(openLink).not.toHaveBeenCalled();
  });

  it("clamps the floating card near viewport edges and repositions on resize", async () => {
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(200);
    const { anchor } = await mountReference();
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(window.innerWidth - 30, window.innerHeight - 35, 60, 20));
    await hoverReference(anchor);
    const card = previewCard();
    expect(Number.parseFloat(card.style.left) + Number.parseFloat(card.style.width)).toBeLessThan(window.innerWidth);
    expect(Number.parseFloat(card.style.top)).toBeLessThan(window.innerHeight - 200);
    vi.stubGlobal("innerWidth", 240);
    vi.stubGlobal("innerHeight", 180);
    window.dispatchEvent(new Event("resize"));
    await tick();
    expect(Number.parseFloat(card.style.left)).toBeGreaterThanOrEqual(0);
    expect(Number.parseFloat(card.style.width)).toBeLessThan(240);
    expect(Number.parseFloat(card.style.top) + Number.parseFloat(card.style.maxHeight)).toBeLessThanOrEqual(180);
    vi.unstubAllGlobals();
  });

  it("shows an unavailable preview without blocking a valid page link", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    getLocation.mockRejectedValueOnce(new Error("Ancestry unavailable"));
    const { anchor } = await mountReference();
    await hoverReference(anchor);
    expect(previewCard().querySelector("[role='status']")?.textContent).toBe("No preview");
    anchor.click();
    await settleMetadata();
    expect(openLink).toHaveBeenCalledExactlyOnceWith(PAGE_URL);
  });
});
