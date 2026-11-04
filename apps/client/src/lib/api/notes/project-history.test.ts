import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { loadNotesProjectHistoryPage, loadNotesProjectHistoryTree } from "./project-history";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:test") }));
vi.mock("$lib/notes/database/window-sync", () => ({ publishNotesDatabaseChange: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const PROJECT_ID = "project-1";
const VERSION_ID = "version-1";
const fileWithoutPayload = { type: "file" };

function versionDto(): Record<string, unknown> {
  return {
    id: VERSION_ID,
    projectId: PROJECT_ID,
    manifestHash: "hash",
    reason: "checkpoint",
    createdBy: "user",
    displayName: { type: "user", resolved_name: "Someone" },
    changedNoteSummary: "",
    pageCount: 2,
    activePageCount: 2,
    archivedPageCount: 0,
    deletedPageCount: 0,
    createdTime: "2026-11-01T12:00:00Z",
  };
}

function pageSummaryDto(id: string, icon: unknown): Record<string, unknown> {
  return {
    id,
    title: "Page",
    parentPageId: null,
    parentDataSourceId: null,
    inTrash: false,
    archived: false,
    icon,
  };
}

function historicalPageDto(icon: unknown, cover: unknown): Record<string, unknown> {
  return {
    id: "page-1",
    title: "Page",
    properties: {},
    icon,
    cover,
    inTrash: false,
    archived: false,
    blocks: [],
    databases: [],
    dataSources: [],
    databaseViews: [],
  };
}

describe("Notes project history parsing", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("reads an unusable synced icon in a version tree as no icon and keeps the other pages", async () => {
    invokeMock.mockResolvedValueOnce({
      version: versionDto(),
      pages: [
        pageSummaryDto("page-1", fileWithoutPayload),
        pageSummaryDto("page-2", { type: "emoji", emoji: "📌" }),
        pageSummaryDto("page-3", null),
      ],
    });
    const tree = await loadNotesProjectHistoryTree(PROJECT_ID, VERSION_ID);
    expect(tree.pages.map((page) => page.id)).toEqual(["page-1", "page-2", "page-3"]);
    expect(tree.pages[0]?.icon).toBeNull();
    expect(tree.pages[1]?.icon).toEqual({ type: "emoji", emoji: "📌" });
    expect(tree.pages[2]?.icon).toBeNull();
  });

  it("reads an unusable synced icon and cover on a historical page as none", async () => {
    invokeMock.mockResolvedValueOnce(historicalPageDto(fileWithoutPayload, fileWithoutPayload));
    const page = await loadNotesProjectHistoryPage(PROJECT_ID, VERSION_ID, "page-1");
    expect(page.icon).toBeNull();
    expect(page.cover).toBeNull();
    expect(console.warn).toHaveBeenCalledTimes(2);
  });

  it("keeps a usable icon and cover on a historical page", async () => {
    const cover = { type: "external", external: { url: "https://example.com/cover.png" } };
    invokeMock.mockResolvedValueOnce(historicalPageDto({ type: "emoji", emoji: "📌" }, cover));
    const page = await loadNotesProjectHistoryPage(PROJECT_ID, VERSION_ID, "page-1");
    expect(page.icon).toEqual({ type: "emoji", emoji: "📌" });
    expect(page.cover).toEqual(cover);
  });
});
