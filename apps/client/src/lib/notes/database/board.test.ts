import { describe, expect, it } from "vitest";
import {
  notesDatabaseBoardCanMoveCards,
  notesDatabaseBoardCardText,
  notesDatabaseBoardCardTitle,
  notesDatabaseBoardColumns,
  notesDatabaseBoardConfigurationFromView,
  notesDatabaseBoardGroupableColumns,
  notesDatabaseBoardUpdate,
  notesDatabaseBoardVisibleColumns,
} from "./board";
import type { NotesDataSource, NotesDataSourceBoardGroup, NotesDatabaseView, NotesPage } from "$lib/notes/types";

const dataSource: NotesDataSource = {
  object: "data_source",
  id: "11111111-1111-4111-8111-111111111111",
  parent: {
    type: "database_id",
    database_id: "22222222-2222-4222-8222-222222222222",
  },
  database_parent: {
    type: "page_id",
    page_id: "33333333-3333-4333-8333-333333333333",
  },
  title: "Tasks",
  title_rich_text: [],
  description: [],
  icon: null,
  properties: {
    Name: {
      id: "title",
      name: "Name",
      description: "",
      type: "title",
      title: {},
    },
    Status: {
      id: "status",
      name: "Status",
      description: "",
      type: "status",
      status: {
        options: [
          { id: "todo", name: "To-do", color: "gray", group: "To-do" },
          { id: "doing", name: "Doing", color: "blue", group: "In progress" },
        ],
      },
    },
    Due: {
      id: "due",
      name: "Due",
      description: "",
      type: "date",
      date: {},
    },
    Owner: {
      id: "owner",
      name: "Owner",
      description: "",
      type: "people",
      people: {},
    },
    Estimate: {
      id: "estimate",
      name: "Estimate",
      description: "",
      type: "number",
      number: { format: "number" },
    },
  },
  in_trash: false,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T00:00:00.000Z",
};

const view: NotesDatabaseView = {
  object: "view",
  id: "44444444-4444-4444-8444-444444444444",
  parent: {
    type: "database_id",
    database_id: "22222222-2222-4222-8222-222222222222",
  },
  data_source_id: dataSource.id,
  name: "Board",
  type: "board",
  filter: {
    type: "and",
    filters: [{ property_id: "status", condition: "equals", value: "Doing" }],
  },
  sorts: [{ property_id: "estimate", direction: "ascending" }],
  configuration: {
    type: "board",
    board: {
      group_property_id: "status",
      group_order: ["todo", "doing", "__empty__"],
      hidden_group_ids: ["__empty__"],
      visible_property_ids: ["estimate", "due"],
      row_open_mode: "side_panel",
    },
  },
  url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T00:00:00.000Z",
};

const page: NotesPage = {
  object: "page",
  id: "55555555-5555-4555-8555-555555555555",
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T01:00:00.000Z",
  parent: {
    type: "data_source_id",
    data_source_id: dataSource.id,
  },
  folder_id: null,
  in_trash: false,
  archived: false,
  icon: null,
  cover: null,
  properties: {
    Name: {
      id: "title",
      type: "title",
      title: [{ type: "text", text: { content: "Ship board", link: null }, plain_text: "Ship board" }],
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "doing", name: "Doing", color: "blue" },
    },
    Due: {
      id: "due",
      type: "date",
      date: { start: "2026-07-02", end: null },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 5,
    },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

const groups: NotesDataSourceBoardGroup[] = [
  { id: "todo", name: "To-do", color: "gray", hidden: false, rows: [] },
  { id: "doing", name: "Doing", color: "blue", hidden: false, rows: [page] },
  { id: "__empty__", name: "No value", color: "default", hidden: true, rows: [] },
];

describe("database board helpers", () => {
  it("reads board configuration, groupable columns, and visible card properties", () => {
    const configuration = notesDatabaseBoardConfigurationFromView(view);
    const columns = notesDatabaseBoardColumns(dataSource, view);
    const groupableColumns = notesDatabaseBoardGroupableColumns(columns);
    const visibleColumns = notesDatabaseBoardVisibleColumns(columns, configuration);

    expect(configuration.group_property_id).toBe("status");
    expect(configuration.hidden_group_ids).toEqual(["__empty__"]);
    expect(configuration.row_open_mode).toBe("side_panel");
    expect(groupableColumns.map((column) => column.id)).toEqual(["due", "owner", "status"]);
    expect(visibleColumns.map((column) => column.id)).toEqual(["estimate", "due"]);
  });

  it("serializes grouping, hidden groups, visible properties, filters, and sorts", () => {
    const configuration = {
      ...notesDatabaseBoardConfigurationFromView(view),
      group_property_id: "due",
      row_open_mode: "full_page" as const,
    };
    const columns = notesDatabaseBoardColumns(dataSource, view);
    const visibleColumns = columns.filter((column) => column.id === "estimate");
    const update = notesDatabaseBoardUpdate(
      configuration,
      groups,
      visibleColumns,
      [{ property_id: "status", condition: "equals", value: "Doing" }],
      [{ property_id: "estimate", direction: "ascending" }],
    );

    expect(update.configuration.group_property_id).toBe("due");
    expect(update.configuration.group_order).toEqual(["todo", "doing", "__empty__"]);
    expect(update.configuration.hidden_group_ids).toEqual(["__empty__"]);
    expect(update.configuration.visible_property_ids).toEqual(["estimate"]);
    expect(update.configuration.row_open_mode).toBe("full_page");
    expect(update.filter).toEqual([{ property_id: "status", condition: "equals", value: "Doing" }]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "ascending" }]);
  });

  it("reads card titles and disables placeholder people moves", () => {
    const columns = notesDatabaseBoardColumns(dataSource, view);
    const statusColumn = columns.find((column) => column.id === "status") ?? null;
    const ownerColumn = columns.find((column) => column.id === "owner") ?? null;
    const estimateColumn = columns.find((column) => column.id === "estimate");

    expect(notesDatabaseBoardCardTitle(page, columns, "Untitled")).toBe("Ship board");
    expect(estimateColumn && notesDatabaseBoardCardText(page, estimateColumn)).toBe("5");
    expect(notesDatabaseBoardCanMoveCards(statusColumn)).toBe(true);
    expect(notesDatabaseBoardCanMoveCards(ownerColumn)).toBe(false);
  });
});
