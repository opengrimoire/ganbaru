// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { notesHeldEditNotice } from "$lib/stores/notes/held-edit-notice.svelte";
import NotesHeldEditToast from "./NotesHeldEditToast.svelte";

const writeTextToClipboard = vi.hoisted(() => vi.fn<(value: string) => Promise<void>>());
vi.mock("$lib/utils/clipboard", () => ({ writeTextToClipboard: (value: string) => writeTextToClipboard(value) }));

let component: ReturnType<typeof mount> | undefined;

afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  notesHeldEditNotice.dismiss();
  writeTextToClipboard.mockReset();
  document.body.replaceChildren();
});

/** A promise with its settle functions exposed, for a copy that finishes when the test says so. */
function deferred(): { promise: Promise<void>; resolve: () => void; reject: (error: Error) => void } {
  let resolve: () => void = () => {};
  let reject: (error: Error) => void = () => {};
  const promise = new Promise<void>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

/** Clicks the copy action of the toast. */
function clickCopy(): void {
  const action = [...document.querySelectorAll<HTMLButtonElement>("button")].find((button) => !button.querySelector(".sr-only"));
  action?.click();
}

describe("Notes held edit toast", () => {
  it("keeps removed text appended while an earlier copy ran", async () => {
    const copy = deferred();
    writeTextToClipboard.mockReturnValueOnce(copy.promise);
    notesHeldEditNotice.show({ kind: "removed", text: "first" });
    component = mount(NotesHeldEditToast, { target: document.body });
    flushSync();
    clickCopy();
    expect(writeTextToClipboard).toHaveBeenCalledWith("first");

    notesHeldEditNotice.show({ kind: "removed", text: "second" });
    copy.resolve();
    await copy.promise;
    await tick();

    expect(notesHeldEditNotice.current).toEqual({ kind: "removed", text: "first\n\nsecond" });
  });

  it("does not show an earlier copy failure on text it did not copy", async () => {
    const copy = deferred();
    writeTextToClipboard.mockReturnValueOnce(copy.promise);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    notesHeldEditNotice.show({ kind: "removed", text: "first" });
    component = mount(NotesHeldEditToast, { target: document.body });
    flushSync();
    const removedMessage = document.body.textContent ?? "";
    clickCopy();

    notesHeldEditNotice.dismiss();
    notesHeldEditNotice.show({ kind: "removed", text: "later" });
    copy.reject(new Error("denied"));
    await copy.promise.catch(() => {});
    await tick();

    expect(document.body.textContent).toBe(removedMessage);
    warn.mockRestore();
  });

  it("dismisses the notice after copying the text it shows", async () => {
    writeTextToClipboard.mockResolvedValueOnce(undefined);
    notesHeldEditNotice.show({ kind: "removed", text: "first" });
    component = mount(NotesHeldEditToast, { target: document.body });
    flushSync();
    clickCopy();
    await tick();
    await tick();

    expect(notesHeldEditNotice.current).toBeNull();
  });
});
