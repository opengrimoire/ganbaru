import { describe, expect, it } from "vitest";
import {
  notesDatabaseTableCellEditValue,
  notesDatabaseTableOptionNames,
  notesDatabaseTableCellText,
  notesDatabaseTableColumnCanEdit,
  notesDatabaseTableColumns,
  notesDatabaseTableConfigurationFromView,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  notesDatabaseTableUpdate,
  notesDatabaseTableVisibleColumns,
} from "./table";
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
    Priority: {
      id: "priority",
      name: "Priority",
      description: "",
      type: "select",
      select: {
        options: [
          { id: "low", name: "Low", color: "blue" },
          { id: "high", name: "High", color: "red" },
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
    Done: {
      id: "done",
      name: "Done",
      description: "",
      type: "checkbox",
      checkbox: {},
    },
    Created: {
      id: "created",
      name: "Created",
      description: "",
      type: "created_time",
      created_time: {},
    },
    Project: {
      id: "project_relation",
      name: "Project",
      description: "",
      type: "relation",
      relation: {
        data_source_id: "66666666-6666-4666-8666-666666666666",
      },
    },
    "Project budget": {
      id: "project_budget",
      name: "Project budget",
      description: "",
      type: "rollup",
      rollup: {
        relation_property_id: "project_relation",
        relation_property_name: "Project",
        rollup_property_id: "budget",
        rollup_property_name: "Budget",
        function: "sum",
      },
    },
    Score: {
      id: "score_formula",
      name: "Score",
      description: "",
      type: "formula",
      formula: {
        expression: 'prop("Estimate") * 2',
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
  name: "Table",
  type: "table",
  filter: {
    type: "and",
    filters: [
      { property_id: "priority", condition: "equals", value: "High" },
      { property_id: "done", condition: "checked", value: null },
    ],
  },
  sorts: [{ property_id: "estimate", direction: "descending" }],
  configuration: {
    type: "table",
    table: {
      property_order: [
        "title",
        "estimate",
        "priority",
        "done",
        "created",
        "project_relation",
        "project_budget",
        "score_formula",
      ],
      hidden_property_ids: ["priority"],
      column_widths: {
        title: 320,
        estimate: 144,
        priority: 200,
      },
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
      title: [
        {
          type: "text",
          text: { content: "Write docs", link: null },
          annotations: {
            bold: false,
            italic: false,
            strikethrough: false,
            underline: false,
            code: false,
            color: "default",
          },
          plain_text: "Write docs",
          href: null,
        },
      ],
    },
    Priority: {
      id: "priority",
      type: "select",
      select: { id: "high", name: "High", color: "red" },
    },
    Estimate: {
      id: "estimate",
      type: "number",
      number: 3,
    },
    Done: {
      id: "done",
      type: "checkbox",
      checkbox: true,
    },
    Created: {
      id: "created",
      type: "created_time",
      created_time: "2026-07-01T00:00:00.000Z",
    },
    Project: {
      id: "project_relation",
      type: "relation",
      relation: [
        {
          id: "66666666-6666-4666-8666-666666666666",
          title: "Project Alpha",
        },
      ],
      has_more: false,
    },
    "Project budget": {
      id: "project_budget",
      type: "rollup",
      rollup: {
        type: "number",
        number: 7,
        function: "sum",
      },
    },
    Score: {
      id: "score_formula",
      type: "formula",
      formula: {
        type: "number",
        number: 6,
      },
    },
  },
  url: null,
  public_url: null,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
};

describe("database table helpers", () => {
  it("loads table configuration, ordered columns, visibility, and widths", () => {
    const configuration = notesDatabaseTableConfigurationFromView(view);
    const columns = notesDatabaseTableColumns(dataSource, view);

    expect(configuration.row_open_mode).toBe("side_panel");
    expect(columns.map((column) => column.id)).toEqual([
      "title",
      "estimate",
      "priority",
      "done",
      "created",
      "project_relation",
      "project_budget",
      "score_formula",
    ]);
    expect(columns[0]?.width).toBe(320);
    expect(columns[1]?.width).toBe(144);
    expect(columns[2]?.hidden).toBe(true);
    expect(notesDatabaseTableVisibleColumns(columns).map((column) => column.id)).toEqual([
      "title",
      "estimate",
      "done",
      "created",
      "project_relation",
      "project_budget",
      "score_formula",
    ]);
  });

  it("serializes filters, sorts, hidden columns, widths, and open mode for persistence", () => {
    const columns = notesDatabaseTableColumns(dataSource, view).map((column) =>
      column.id === "done" ? { ...column, hidden: true, width: 999 } : column,
    );
    const update = notesDatabaseTableUpdate(
      columns,
      "full_page",
      notesDatabaseTableFiltersFromView(view),
      notesDatabaseTableSortsFromView(view),
    );

    expect(update.filter).toEqual([
      { property_id: "priority", condition: "equals", value: "High" },
      { property_id: "done", condition: "checked", value: null },
    ]);
    expect(update.sorts).toEqual([{ property_id: "estimate", direction: "descending" }]);
    expect(update.configuration.hidden_property_ids).toEqual(["priority", "done"]);
    expect(update.configuration.column_widths.done).toBe(480);
    expect(update.configuration.row_open_mode).toBe("full_page");
  });

  it("reads editable and read-only cell values from Notion-shaped properties", () => {
    const columns = notesDatabaseTableColumns(dataSource, view);
    const title = columns.find((column) => column.id === "title");
    const priority = columns.find((column) => column.id === "priority");
    const done = columns.find((column) => column.id === "done");
    const created = columns.find((column) => column.id === "created");
    const project = columns.find((column) => column.id === "project_relation");
    const projectBudget = columns.find((column) => column.id === "project_budget");
    const score = columns.find((column) => column.id === "score_formula");

    expect(title && notesDatabaseTableCellText(page, title)).toBe("Write docs");
    expect(priority && notesDatabaseTableCellText(page, priority)).toBe("High");
    expect(done && notesDatabaseTableCellEditValue(page, done)).toBe(true);
    expect(created && notesDatabaseTableColumnCanEdit(created)).toBe(false);
    expect(project && notesDatabaseTableCellText(page, project)).toBe("Project Alpha");
    expect(project && notesDatabaseTableCellEditValue(page, project)).toEqual([
      "66666666-6666-4666-8666-666666666666",
    ]);
    expect(projectBudget && notesDatabaseTableCellText(page, projectBudget)).toBe("7");
    expect(projectBudget && notesDatabaseTableColumnCanEdit(projectBudget)).toBe(false);
    expect(score && notesDatabaseTableCellText(page, score)).toBe("6");
    expect(score && notesDatabaseTableColumnCanEdit(score)).toBe(false);
  });

  it("renders database button columns as read-only action cells", () => {
    const dataSourceWithButton: NotesDataSource = {
      ...dataSource,
      properties: {
        ...dataSource.properties,
        Finish: {
          id: "finish_button",
          name: "Finish",
          type: "button",
          button: {
            label: "Mark done",
            requires_confirmation: true,
            actions: [{
              type: "update_current_row_property",
              property_id: "done",
              property_name: "Done",
              property_type: "checkbox",
              value: true,
            }],
          },
        },
      },
    };
    const viewWithButton: NotesDatabaseView = {
      ...view,
      configuration: {
        type: "table",
        table: {
          property_order: [
            "title",
            "done",
            "finish_button",
          ],
          hidden_property_ids: [],
          column_widths: {},
          row_open_mode: "full_page",
        },
      },
    };
    const pageWithButton: NotesPage = {
      ...page,
      properties: {
        ...page.properties,
        Finish: {
          id: "finish_button",
          type: "button",
          button: {
            label: "Mark done",
          },
        },
      },
    };

    const columns = notesDatabaseTableColumns(dataSourceWithButton, viewWithButton);
    const finish = columns.find((column) => column.id === "finish_button");

    expect(finish?.buttonLabel).toBe("Mark done");
    expect(finish?.buttonRequiresConfirmation).toBe(true);
    expect(finish && notesDatabaseTableCellText(pageWithButton, finish)).toBe("Mark done");
    expect(finish && notesDatabaseTableColumnCanEdit(finish)).toBe(false);
  });
});


describe("database option display", () => {
  it("preserves punctuation in multi-select names and skips malformed entries", () => {
    const columns = notesDatabaseTableColumns(dataSource, view);
    const column = { ...columns[0], id: "tags", name: "Tags", type: "multi_select" as const };
    const row = { ...page, properties: { Tags: { id: "tags", type: "multi_select", multi_select: [{ name: "Design, review" }, { name: "Build" }, null, { name: 123 }] } } };
    expect(notesDatabaseTableOptionNames(row, column)).toEqual(["Design, review", "Build"]);
    expect(notesDatabaseTableOptionNames({ ...row, properties: {} }, column)).toEqual([]);
  });
});
