import { describe, expect, it } from "vitest";
import {
  notesDatabaseTimelineColumns,
  notesDatabaseTimelineConfigurationFromView,
  notesDatabaseTimelineDateColumns,
  notesDatabaseTimelineDates,
  notesDatabaseTimelineDateValue,
  notesDatabaseTimelineGroupableColumns,
  notesDatabaseTimelineGroups,
  notesDatabaseTimelineItems,
  notesDatabaseTimelineMonthRange,
  notesDatabaseTimelineResizedDateValue,
  notesDatabaseTimelineRowCreateProperties,
  notesDatabaseTimelineRowTitle,
  notesDatabaseTimelineShiftMonth,
  notesDatabaseTimelineUpdate,
  notesDatabaseTimelineVisibleColumns,
} from "./timeline";
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
    Window: {
      id: "window",
      name: "Window",
      description: "",
      type: "date",
      date: {},
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
  name: "Timeline",
  type: "timeline",
  filter: {
    type: "and",
    filters: [{ property_id: "status", condition: "equals", value: "Doing" }],
  },
  sorts: [{ property_id: "estimate", direction: "descending" }],
  configuration: {
    type: "timeline",
    timeline: {
      date_property_id: "window",
      group_property_id: "status",
      group_order: ["doing", "todo", "__empty__"],
      hidden_group_ids: ["__empty__"],
      range_start: "2026-07-01",
      range_end: "2026-07-31",
      visible_property_ids: ["estimate"],
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

const alphaPage: NotesPage = {
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
          text: { content: "Write timeline", link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: "Write timeline",
          href: null,
        },
      ],
    },
    Window: {
      id: "window",
      type: "date",
      date: { start: "2026-07-10", end: "2026-07-15" },
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "doing", name: "Doing", color: "blue" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 3,
    },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

const betaPage: NotesPage = {
  ...alphaPage,
  id: "66666666-6666-4666-8666-666666666666",
  properties: {
    ...alphaPage.properties,
    Name: {
      id: "title",
      type: "title",
      title: [{ type: "text", text: { content: "Ship timeline", link: null }, plain_text: "Ship timeline" }],
    },
    Window: {
      id: "window",
      type: "date",
      date: { start: "2026-07-30", end: "2026-08-02" },
    },
    Status: {
      id: "status",
      type: "status",
      status: { id: "todo", name: "To-do", color: "gray" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 8,
    },
  },
};

describe("database timeline helpers", () => {
  it("reads configuration, date columns, groupable columns, and visible properties", () => {
    const configuration = notesDatabaseTimelineConfigurationFromView(view);
    const columns = notesDatabaseTimelineColumns(dataSource, view);
    const dateColumns = notesDatabaseTimelineDateColumns(columns);
    const groupableColumns = notesDatabaseTimelineGroupableColumns(columns);
    const visibleColumns = notesDatabaseTimelineVisibleColumns(columns, configuration);

    expect(configuration.date_property_id).toBe("window");
    expect(configuration.group_property_id).toBe("status");
    expect(configuration.hidden_group_ids).toEqual(["__empty__"]);
    expect(configuration.row_open_mode).toBe("side_panel");
    expect(dateColumns.map((column) => column.id)).toEqual(["window"]);
    expect(groupableColumns.map((column) => column.id).sort()).toEqual(["status", "window"]);
    expect(visibleColumns.map((column) => column.id)).toEqual(["estimate"]);
    expect(notesDatabaseTimelineMonthRange("2026-07-10")).toEqual({
      range_start: "2026-07-01",
      range_end: "2026-07-31",
    });
  });

  it("groups rows and computes horizontal item spans", () => {
    const configuration = notesDatabaseTimelineConfigurationFromView(view);
    const columns = notesDatabaseTimelineColumns(dataSource, view);
    const dateColumn = notesDatabaseTimelineDateColumns(columns)[0] ?? null;
    const groups = notesDatabaseTimelineGroups([betaPage, alphaPage], columns, configuration);
    const doingItems = notesDatabaseTimelineItems(groups[0]?.rows ?? [], dateColumn, configuration);
    const todoItems = notesDatabaseTimelineItems(groups[1]?.rows ?? [], dateColumn, configuration);
    const dates = notesDatabaseTimelineDates(configuration);

    expect(dates).toHaveLength(31);
    expect(groups.map((group) => group.id)).toEqual(["doing", "todo", "__empty__"]);
    expect(groups[0]?.rows.map((row) => row.id)).toEqual([alphaPage.id]);
    expect(groups[1]?.rows.map((row) => row.id)).toEqual([betaPage.id]);
    expect(groups[2]?.hidden).toBe(true);
    expect(doingItems[0]?.grid_column).toBe("10 / span 6");
    expect(todoItems[0]?.grid_column).toBe("30 / span 2");
    expect(notesDatabaseTimelineRowTitle(betaPage, columns, "Untitled")).toBe("Ship timeline");
  });

  it("serializes updates, row creation properties, and resize payloads", () => {
    const configuration = notesDatabaseTimelineConfigurationFromView(view);
    const shifted = notesDatabaseTimelineShiftMonth(configuration, 1);
    const columns = notesDatabaseTimelineColumns(dataSource, view);
    const visibleColumns = notesDatabaseTimelineVisibleColumns(columns, configuration);
    const update = notesDatabaseTimelineUpdate(
      shifted,
      visibleColumns,
      [{ property_id: "status", condition: "equals", value: "Doing" }],
      [{ property_id: "estimate", direction: "descending" }],
    );

    expect(shifted.range_start).toBe("2026-08-01");
    expect(shifted.range_end).toBe("2026-08-31");
    expect(update.configuration.visible_property_ids).toEqual(["estimate"]);
    expect(update.configuration.group_property_id).toBe("status");
    expect(update.filter).toEqual([{ property_id: "status", condition: "equals", value: "Doing" }]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "descending" }]);
    expect(notesDatabaseTimelineRowCreateProperties(dataSource, "window", "2026-08-04")).toEqual({
      Window: {
        id: "window",
        type: "date",
        date: { start: "2026-08-04", end: null, time_zone: null },
      },
    });
    expect(notesDatabaseTimelineDateValue("2026-08-10", "2026-08-12")).toEqual({
      start: "2026-08-10",
      end: "2026-08-12",
      time_zone: null,
    });
    expect(notesDatabaseTimelineDateValue("2026-08-12", "2026-08-10")).toEqual({
      start: "2026-08-12",
      end: null,
      time_zone: null,
    });
  });
});

describe("notesDatabaseTimelineResizedDateValue", () => {
  it("keeps the row's reminder when a resize moves its dates", () => {
    const reminded: NotesPage = {
      ...alphaPage,
      properties: {
        ...alphaPage.properties,
        Window: {
          id: "window",
          type: "date",
          date: { start: "2026-07-10", end: "2026-07-15", time_zone: null, ganbaru_reminder: { enabled: true, minutes_before: 1440 } },
        },
      },
    };
    expect(notesDatabaseTimelineResizedDateValue(reminded, "window", "2026-07-12", "2026-07-20")).toEqual({
      start: "2026-07-12",
      end: "2026-07-20",
      time_zone: null,
      ganbaru_reminder: { enabled: true, minutes_before: 1440 },
    });
  });

  it("saves plain dates when the row has no date yet or no reminder", () => {
    const undated: NotesPage = { ...alphaPage, properties: { Name: alphaPage.properties.Name } };
    expect(notesDatabaseTimelineResizedDateValue(undated, "window", "2026-07-12", "2026-07-20")).toEqual({
      start: "2026-07-12",
      end: "2026-07-20",
      time_zone: null,
    });
    expect(notesDatabaseTimelineResizedDateValue(alphaPage, "window", "2026-07-12", "2026-07-12")).toEqual({
      start: "2026-07-12",
      end: null,
      time_zone: null,
    });
  });
});
