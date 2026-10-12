import { describe, expect, it } from "vitest";
import {
  notesDatabaseListColumns,
  notesDatabaseListConfigurationFromView,
  notesDatabaseListFiltersFromView,
  notesDatabaseListGroupableColumns,
  notesDatabaseListGroups,
  notesDatabaseListRowText,
  notesDatabaseListRowTitle,
  notesDatabaseListSortsFromView,
  notesDatabaseListUpdate,
  notesDatabaseListVisibleColumns,
} from "./list";
import type { NotesDataSource, NotesDatabaseView, NotesPage } from "$lib/notes/types";

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
  name: "List",
  type: "list",
  filter: {
    type: "and",
    filters: [{ property_id: "status", condition: "equals", value: "Doing" }],
  },
  sorts: [{ property_id: "estimate", direction: "descending" }],
  configuration: {
    type: "list",
    list: {
      group_property_id: "status",
      group_order: ["doing", "todo", "__empty__"],
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

const doingPage: NotesPage = {
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
      title: [
        {
          type: "text",
          text: { content: "Write list", link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: "Write list",
          href: null,
        },
      ],
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "doing", name: "Doing", color: "blue" },
    },
    Due: {
      id: "due",
      type: "date",
      date: { start: "2026-07-03", end: null },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 8,
    },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

const todoPage: NotesPage = {
  ...doingPage,
  id: "66666666-6666-4666-8666-666666666666",
  properties: {
    ...doingPage.properties,
    Name: {
      id: "title",
      type: "title",
      title: [{ type: "text", text: { content: "Review list", link: null }, plain_text: "Review list" }],
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "todo", name: "To-do", color: "gray" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 3,
    },
  },
};

describe("database list helpers", () => {
  it("reads list configuration, groupable columns, and visible row properties", () => {
    const configuration = notesDatabaseListConfigurationFromView(view);
    const columns = notesDatabaseListColumns(dataSource, view);
    const groupableColumns = notesDatabaseListGroupableColumns(columns);
    const visibleColumns = notesDatabaseListVisibleColumns(columns, configuration);

    expect(configuration.group_property_id).toBe("status");
    expect(configuration.hidden_group_ids).toEqual(["__empty__"]);
    expect(configuration.row_open_mode).toBe("side_panel");
    expect(groupableColumns.map((column) => column.id).sort()).toEqual(["due", "owner", "status"]);
    expect(visibleColumns.map((column) => column.id)).toEqual(["estimate", "due"]);
  });

  it("groups compact rows and preserves hidden group state", () => {
    const configuration = notesDatabaseListConfigurationFromView(view);
    const columns = notesDatabaseListColumns(dataSource, view);
    const groups = notesDatabaseListGroups([todoPage, doingPage], columns, configuration);

    expect(groups.map((group) => group.id)).toEqual(["doing", "todo", "__empty__"]);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual([doingPage.id]);
    expect(groups[1]?.rows.map((row) => row.id)).toEqual([todoPage.id]);
    expect(groups[2]?.hidden).toBe(true);
  });

  it("serializes grouping, visible properties, filters, and sorts", () => {
    const configuration = {
      ...notesDatabaseListConfigurationFromView(view),
      row_open_mode: "full_page" as const,
      hidden_group_ids: ["todo"],
    };
    const columns = notesDatabaseListColumns(dataSource, view);
    const visibleColumns = columns.filter((column) => column.id === "estimate");
    const update = notesDatabaseListUpdate(
      configuration,
      visibleColumns,
      notesDatabaseListFiltersFromView(view),
      notesDatabaseListSortsFromView(view),
    );

    expect(update.configuration.group_property_id).toBe("status");
    expect(update.configuration.group_order).toEqual(["doing", "todo", "__empty__"]);
    expect(update.configuration.hidden_group_ids).toEqual(["todo"]);
    expect(update.configuration.visible_property_ids).toEqual(["estimate"]);
    expect(update.configuration.row_open_mode).toBe("full_page");
    expect(update.filter).toEqual([{ property_id: "status", condition: "equals", value: "Doing" }]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "descending" }]);
  });

  it("reads row titles and property text", () => {
    const columns = notesDatabaseListColumns(dataSource, view);
    const estimateColumn = columns.find((column) => column.id === "estimate");

    expect(notesDatabaseListRowTitle(doingPage, columns, "Untitled")).toBe("Write list");
    expect(estimateColumn && notesDatabaseListRowText(doingPage, estimateColumn)).toBe("8");
  });
});
