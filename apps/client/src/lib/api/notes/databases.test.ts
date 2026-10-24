import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createRichText } from "$lib/notes/blocks/payloads";
import { notesDatabasePropertyCell } from "$lib/notes/database/property-cell";
import {
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableRelationItems,
} from "$lib/notes/database/table";
import { getNotesDataSourceTableView, listNotesDataSourceTemplates } from "./databases";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:test") }));
vi.mock("$lib/notes/collaboration/notification-schedule.svelte", () => ({
  invalidateNotesNotificationSchedule: vi.fn(),
}));
vi.mock("./mutation", () => ({ invokeNotesMutation: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const now = "2026-10-24T09:00:00.000Z";
const DATABASE_ID = "11111111-1111-4111-8111-111111111111";
const SOURCE_ID = "22222222-2222-4222-8222-222222222222";
const PROJECTS_ID = "33333333-3333-4333-8333-333333333333";
const VIEW_ID = "44444444-4444-4444-8444-444444444444";
const TASK_ID = "55555555-5555-4555-8555-555555555555";
const MISMATCH_ID = "66666666-6666-4666-8666-666666666666";
const PROJECT_ID = "77777777-7777-4777-8777-777777777777";

function dataSourceDto() {
  return {
    object: "data_source",
    id: SOURCE_ID,
    parent: { type: "database_id", database_id: DATABASE_ID },
    database_parent: { type: "workspace", workspace: true },
    title: "Tasks",
    title_rich_text: [createRichText("Tasks")],
    description: [],
    icon: null,
    properties: {
      Name: { id: "title", name: "Name", type: "title", title: {} },
      Estimate: { id: "estimate", name: "Estimate", type: "number", number: { format: "number" } },
      Project: {
        id: "project",
        name: "Project",
        type: "relation",
        relation: {
          data_source_id: PROJECTS_ID,
          dual_property: { synced_property_id: "tasks", synced_property_name: "Tasks" },
        },
      },
    },
    in_trash: false,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: now,
    last_edited_time: now,
  };
}

function viewDto() {
  return {
    object: "view",
    id: VIEW_ID,
    parent: { type: "database_id", database_id: DATABASE_ID },
    data_source_id: SOURCE_ID,
    name: "Table",
    type: "table",
    filter: null,
    sorts: [],
    configuration: null,
    url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
    created_time: now,
    last_edited_time: now,
  };
}

function rowDto(id: string, title: string, properties: Record<string, unknown>) {
  return {
    object: "page",
    id,
    created_time: now,
    last_edited_time: now,
    parent: { type: "data_source_id", data_source_id: SOURCE_ID },
    folder_id: null,
    in_trash: false,
    archived: false,
    icon: null,
    cover: null,
    properties: {
      Name: { id: "title", type: "title", title: [createRichText(title)] },
      ...properties,
    },
    url: null,
    public_url: null,
    source_provider: null,
    source_object_id: null,
    source_workspace_id: null,
    source_last_edited_time: null,
  };
}

function tableViewDto(rows: unknown[]) {
  return {
    data_source: dataSourceDto(),
    view: viewDto(),
    rows,
    total_row_count: rows.length,
    next_cursor: null,
    has_more: false,
    group_counts: {},
    calculations: { overall: {}, groups: {} },
    row_hierarchy: {
      [MISMATCH_ID]: {
        parent_row_page_id: TASK_ID,
        ancestor_row_page_ids: [TASK_ID],
        depth: 1,
        child_count: 0,
      },
    },
  };
}

function templateDto(isDefault: unknown) {
  return {
    object: "data_source_template",
    id: "88888888-8888-4888-8888-888888888888",
    data_source_id: SOURCE_ID,
    source_page_id: null,
    name: "Weekly review",
    properties: {},
    is_default: isDefault,
    block_count: 2,
    created_time: now,
    last_edited_time: now,
  };
}

describe("Notes databases API boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("maps stored values, reverse relation reads, and type mismatches from a table window", async () => {
    invokeMock.mockResolvedValue(tableViewDto([
      rowDto(TASK_ID, "Write tests", {
        Estimate: { id: "estimate", type: "number", number: 3 },
        Project: { id: "project", type: "relation", relation: [{ id: PROJECT_ID, title: "Alpha" }], has_more: false },
      }),
      rowDto(MISMATCH_ID, "Imported", {
        Estimate: { id: "estimate", type: "rich_text", rich_text: [createRichText("about three")] },
        Project: { id: "project", type: "relation", relation: [], has_more: false },
      }),
    ]));

    const table = await getNotesDataSourceTableView(SOURCE_ID, null, { page_size: 50, start_cursor: null });

    expect(invokeMock).toHaveBeenCalledWith("notes_data_source_table_view", {
      dbUrl: "sqlite:test",
      dataSourceId: SOURCE_ID,
      databaseId: null,
      viewId: null,
      window: { page_size: 50, start_cursor: null },
    });
    const columns = notesDatabaseTableColumns(table.data_source, table.view);
    const estimate = columns.find((column) => column.id === "estimate");
    const project = columns.find((column) => column.id === "project");
    if (!estimate || !project) throw new Error("expected estimate and project columns");
    const [task, mismatch] = table.rows;

    expect(notesDatabasePropertyCell(task, estimate)).toMatchObject({ kind: "value", payload: 3 });
    expect(notesDatabaseTableCellText(task, estimate)).toBe("3");
    expect(notesDatabaseTableRelationItems(task, project)).toEqual([{ id: PROJECT_ID, title: "Alpha" }]);

    expect(notesDatabasePropertyCell(mismatch, estimate)).toMatchObject({
      kind: "mismatch",
      storedType: "rich_text",
      payload: [expect.objectContaining({ plain_text: "about three" })],
    });
    expect(notesDatabaseTableCellText(mismatch, estimate)).toBe("");
    expect(notesDatabaseTableRelationItems(mismatch, project)).toEqual([]);
    expect(notesDatabasePropertyCell(mismatch, { id: "missing", type: "number" })).toEqual({ kind: "empty" });
    expect(table.row_hierarchy?.[MISMATCH_ID]?.parent_row_page_id).toBe(TASK_ID);
  });

  it("treats a property entry without a stored type as empty instead of guessing its type", async () => {
    invokeMock.mockResolvedValue(tableViewDto([
      rowDto(TASK_ID, "Write tests", { Estimate: { id: "estimate", number: 3 } }),
    ]));

    const table = await getNotesDataSourceTableView(SOURCE_ID);

    expect(notesDatabasePropertyCell(table.rows[0], { id: "estimate", type: "number" })).toEqual({ kind: "empty" });
  });

  it("rejects table windows whose rows or sub-item metadata are malformed", async () => {
    invokeMock.mockResolvedValueOnce(tableViewDto([{ ...rowDto(TASK_ID, "Write tests", {}), properties: [] }]));
    await expect(getNotesDataSourceTableView(SOURCE_ID)).rejects.toThrow("page.properties must be an object");

    const inconsistent = tableViewDto([rowDto(TASK_ID, "Write tests", {})]);
    inconsistent.row_hierarchy[MISMATCH_ID].ancestor_row_page_ids = [];
    invokeMock.mockResolvedValueOnce(inconsistent);
    await expect(getNotesDataSourceTableView(SOURCE_ID)).rejects.toThrow("Database row hierarchy is inconsistent");
  });

  it("maps the default template flag and rejects malformed template lists", async () => {
    invokeMock.mockResolvedValueOnce([templateDto(true), { ...templateDto(false), id: "99999999-9999-4999-8999-999999999999" }]);
    const templates = await listNotesDataSourceTemplates(SOURCE_ID);
    expect(templates.map((template) => template.is_default)).toEqual([true, false]);
    expect(invokeMock).toHaveBeenCalledWith("notes_list_data_source_templates", { dbUrl: "sqlite:test", dataSourceId: SOURCE_ID });

    invokeMock.mockResolvedValueOnce([templateDto("yes")]);
    await expect(listNotesDataSourceTemplates(SOURCE_ID)).rejects.toThrow("data source template.is_default must be a boolean");

    invokeMock.mockResolvedValueOnce({ templates: [] });
    await expect(listNotesDataSourceTemplates(SOURCE_ID)).rejects.toThrow("non-array payload");
  });
});
