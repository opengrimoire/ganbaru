// @vitest-environment jsdom

import { mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRichText } from "$lib/notes/blocks/factory";
import type { NotesFolder, NotesPage } from "$lib/notes/types";
import NotesHierarchyPickerPanel from "./NotesHierarchyPickerPanel.svelte";

const timestamp = "2026-07-01T12:00:00.000Z";

function page(id: string, title: string, folderId: string | null): NotesPage {
  return {
    object: "page",
    id,
    created_time: timestamp,
    last_edited_time: timestamp,
    parent: { type: "workspace", workspace: true },
    folder_id: folderId,
    in_trash: false,
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

const folder: NotesFolder = {
  object: "folder",
  id: "folder-a",
  project_id: "project-a",
  parent_folder_id: null,
  name: "Folder A",
  created_time: timestamp,
  last_edited_time: timestamp,
};

const editor = vi.hoisted(() => ({
  store: {
    allPages: [] as NotesPage[],
    folders: [] as NotesFolder[],
    navigationDatabases: [],
    favoriteFolderIds: [] as string[],
    favoritePageIds: [] as string[],
    sidebarPageIdsWithChildren: [] as string[],
    showArchivedInPickers: false,
    pickerArchivedPages: [] as NotesPage[],
    pickerArchiveError: null as string | null,
    loading: false,
    loadError: null as string | null,
    sidebarOrderOverride: (_projectId: string | null) => null,
    sidebarOrderForProject: (_projectId: string | null) => "name" as const,
    customOrderForProject: (_projectId: string | null) => [] as string[],
    ensurePickerArchivedPages: () => undefined,
    reloadPickerArchivedPages: () => Promise.resolve(),
    loadNavigationChildren: (_pageId: string) => Promise.resolve(),
    selectPage: (_pageId: string, _options: { openMode: "full" }) => Promise.resolve(),
  },
}));

vi.mock("$lib/components/notes/editor-context", () => ({
  getNotesEditor: () => editor.store,
}));

describe("NotesHierarchyPickerPanel", () => {
  let target: HTMLDivElement | null = null;
  let component: ReturnType<typeof mount> | null = null;

  afterEach(async () => {
    if (component) await unmount(component);
    target?.remove();
    component = null;
    target = null;
    vi.restoreAllMocks();
  });

  function buttonLabeled(label: string): HTMLButtonElement {
    const button = target?.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
    if (!button) throw new Error(`Button ${label} is missing`);
    return button;
  }

  it("lists the rows of another level after a note on the previous level failed to open", async () => {
    editor.store.allPages = [page("page-root", "Root note", null), page("page-a", "Nested note", "folder-a")];
    editor.store.folders = [folder];
    vi.spyOn(editor.store, "selectPage").mockRejectedValue(new Error("open failed"));
    target = document.createElement("div");
    document.body.append(target);
    component = mount(NotesHierarchyPickerPanel, {
      target,
      props: { projectId: null, frameStyle: "", mobileLayout: true, onPageSelected: vi.fn() },
    });
    await tick();

    buttonLabeled("Folder A").click();
    await tick();
    buttonLabeled("Nested note").click();
    await vi.waitFor(() => expect(target?.textContent).toContain("This note could not be opened."));
    expect(target?.querySelector('button[aria-label="Nested note"]')).toBeNull();

    buttonLabeled("Back to notes").click();
    await tick();
    expect(target?.textContent).not.toContain("This note could not be opened.");
    expect(buttonLabeled("Folder A")).toBeTruthy();
    expect(buttonLabeled("Root note")).toBeTruthy();

    buttonLabeled("Folder A").click();
    await tick();
    expect(target?.textContent).not.toContain("This note could not be opened.");
    expect(buttonLabeled("Nested note")).toBeTruthy();
  });
});
