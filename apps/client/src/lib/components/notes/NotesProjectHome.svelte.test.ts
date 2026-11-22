// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotesFolder } from "$lib/notes/types";

const backend = vi.hoisted(() => {
  const pending = new Promise<never>(() => undefined);
  const folderRequests: Array<{ reject: (error: Error) => void }> = [];
  return { pending, folderRequests };
});

vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tauri-apps/api/window")>();
  return {
    ...actual,
    getCurrentWindow: () => ({ label: "main" }),
  };
});

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    listNotesWorkingMarkdown: () => backend.pending,
    createNotesFolder: () => new Promise<NotesFolder>((_resolve, reject) => {
      backend.folderRequests.push({ reject });
    }),
  };
});

/** Lets rejected store promises settle and the component rerender. */
async function settle(): Promise<void> {
  for (let index = 0; index < 4; index += 1) await Promise.resolve();
  await tick();
}

describe("NotesProjectHome folder action errors", () => {
  let target: HTMLDivElement | undefined;
  let component: ReturnType<typeof mount> | undefined;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = undefined;
    target = undefined;
    backend.folderRequests.length = 0;
  });

  /** Mounts the explorer with a project prop the test can change. */
  async function mountHome(initialProjectId: string) {
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesProjectHome } = await import("./NotesProjectHome.svelte");
    const props = $state({
      projectId: initialProjectId,
      onCreationFolderChange: () => undefined,
      onSelectWorkingMarkdownFile: () => undefined,
      onBeforeDocumentNavigation: () => Promise.resolve(true),
    });
    component = mount(NotesProjectHome, { target, props });
    await tick();
    return props;
  }

  /** Clicks the new folder button in the explorer header. */
  function clickNewFolder(): void {
    const button = target?.querySelector<HTMLButtonElement>('button[aria-label="New folder"]');
    expect(button).not.toBeNull();
    button?.click();
  }

  it("keeps a late folder failure from another project out of the shown project", async () => {
    const props = await mountHome("project-a");
    clickNewFolder();
    await settle();
    expect(backend.folderRequests).toHaveLength(1);

    props.projectId = "project-b";
    flushSync();
    backend.folderRequests[0]?.reject(new Error("disk full"));
    await settle();

    expect(target?.textContent).not.toContain("The folder could not be updated.");
  }, 15_000);

  it("hides a project's folder failure after switching to another project", async () => {
    const props = await mountHome("project-a");
    clickNewFolder();
    await settle();
    backend.folderRequests[0]?.reject(new Error("disk full"));
    await settle();
    expect(target?.textContent).toContain("The folder could not be updated.");

    props.projectId = "project-b";
    flushSync();
    await tick();

    expect(target?.textContent).not.toContain("The folder could not be updated.");
  }, 15_000);
});
