// @vitest-environment jsdom

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesPage } from "$lib/notes/types";

/** Builds an archived page fixture with the given id and title. */
function archivedPage(id: string, title: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-07-01T12:00:00.000Z",
    last_edited_time: "2026-07-01T12:00:00.000Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: true,
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
    archivedPages: [] as NotesPage[],
    archiveLoading: false,
    archiveError: null as string | null,
    archiveHasMore: false,
    reloadArchivedPages: async (_query: string) => undefined,
    loadMoreArchivedPages: async () => undefined,
    unarchivePage: async (_pageId: string) => undefined,
  },
}));

vi.mock("$lib/stores/notes.svelte", () => ({ useNotesWorkspace: () => workspace.store }));

describe("NotesArchiveView", () => {
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

  it("keeps each unarchiving page disabled until its own unarchive settles", async () => {
    workspace.store.archivedPages = [archivedPage("page-a", "Alpha"), archivedPage("page-b", "Beta")];
    workspace.store.unarchivePage = (pageId: string) =>
      new Promise<undefined>((resolve) => { workspace.settlers.set(pageId, () => resolve(undefined)); });
    target = document.createElement("div");
    document.body.append(target);
    const { default: NotesArchiveView } = await import("./NotesArchiveView.svelte");
    component = mount(NotesArchiveView, { target });
    flushSync();

    button("Unarchive Alpha").click();
    flushSync();
    button("Unarchive Beta").click();
    flushSync();
    expect(button("Unarchive Alpha").disabled).toBe(true);
    expect(button("Unarchive Beta").disabled).toBe(true);

    workspace.settlers.get("page-a")?.();
    await tick();
    await tick();

    expect(button("Unarchive Alpha").disabled).toBe(false);
    expect(button("Unarchive Beta").disabled).toBe(true);
  });
});
