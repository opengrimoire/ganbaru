import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { isNotesPageActive } from "./workspace";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: async () => "sqlite:test" }));

const invokeMock = vi.mocked(invoke);
const pageId = "00000000-0000-4000-8000-000000000001";

function shell(pages: Record<string, unknown>[]) {
  return {
    pages,
    folders: [],
    navigation_pages: [],
    navigation_folders: [],
    navigation_page_ids_with_children: [],
    navigation_databases: [],
    page_ids_with_children: [],
    missing_parent_page_ids: [],
    trashed_parent_page_ids: [],
    resolved_selected_page_id: pages.some((page) => page.id === pageId) ? pageId : null,
    total_page_count: pages.length,
    total_folder_count: 0,
    next_page_cursor: null,
    next_folder_cursor: null,
  };
}

function activePage() {
  return {
    id: pageId,
    created_time: "2026-09-28T12:00:00Z",
    last_edited_time: "2026-09-28T12:00:00Z",
    parent_type: "workspace",
    parent_page_id: null,
    parent_block_id: null,
    parent_data_source_id: null,
    folder_id: null,
    title: "Note",
    project_id: null,
    icon: null,
  };
}

describe("Notes active page probe", () => {
  beforeEach(() => vi.clearAllMocks());

  it("checks the selected page across projects without loading a page window", async () => {
    invokeMock.mockResolvedValue(shell([activePage()]));

    await expect(isNotesPageActive(pageId)).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("notes_load_workspace_shell", {
      dbUrl: "sqlite:test",
      request: {
        project_id: null,
        selected_page_id: pageId,
        expanded_page_ids: [],
        seed_page_ids: [],
        page_cursor: "end",
        folder_cursor: "end",
      },
    });
  });

  it("reports a page absent from the authoritative active shell", async () => {
    invokeMock.mockResolvedValue(shell([]));

    await expect(isNotesPageActive(pageId)).resolves.toBe(false);
  });
});
