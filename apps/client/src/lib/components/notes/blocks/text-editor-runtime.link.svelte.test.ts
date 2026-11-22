// @vitest-environment jsdom
import { flushSync } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createBlockWrite } from "$lib/notes/blocks/factory";
import { restoreNotesEditableSelection } from "$lib/notes/editor/selection";
import type { NotesLinkOpener } from "$lib/notes/links/navigation";
import type { NotesBlock } from "$lib/notes/types";
import { NotesTextEditorController, type NotesTextEditorControllerSource } from "./text-editor-runtime.svelte";

const openLink = vi.hoisted(() => vi.fn<(url: string, notes: NotesLinkOpener) => Promise<void>>());
vi.mock("$lib/notes/links/navigation", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/notes/links/navigation")>(), openNotesTextLink: openLink,
}));

const cleanups: (() => void)[] = [];

afterEach(() => {
  while (cleanups.length) cleanups.pop()?.();
  openLink.mockReset();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

/** A promise with its settle functions exposed, for a link open that finishes when the test says so. */
function deferred(): { promise: Promise<void>; resolve: () => void; reject: (error: Error) => void } {
  let resolve: () => void = () => {};
  let reject: (error: Error) => void = () => {};
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

/** A paragraph controller over a plain editable host. */
function linkController() {
  const block = {
    ...createBlockWrite("block", "paragraph", "Alpha beta gamma"), object: "block", parent: { type: "page_id", page_id: "page" },
    created_time: "", last_edited_time: "", has_children: false, in_trash: false,
    source_provider: null, source_object_id: null, source_last_edited_time: null,
  } as NotesBlock;
  const source = {
    block: () => block,
    focusBlockId: () => null,
    focusRequestId: () => 0,
    focusSelection: () => null,
    mentionTargets: () => [],
    translate: (key: string) => key,
    notes: { openNotesLink: async () => true },
  } as unknown as NotesTextEditorControllerSource;
  const host = document.createElement("div");
  host.contentEditable = "true";
  host.textContent = "Alpha beta gamma";
  document.body.append(host);
  let controller: NotesTextEditorController | undefined;
  cleanups.push($effect.root(() => { controller = new NotesTextEditorController(source); }));
  return { controller: controller!, host };
}

/** Selects a range of the host text and opens the link form on it, as the link shortcut does. */
function openLinkForm(controller: NotesTextEditorController, host: HTMLElement, start: number, end: number): void {
  restoreNotesEditableSelection(host, { start, end });
  expect(controller.openLinkEditorFromEditor(host)).toBe(true);
  flushSync();
}

/** Opens a link from the form on the first word and returns the pending open. */
function startOpeningLink(controller: NotesTextEditorController, host: HTMLElement) {
  const open = deferred();
  openLink.mockReturnValueOnce(open.promise);
  openLinkForm(controller, host, 0, 5);
  controller.updateLinkInput("https://example.com/alpha");
  const opening = controller.openLinkDestination();
  expect(openLink).toHaveBeenCalledWith("https://example.com/alpha", expect.anything());
  return { open, opening };
}

describe("Notes text link actions", () => {
  it("does not show an earlier link open failure on a form opened on other text", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const { controller, host } = linkController();
    const { open, opening } = startOpeningLink(controller, host);

    controller.cancelLinkEditor();
    openLinkForm(controller, host, 11, 16);
    open.reject(new Error("unavailable"));
    await opening;
    flushSync();

    expect(controller.linkEditorOpen).toBe(true);
    expect(controller.linkMode).toBe("edit");
    expect(controller.linkRange).toMatchObject({ start: 11, end: 16 });
    expect(controller.linkError).toBeNull();
  });

  it("keeps a form opened on other text when an earlier link open finishes", async () => {
    const { controller, host } = linkController();
    const { open, opening } = startOpeningLink(controller, host);

    controller.cancelLinkEditor();
    openLinkForm(controller, host, 11, 16);
    open.resolve();
    await opening;
    flushSync();

    expect(controller.linkEditorOpen).toBe(true);
    expect(controller.linkRange).toMatchObject({ start: 11, end: 16 });
  });

  it("shows a link open failure on the panel it was opened from", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const { controller, host } = linkController();
    const { open, opening } = startOpeningLink(controller, host);

    open.reject(new Error("unavailable"));
    await opening;
    flushSync();

    expect(controller.linkEditorOpen).toBe(true);
    expect(controller.linkError).toBe("notes.linkOpenFailed");
  });
});
