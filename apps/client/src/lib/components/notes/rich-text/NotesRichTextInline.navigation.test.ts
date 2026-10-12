// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { applyRichTextLink, createTextRichText } from "$lib/notes/rich-text/core";
import NotesRichTextInline from "./NotesRichTextInline.svelte";

const openLink = vi.hoisted(() => vi.fn(async (_url: string) => undefined));
vi.mock("$lib/notes/links/navigation", async (importOriginal) => ({
  ...await importOriginal<typeof import("$lib/notes/links/navigation")>(), openNotesTextLink: openLink,
}));
let component: ReturnType<typeof mount> | undefined;
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  openLink.mockClear();
  document.body.replaceChildren();
});

/** Mount the shared renderer in either a preview or a simple table editing host. */
async function linkedText(editable = false): Promise<HTMLAnchorElement> {
  const host = document.createElement("div");
  if (editable) host.setAttribute("contenteditable", "true");
  document.body.append(host);
  component = mount(NotesRichTextInline, { target: host, props: {
    richText: applyRichTextLink([createTextRichText("Tasks")], 0, 5, "https://example.com/tasks"),
  } });
  await tick();
  return host.querySelector<HTMLAnchorElement>("a")!;
}

describe("shared rich-text link navigation", () => {
  it("opens a read-only link with click or keyboard activation", async () => {
    const link = await linkedText();
    link.click();
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledExactlyOnceWith("https://example.com/tasks"));
    link.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledTimes(2));
  });

  it("opens modifier clicks in table editing hosts without navigating the webview", async () => {
    const link = await linkedText(true);
    const event = new MouseEvent("click", { ctrlKey: true, bubbles: true, cancelable: true });
    link.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(openLink).toHaveBeenCalledExactlyOnceWith("https://example.com/tasks"));
  });

  it("preserves selected text and editing Enter in table hosts", async () => {
    const link = await linkedText(true);
    const range = document.createRange();
    range.selectNodeContents(link);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    link.click();
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    link.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(false);
    expect(openLink).not.toHaveBeenCalled();
  });
});
