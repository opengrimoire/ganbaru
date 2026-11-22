// @vitest-environment jsdom

import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getQuickNote, QuickNoteWriteError, updateQuickNote } from "$lib/api/quick-notes";
import type { QuickNote } from "$lib/quick-notes/types";
import { lightTheme } from "$lib/themes";
import QuickNoteEditorModal from "./QuickNoteEditorModal.svelte";

vi.mock("$lib/api/quick-notes", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/api/quick-notes")>(),
  getQuickNote: vi.fn(),
  updateQuickNote: vi.fn(),
}));

vi.mock("$lib/quick-notes/window-sync", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/quick-notes/window-sync")>(),
  listenForQuickNotesChanges: () => Promise.resolve(() => undefined),
  publishQuickNotesChanged: () => undefined,
}));

const note: QuickNote = {
  id: "note-a",
  title: "Groceries",
  bodyPlainText: "",
  runs: [],
  previewTruncated: false,
  images: [],
  color: 0,
  tagId: null,
  pinned: false,
  archived: false,
  trashedAt: null,
  revision: 1,
  createdAt: "2026-11-20T10:00:00.000Z",
  updatedAt: "2026-11-20T10:00:00.000Z",
  hasConflict: false,
};

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;

function titleInput(): HTMLInputElement {
  const input = target.querySelector<HTMLInputElement>('input[aria-label="Title"]')
    ?? target.querySelector<HTMLInputElement>("input[type='text']");
  if (!input) throw new Error("No title input");
  return input;
}

function typeTitle(value: string): void {
  const input = titleInput();
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function buttonWithText(text: string): HTMLButtonElement {
  const button = [...document.querySelectorAll<HTMLButtonElement>("button")]
    .find((entry) => entry.textContent?.trim() === text);
  if (!button) throw new Error(`No button ${text}`);
  return button;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("ResizeObserver", class { observe(): void {} unobserve(): void {} disconnect(): void {} });
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: false, media: query, addEventListener: () => undefined, removeEventListener: () => undefined }));
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  target.remove();
  document.body.replaceChildren();
  vi.resetAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("QuickNoteEditorModal", () => {
  it("keeps an edit typed while a conflict reload reads the note", async () => {
    const remote: QuickNote = { ...note, title: "Remote title", revision: 2 };
    vi.mocked(updateQuickNote).mockRejectedValue(new QuickNoteWriteError("revision_conflict", "conflict"));
    vi.mocked(getQuickNote).mockResolvedValue(remote);
    component = mount(QuickNoteEditorModal, {
      target,
      props: {
        note,
        tags: [],
        defaultTagId: null,
        defaultColor: 0,
        untaggedColor: 0,
        onCreateTag: () => Promise.reject(new Error("unused")),
        theme: lightTheme,
        onClose: () => undefined,
        onSaved: () => undefined,
        onArchive: () => undefined,
        onUnarchive: () => undefined,
        onTrash: () => undefined,
        onRestore: () => undefined,
        onDelete: () => undefined,
      },
    });
    flushSync();

    typeTitle("Local title");
    // Closing saves at once and keeps the editor open on the conflict.
    buttonWithText("Close").click();
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(buttonWithText("Reload")).toBeTruthy();

    let finishReload: (read: QuickNote) => void = () => undefined;
    vi.mocked(getQuickNote).mockImplementation(() => new Promise<QuickNote>((resolve) => { finishReload = resolve; }));
    buttonWithText("Reload").click();
    typeTitle("Typed during reload");

    finishReload(remote);
    await Promise.resolve();
    await Promise.resolve();
    flushSync();
    expect(titleInput().value).toBe("Typed during reload");
  });
});
