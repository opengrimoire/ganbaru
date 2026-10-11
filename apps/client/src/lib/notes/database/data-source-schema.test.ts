import { describe, expect, it, vi } from "vitest";
import {
  createNotesDataSourcePropertyDraft,
  NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS,
  notesDataSourceDuplicatePropertyName,
  notesDataSourceButtonTargetOptions,
  notesDataSourceDefaultButtonPatch,
  notesDataSourcePropertyIcon,
  notesDataSourceRenameProperty,
  notesDataSourceSchemaDraftFromDto,
  notesDataSourceSchemaUpdateFromDraft,
  notesDataSourceSyncPropertyReferences,
  type NotesDataSourceSchemaPropertyDraft,
} from "./data-source-schema";
import type { NotesDataSource, NotesDatabaseView } from "$lib/notes/types";

vi.stubGlobal("crypto", {
  randomUUID: vi.fn(() => "99999999-9999-4999-8999-999999999999"),
});

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
      description: "Task importance",
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
      number: { format: "percent" },
    },
    Project: {
      id: "project_relation",
      name: "Project",
      description: "",
      type: "relation",
      relation: {
        data_source_id: "66666666-6666-4666-8666-666666666666",
        dual_property: {
          synced_property_id: "tasks_relation",
          synced_property_name: "Tasks",
        },
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
  filter: null,
  sorts: [],
  configuration: {
    type: "table",
    table: {
      property_order: [
        "title",
        "estimate",
        "priority",
        "project_relation",
        "project_budget",
        "score_formula",
      ],
      hidden_property_ids: ["priority"],
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

describe("data source schema helpers", () => {
  it("generates copy names with the native case-insensitive collision semantics", () => {
    const properties = [{ name: "Priority" }, { name: " Priority (Copy) " }, { name: "PRIORITY (COPY) 2" }];
    expect(notesDataSourceDuplicatePropertyName("Priority", properties, (name) => `${name} (copy)`)).toBe("Priority (copy) 3");
    expect(properties[1].name).toBe(" Priority (Copy) ");
    expect(notesDataSourceDuplicatePropertyName("Árbol", [{ name: "ÁRBOL (COPIA)" }], (name) => `${name} (copia)`)).toBe("Árbol (copia) 2");
  });

  it("preserves localized copy decorations when a maximum-length name needs shortening", () => {
    const original = "a".repeat(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS);
    const copy = notesDataSourceDuplicatePropertyName(original, [], (name) => `${name} (copy)`);
    expect(copy).toBe(`${"a".repeat(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS - " (copy)".length)} (copy)`);
    const next = notesDataSourceDuplicatePropertyName(original, [{ name: copy.toUpperCase() }], (name) => `${name} (copy)`);
    expect(next.endsWith(" (copy) 2")).toBe(true);
    expect(Array.from(next)).toHaveLength(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS);
  });

  it("counts Unicode scalar characters rather than UTF-16 units and adjusts for larger suffixes", () => {
    const original = "\u{10400}".repeat(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS);
    const formatCopy = (name: string) => `${name} (copia)`;
    const copies: { name: string }[] = [];
    for (let index = 1; index <= 12; index += 1) {
      const name = notesDataSourceDuplicatePropertyName(original, copies, formatCopy);
      expect(Array.from(name)).toHaveLength(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS);
      expect(name.endsWith(index === 1 ? " (copia)" : ` (copia) ${index}`)).toBe(true);
      copies.push({ name });
    }
    expect(new Set(copies.map((property) => property.name)).size).toBe(12);
  });
  it("loads schema draft properties in table order with visibility state", () => {
    const draft = notesDataSourceSchemaDraftFromDto(dataSource, view);

    expect(draft.map((property) => property.id)).toEqual([
      "title",
      "estimate",
      "priority",
      "project_relation",
      "project_budget",
      "score_formula",
    ]);
    expect(draft[1]?.numberFormat).toBe("percent");
    expect(draft[2]?.options[1]?.name).toBe("High");
    expect(draft[3]?.relationDataSourceId).toBe("66666666-6666-4666-8666-666666666666");
    expect(draft[3]?.relationSyncedPropertyId).toBe("tasks_relation");
    expect(draft[4]?.rollupRelationPropertyId).toBe("project_relation");
    expect(draft[4]?.rollupPropertyId).toBe("budget");
    expect(draft[4]?.rollupFunction).toBe("sum");
    expect(draft[5]?.formulaExpression).toBe('prop("Estimate") * 2');
  });

  it("serializes source definitions independently of draft presentation", () => {
    const draft: NotesDataSourceSchemaPropertyDraft[] = [
      ...notesDataSourceSchemaDraftFromDto(dataSource, view),
      {
        ...createNotesDataSourcePropertyDraft("status", "Status", "status"),
        options: [
          { id: "todo", name: "Todo", color: "default", group: "To-do" },
          { id: "doing", name: "Doing", color: "blue", group: "In progress" },
        ],
      },
      {
        ...createNotesDataSourcePropertyDraft("unique_id", "Task ID", "task_id"),
        uniquePrefix: "TASK",
      },
    ];
    draft[1] = { ...draft[1]!, name: "Effort" };

    const update = notesDataSourceSchemaUpdateFromDraft(draft);

    expect(Object.keys(update)).toEqual(["properties"]);
    expect(notesDataSourceSchemaUpdateFromDraft([...draft].reverse())).toEqual(update);
    expect(update.properties.Effort).toMatchObject({
      id: "estimate",
      name: "Effort",
      type: "number",
      number: { format: "percent" },
    });
    expect(update.properties.Status).toMatchObject({
      id: "status",
      type: "status",
      status: {
        groups: [
          { id: "To-do", name: "To-do", color: "gray", option_ids: ["todo"] },
          {
            id: "In progress",
            name: "In progress",
            color: "blue",
            option_ids: ["doing"],
          },
          { id: "Complete", name: "Complete", color: "green", option_ids: [] },
        ],
        default_option_id: null,
      },
    });
    expect(update.properties["Task ID"]).toMatchObject({
      unique_id: { prefix: "TASK" },
    });
    expect(update.properties.Project).toMatchObject({
      id: "project_relation",
      type: "relation",
      relation: {
        data_source_id: "66666666-6666-4666-8666-666666666666",
        dual_property: {
          synced_property_id: "tasks_relation",
          synced_property_name: "Tasks",
        },
      },
    });
    expect(update.properties["Project budget"]).toMatchObject({
      id: "project_budget",
      type: "rollup",
      rollup: {
        relation_property_id: "project_relation",
        relation_property_name: "Project",
        rollup_property_id: "budget",
        rollup_property_name: "Budget",
        function: "sum",
      },
    });
    expect(update.properties.Score).toMatchObject({
      id: "score_formula",
      type: "formula",
      formula: {
        expression: 'prop("Estimate") * 2',
      },
    });
  });

  it("round-trips the status default option and drops a default that no longer exists", () => {
    const status: NotesDataSourceSchemaPropertyDraft = {
      ...createNotesDataSourcePropertyDraft("status", "Status", "status"),
      options: [
        { id: "todo", name: "Todo", color: "default", group: "To-do" },
        { id: "doing", name: "Doing", color: "blue", group: "In progress" },
      ],
      statusDefaultOptionId: "doing",
    };
    const update = notesDataSourceSchemaUpdateFromDraft([status]);
    expect(update.properties.Status).toMatchObject({ status: { default_option_id: "doing" } });

    const reloaded = notesDataSourceSchemaDraftFromDto({ ...dataSource, properties: update.properties }, view);
    const reloadedStatus = reloaded.find((property) => property.id === "status");
    expect(reloadedStatus?.statusDefaultOptionId).toBe("doing");
    expect(reloadedStatus?.options.map((option) => [option.id, option.group])).toEqual([
      ["todo", "To-do"],
      ["doing", "In progress"],
    ]);

    const removed = notesDataSourceSchemaUpdateFromDraft([{ ...status, options: status.options.slice(0, 1) }]);
    expect(removed.properties.Status).toMatchObject({ status: { default_option_id: null } });
  });

  it("round-trips Lucide property icons and leaves other icons on the type icon", () => {
    const nativeIcon = { type: "icon", icon: { name: "star", color: "blue" } };
    const source: NotesDataSource = {
      ...dataSource,
      properties: { ...dataSource.properties, Priority: { ...(dataSource.properties.Priority as Record<string, unknown>), icon: nativeIcon } },
    };
    const draft = notesDataSourceSchemaDraftFromDto(source, view);
    const priority = draft.find((property) => property.id === "priority")!;
    expect(priority.icon).toBe(notesDataSourcePropertyIcon(nativeIcon));
    expect(priority.icon).not.toBeNull();
    expect(draft.find((property) => property.id === "estimate")?.icon).toBeNull();

    const update = notesDataSourceSchemaUpdateFromDraft([
      ...draft,
      createNotesDataSourcePropertyDraft("rich_text", "Emoji", "emoji", "emoji:🙂"),
    ]);
    expect(update.properties.Priority).toHaveProperty("icon", nativeIcon);
    expect(update.properties.Estimate).not.toHaveProperty("icon");
    expect(update.properties.Emoji).not.toHaveProperty("icon");
    expect(notesDataSourcePropertyIcon({ type: "emoji", emoji: "🙂" })).toBeNull();
    expect(notesDataSourcePropertyIcon({ type: "icon", icon: { name: "" } })).toBeNull();
    expect(notesDataSourcePropertyIcon({ type: "icon", icon: { name: "star", color: "neon" } }))
      .toBe(notesDataSourcePropertyIcon({ type: "icon", icon: { name: "star" } }));
  });

  it("rejects duplicate property names before persistence", () => {
    const draft = notesDataSourceSchemaDraftFromDto(dataSource, view);
    draft[1] = { ...draft[1]!, name: "Name" };

    expect(() => notesDataSourceSchemaUpdateFromDraft(draft)).toThrow(
      "property names must be unique",
    );
  });

  it("serializes database button actions with synced target references", () => {
    const title = createNotesDataSourcePropertyDraft("title", "Name", "title");
    const done = createNotesDataSourcePropertyDraft("checkbox", "Done", "done");
    const finish = createNotesDataSourcePropertyDraft("button", "Finish", "finish_button");
    const targetOptions = notesDataSourceButtonTargetOptions([title, done, finish], finish.id);
    const defaultPatch = notesDataSourceDefaultButtonPatch([title, done, finish], finish.id);
    const button = {
      ...finish,
      ...defaultPatch,
      buttonLabel: "Mark done",
      buttonRequiresConfirmation: true,
    };

    expect(targetOptions.map((property) => property.id)).toEqual(["title", "done"]);
    expect(button.buttonActionPropertyId).toBe("done");

    const synced = notesDataSourceSyncPropertyReferences(
      [title, { ...done, name: "Complete" }, { ...button, buttonActionPropertyId: "done" }],
      dataSource.id,
      [],
    );
    const update = notesDataSourceSchemaUpdateFromDraft(synced);

    expect(Object.keys(update)).toEqual(["properties"]);
    expect(update.properties.Finish).toMatchObject({
      id: "finish_button",
      name: "Finish",
      type: "button",
      button: {
        label: "Mark done",
        requires_confirmation: true,
        actions: [{
          type: "update_current_row_property",
          property_id: "done",
          property_name: "Complete",
          property_type: "checkbox",
          value: true,
        }],
      },
    });
  });
});

describe("notesDataSourceRenameProperty", () => {
  const properties = [
    createNotesDataSourcePropertyDraft("title", "Name", "title"),
    createNotesDataSourcePropertyDraft("number", "Points", "points"),
    createNotesDataSourcePropertyDraft("checkbox", "Done", "done"),
  ];

  it("renames only the chosen property and keeps its ID", () => {
    const result = notesDataSourceRenameProperty(properties, "points", "  Effort ");
    expect(result.status).toBe("renamed");
    if (result.status !== "renamed") return;
    expect(result.properties.map(({ id, name }) => ({ id, name }))).toEqual([
      { id: "title", name: "Name" }, { id: "points", name: "Effort" }, { id: "done", name: "Done" },
    ]);
    expect(properties[1].name).toBe("Points");
  });

  it("leaves blank and unchanged names alone and reports unknown properties", () => {
    expect(notesDataSourceRenameProperty(properties, "points", "   ")).toEqual({ status: "unchanged" });
    expect(notesDataSourceRenameProperty(properties, "points", " Points ")).toEqual({ status: "unchanged" });
    expect(notesDataSourceRenameProperty(properties, "missing", "Effort")).toEqual({ status: "missing" });
  });

  it("rejects another property's name in any letter case but allows changing only the case", () => {
    expect(notesDataSourceRenameProperty(properties, "points", "done")).toEqual({ status: "duplicate" });
    const result = notesDataSourceRenameProperty(properties, "points", "POINTS");
    expect(result.status === "renamed" && result.properties[1].name).toBe("POINTS");
  });

  it("limits the name to the native character limit", () => {
    const result = notesDataSourceRenameProperty(properties, "points", "😀".repeat(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS + 5));
    expect(result.status === "renamed" && Array.from(result.properties[1].name)).toHaveLength(NOTES_DATA_SOURCE_PROPERTY_NAME_MAX_CHARACTERS);
  });
});
