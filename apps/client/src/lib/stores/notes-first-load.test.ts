// @vitest-environment jsdom

import { describe, expect, it, vi } from "vitest";
import type { NotesPage, NotesWorkspaceShell } from "$lib/notes/types";

const backend = vi.hoisted(() => {
  const pending: Array<(value: NotesWorkspaceShell) => void> = [];
  return {
    calls: 0,
    optionalCalls: [] as string[],
    load() {
      this.calls += 1;
      return new Promise<NotesWorkspaceShell>((resolve) => pending.push(resolve));
    },
    resolve(index: number, value: NotesWorkspaceShell) {
      pending[index]?.(value);
    },
  };
});

vi.mock("$lib/vault/config", () => ({
  getConfigKey: (key: string, fallback: unknown) =>
    key === "notes.selectedPageId" ? "11111111-1111-4111-8111-111111111111" : fallback,
  setConfigKey: vi.fn(),
}));

vi.mock("$lib/api/notes", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/api/notes")>();
  return {
    ...actual,
    loadNotesWorkspaceShell: () => backend.load(),
    listNotesPageTemplates: () => {
      backend.optionalCalls.push("templates");
      return Promise.resolve([]);
    },
    getNotesLocalUser: () => {
      backend.optionalCalls.push("local-user");
      return Promise.reject(new Error("unexpected local user load"));
    },
    openNotesPage: () => {
      backend.optionalCalls.push("page");
      return Promise.reject(new Error("unexpected page load"));
    },
  };
});

function page(id: string, title: string): NotesPage {
  return {
    object: "page",
    id,
    created_time: "2026-01-01T00:00:00Z",
    last_edited_time: "2026-01-01T00:00:00Z",
    parent: { type: "workspace", workspace: true },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: { title: { id: "title", type: "title", title: [], plain_text: title } },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function shell(
  pages: NotesPage[],
  selected: string | null = null,
  navigationPages: NotesPage[] = [],
): NotesWorkspaceShell {
  return {
    pages,
    folders: [],
    navigation_pages: navigationPages,
    navigation_folders: [],
    navigation_page_ids_with_children: [],
    navigation_databases: [],
    page_ids_with_children: [],
    missing_parent_page_ids: [],
    trashed_parent_page_ids: [],
    resolved_selected_page_id: selected,
    total_page_count: pages.length,
    total_folder_count: 0,
    next_page_cursor: null,
    next_folder_cursor: null,
  };
}

describe("Notes initial loading", () => {
  it("single-flights the shell and ignores stale selection and workspace responses", async () => {
    const { getNotes } = await import("./notes.svelte");
    const notes = getNotes();
    const first = notes.ensureLoaded();
    const duplicate = notes.ensureLoaded();

    expect(backend.calls).toBe(1);
    await notes.selectPage(null);
    backend.resolve(0, shell([
      page("11111111-1111-4111-8111-111111111111", "Old selection"),
    ], "11111111-1111-4111-8111-111111111111"));
    await Promise.all([first, duplicate]);
    expect(notes.loaded).toBe(true);
    expect(notes.selectedPageId).toBeNull();
    expect(backend.optionalCalls).toEqual([]);

    const staleWorkspace = notes.load();
    const currentWorkspace = notes.load();
    backend.resolve(2, shell(
      [page("33333333-3333-4333-8333-333333333333", "Current")],
      null,
      [page("44444444-4444-4444-8444-444444444444", "Other project")],
    ));
    await currentWorkspace;
    backend.resolve(1, shell([page("22222222-2222-4222-8222-222222222222", "Stale")]));
    await staleWorkspace;

    expect(notes.allPages.map((item) => item.id)).toEqual([
      "44444444-4444-4444-8444-444444444444",
      "33333333-3333-4333-8333-333333333333",
    ]);
    expect(backend.optionalCalls).toEqual([]);
  });
});
