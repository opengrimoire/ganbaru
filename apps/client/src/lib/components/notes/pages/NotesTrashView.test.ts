// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesPage } from "$lib/notes/types";

/** Builds a trashed page fixture with the given id and title. */
function trashedPage(id: string, title: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-01T12:00:00.000Z",
    last_edited_time: "2026-07-01T12:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: true,
    archived: false,
    icon: null,
    cover: null,
    properties: { title: { id: "title", type: "title", title: [createRichText(title)] } },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

const workspace = vi.hoisted(() => ({
  settlers: new Map<string, () => void>(),
  store: {
    trashedPages: [] as NotesPage[],
    trashLoading: false,
    trashError: null as string | null,
    trashHasMore: false,
    reloadTrashedPages: async (_query: string) => undefined,
    loadMoreTrashedPages: async () => undefined,
    restorePage: async (_pageId: string) => undefined,
    permanentlyDeletePage: async (_pageId: string) => undefined,
  },
}));

vi.mock("$lib/stores/notes.svelte", () => ({ useNotesWorkspace: () => workspace.store }));

describe("NotesTrashView", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    workspace.settlers.clear();
  });

  function button(label: string): HTMLButtonElement {
    const found = target?.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
    if (!found) throw new Error(`Missing button ${label}`);
    return found;
  }

  it("keeps each restoring page disabled until its own restore settles", async () => {
    workspace.store.trashedPages = [trashedPage("page-a", "Alpha"), trashedPage("page-b", "Beta")];
    workspace.store.restorePage = (pageId: string) =>
      new Promise<undefined>((resolve) => { workspace.settlers.set(pageId, () => resolve(undefined)); });
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesTrashView } = await import("./NotesTrashView.svelte");
    component = mount(NotesTrashView, { target });
    flushSync();

    button("Restore Alpha").click();
    flushSync();
    button("Restore Beta").click();
    flushSync();
    expect(button("Restore Alpha").disabled).toBe(true);
    expect(button("Delete Alpha forever").disabled).toBe(true);
    expect(button("Restore Beta").disabled).toBe(true);

    workspace.settlers.get("page-a")?.();
    await tick();
    await tick();

    expect(button("Restore Alpha").disabled).toBe(false);
    expect(button("Restore Beta").disabled).toBe(true);
    expect(button("Delete Beta forever").disabled).toBe(true);
  });
});
