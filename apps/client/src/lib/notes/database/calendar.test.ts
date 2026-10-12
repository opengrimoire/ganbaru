import { describe, expect, it } from "vitest";
import {
  notesDatabaseCalendarColumns,
  notesDatabaseCalendarConfigurationFromView,
  notesDatabaseCalendarDateColumns,
  notesDatabaseCalendarDays,
  notesDatabaseCalendarMonthRange,
  notesDatabaseCalendarRowCreateProperties,
  notesDatabaseCalendarRowTitle,
  notesDatabaseCalendarShiftMonth,
  notesDatabaseCalendarUpdate,
  notesDatabaseCalendarVisibleColumns,
} from "./calendar";
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
    Due: {
      id: "due",
      name: "Due",
      description: "",
      type: "date",
      date: {},
    },
    Estimate: {
      id: "estimate",
      name: "Estimate",
      description: "",
      type: "number",
      number: { format: "number" },
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
  name: "Calendar",
  type: "calendar",
  filter: {
    type: "and",
    filters: [{ property_id: "status", condition: "equals", value: "Doing" }],
  },
  sorts: [{ property_id: "estimate", direction: "descending" }],
  configuration: {
    type: "calendar",
    calendar: {
      date_property_id: "due",
      range_start: "2026-07-01",
      range_end: "2026-07-31",
      visible_property_ids: ["estimate", "status"],
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
          text: { content: "Write calendar", link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: "Write calendar",
          href: null,
        },
      ],
    },
    Due: {
      id: "due",
      type: "date",
      date: { start: "2026-07-10", end: null },
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
      title: [{ type: "text", text: { content: "Ship calendar", link: null }, plain_text: "Ship calendar" }],
    },
    Due: {
      id: "due",
      type: "date",
      date: { start: "2026-07-30", end: "2026-08-02" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 8,
    },
  },
};

describe("database calendar helpers", () => {
  it("reads configuration, date columns, visible properties, and month range", () => {
    const configuration = notesDatabaseCalendarConfigurationFromView(view);
    const columns = notesDatabaseCalendarColumns(dataSource, view);
    const dateColumns = notesDatabaseCalendarDateColumns(columns);
    const visibleColumns = notesDatabaseCalendarVisibleColumns(columns, configuration);

    expect(configuration.date_property_id).toBe("due");
    expect(configuration.range_start).toBe("2026-07-01");
    expect(configuration.range_end).toBe("2026-07-31");
    expect(configuration.row_open_mode).toBe("side_panel");
    expect(dateColumns.map((column) => column.id)).toEqual(["due"]);
    expect(visibleColumns.map((column) => column.id)).toEqual(["estimate", "status"]);
    expect(notesDatabaseCalendarMonthRange("2026-07-10")).toEqual({
      range_start: "2026-07-01",
      range_end: "2026-07-31",
    });
  });

  it("places rows on month days and repeats rows across date ranges", () => {
    const configuration = notesDatabaseCalendarConfigurationFromView(view);
    const columns = notesDatabaseCalendarColumns(dataSource, view);
    const dateColumn = notesDatabaseCalendarDateColumns(columns)[0] ?? null;
    const days = notesDatabaseCalendarDays([alphaPage, betaPage], dateColumn, configuration);
    const julyTen = days.find((day) => day.date === "2026-07-10");
    const julyThirtyOne = days.find((day) => day.date === "2026-07-31");
    const augustOne = days.find((day) => day.date === "2026-08-01");

    expect(days).toHaveLength(42);
    expect(julyTen?.rows.map((row) => row.id)).toEqual([alphaPage.id]);
    expect(julyThirtyOne?.rows.map((row) => row.id)).toEqual([betaPage.id]);
    expect(augustOne?.in_month).toBe(false);
    expect(augustOne?.rows).toEqual([]);
    expect(notesDatabaseCalendarRowTitle(betaPage, columns, "Untitled")).toBe("Ship calendar");
  });

  it("serializes updates and row creation properties", () => {
    const configuration = notesDatabaseCalendarConfigurationFromView(view);
    const shifted = notesDatabaseCalendarShiftMonth(configuration, 1);
    const columns = notesDatabaseCalendarColumns(dataSource, view);
    const visibleColumns = notesDatabaseCalendarVisibleColumns(columns, configuration);
    const update = notesDatabaseCalendarUpdate(
      shifted,
      visibleColumns,
      [{ property_id: "status", condition: "equals", value: "Doing" }],
      [{ property_id: "estimate", direction: "descending" }],
    );

    expect(shifted.range_start).toBe("2026-08-01");
    expect(shifted.range_end).toBe("2026-08-31");
    expect(update.configuration.visible_property_ids).toEqual(["estimate", "status"]);
    expect(update.filter).toEqual([{ property_id: "status", condition: "equals", value: "Doing" }]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "descending" }]);
    expect(notesDatabaseCalendarRowCreateProperties(dataSource, "due", "2026-08-04")).toEqual({
      Due: {
        id: "due",
        type: "date",
        date: { start: "2026-08-04", end: null, time_zone: null },
      },
    });
  });
});
