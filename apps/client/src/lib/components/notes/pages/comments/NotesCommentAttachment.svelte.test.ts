// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  openNotesExternalUrl: vi.fn(async (_url: string) => undefined),
}));

vi.mock("$lib/api/notes/knowledge", () => ({ openNotesExternalUrl: api.openNotesExternalUrl }));
vi.mock("$lib/api/notes/file-assets", () => ({
  notesFileAssetUrl: async (path: string) => path,
  releaseNotesFileAssetUrl: () => undefined,
}));

/** Builds an external file attachment with the given name and link. */
function externalAttachment(name: string, url: string): Record<string, unknown> {
  return { type: "file", name, file: { url } };
}

describe("NotesCommentAttachment", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    api.openNotesExternalUrl.mockReset();
  });

  function errorText(): string | null {
    return target?.querySelector(".notice-message")?.textContent?.trim() ?? null;
  }

  it("does not show a failed open under the attachment that takes its slot", async () => {
    const props = $state({ attachment: externalAttachment("first.pdf", "https://example.com/first.pdf") });
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesCommentAttachment } = await import("./NotesCommentAttachment.svelte");
    component = mount(NotesCommentAttachment, { target, props });
    flushSync();
    api.openNotesExternalUrl.mockRejectedValueOnce(new Error("denied"));

    target.querySelector<HTMLButtonElement>('button[title="https://example.com/first.pdf"]')?.click();
    await tick();
    await tick();
    expect(errorText()).toBe("The attachment could not be opened.");

    props.attachment = externalAttachment("second.pdf", "https://example.com/second.pdf");
    flushSync();

    expect(errorText()).toBeNull();
  });

  it("keeps the latest open's outcome when an earlier open fails later", async () => {
    const props = $state({ attachment: externalAttachment("first.pdf", "https://example.com/first.pdf") });
    let failFirst = (): void => undefined;
    api.openNotesExternalUrl
      .mockImplementationOnce(() => new Promise<undefined>((_resolve, reject) => { failFirst = () => reject(new Error("denied")); }))
      .mockResolvedValueOnce(undefined);
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesCommentAttachment } = await import("./NotesCommentAttachment.svelte");
    component = mount(NotesCommentAttachment, { target, props });
    flushSync();
    const open = target.querySelector<HTMLButtonElement>('button[title="https://example.com/first.pdf"]');

    open?.click();
    open?.click();
    await tick();
    failFirst();
    await tick();
    await tick();

    expect(api.openNotesExternalUrl).toHaveBeenCalledTimes(2);
    expect(errorText()).toBeNull();
  });
});
