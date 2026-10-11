import { describe, expect, it } from "vitest";
import { createNotesDataSourcePropertyDraft, type NotesDataSourceSchemaPropertyDraft } from "./data-source-schema";
import {
  notesDataSourceDefaultFormulaExpression,
  notesDataSourcePropertyDefinition,
  planNewNotesDataSourceProperty,
  planNotesDataSourceTypeChange,
  type NotesDataSourcePropertyPlanContext,
} from "./property-planning";
import type { NotesDataSource } from "$lib/notes/types";

const SOURCE_ID = "11111111-1111-4111-8111-111111111111";
const OTHER_SOURCE_ID = "66666666-6666-4666-8666-666666666666";

function draft(type: NotesDataSourceSchemaPropertyDraft["type"], name: string, id: string): NotesDataSourceSchemaPropertyDraft {
  return createNotesDataSourcePropertyDraft(type, name, id);
}

const title = draft("title", "Name", "title");
const estimate = draft("number", "Estimate", "estimate");
const score = { ...draft("formula", "Score", "score"), formulaExpression: "1" };
const project = { ...draft("relation", "Project", "project"), relationDataSourceId: OTHER_SOURCE_ID };

const otherSource: NotesDataSource = {
  object: "data_source",
  id: OTHER_SOURCE_ID,
  parent: { type: "database_id", database_id: "22222222-2222-4222-8222-222222222222" },
  database_parent: { type: "page_id", page_id: "33333333-3333-4333-8333-333333333333" },
  title: "Projects",
  title_rich_text: [],
  description: [],
  icon: null,
  properties: {
    Name: { id: "title", name: "Name", description: "", type: "title", title: {} },
    Budget: { id: "budget", name: "Budget", description: "", type: "number", number: { format: "number" } },
  },
  in_trash: false,
  source_provider: null,
  source_object_id: null,
  source_workspace_id: null,
  source_last_edited_time: null,
  created_time: "2026-07-01T00:00:00.000Z",
  last_edited_time: "2026-07-01T00:00:00.000Z",
};

function context(drafts: readonly NotesDataSourceSchemaPropertyDraft[]): NotesDataSourcePropertyPlanContext {
  return { dataSourceId: SOURCE_ID, drafts, dataSources: [otherSource] };
}

describe("planNewNotesDataSourceProperty", () => {
  it("points a new relation at its own source", () => {
    const plan = planNewNotesDataSourceProperty("relation", "Parent task", null, context([title]));
    expect(plan.status).toBe("ready");
    if (plan.status === "ready") expect(plan.property.relationDataSourceId).toBe(SOURCE_ID);
  });

  it("reads a new rollup through the first relation and its first target", () => {
    const plan = planNewNotesDataSourceProperty("rollup", "Budget", "lucide:coins", context([title, project]));
    expect(plan.status).toBe("ready");
    if (plan.status !== "ready") return;
    expect(plan.property).toMatchObject({
      icon: "lucide:coins",
      rollupRelationPropertyId: "project",
      rollupRelationPropertyName: "Project",
      rollupFunction: "count",
    });
    expect(plan.property.rollupPropertyId).not.toBe("");
  });

  it("reports a rollup that has no relation to read through", () => {
    expect(planNewNotesDataSourceProperty("rollup", "Budget", null, context([title, estimate]))).toEqual({ status: "rollup_without_relation" });
  });

  it("starts a new formula from the first property that is not computed", () => {
    const plan = planNewNotesDataSourceProperty("formula", "Total", null, context([score, estimate, title]));
    expect(plan.status === "ready" && plan.property.formulaExpression).toBe("prop(\"Estimate\")");
  });

  it("targets a new button at the first checkbox", () => {
    const done = draft("checkbox", "Done", "done");
    const plan = planNewNotesDataSourceProperty("button", "Finish", null, context([title, done]));
    expect(plan.status === "ready" && plan.property).toMatchObject({ buttonActionPropertyId: "done", buttonActionValue: true });
  });

  it("does not change the drafts it reads", () => {
    const drafts = [title, project];
    const snapshot = structuredClone(drafts);
    planNewNotesDataSourceProperty("rollup", "Budget", null, context(drafts));
    expect(drafts).toEqual(snapshot);
  });
});

describe("planNotesDataSourceTypeChange", () => {
  it("sends no configuration for select-like types", () => {
    for (const type of ["select", "multi_select", "status"] as const) {
      expect(planNotesDataSourceTypeChange(estimate, type, context([title, estimate]))).toEqual({ status: "ready", config: null });
    }
  });

  it("keeps the property's name and icon in a button's label", () => {
    const iconic = { ...estimate, icon: "lucide:star" };
    const plan = planNotesDataSourceTypeChange(iconic, "button", context([title, iconic]));
    expect(plan.status === "ready" && plan.config).toMatchObject({ label: "Estimate" });
  });

  it("builds a formula that never refers to the converted property", () => {
    const plan = planNotesDataSourceTypeChange(estimate, "formula", context([estimate, title]));
    expect(plan.status === "ready" && plan.config).toEqual({ expression: "prop(\"Name\")" });
  });

  it("reports a rollup conversion without relations", () => {
    expect(planNotesDataSourceTypeChange(estimate, "rollup", context([title, estimate]))).toEqual({ status: "rollup_without_relation" });
  });
});

describe("notesDataSourcePropertyDefinition", () => {
  it("serializes a draft as the native definition keyed by its trimmed name", () => {
    const definition = notesDataSourcePropertyDefinition({ ...estimate, name: " Estimate ", pageVisibility: "hide_when_empty", pagePosition: 2 });
    expect(definition).toMatchObject({
      id: "estimate",
      name: "Estimate",
      type: "number",
      number: { format: "number" },
      page_visibility: "hide_when_empty",
      page_position: 2,
    });
  });

  it("rejects a draft whose type reference is missing", () => {
    expect(() => notesDataSourcePropertyDefinition(draft("relation", "Link", "link"))).toThrow();
  });
});

describe("notesDataSourceDefaultFormulaExpression", () => {
  it("escapes quotes and backslashes in the referenced name", () => {
    expect(notesDataSourceDefaultFormulaExpression("x", [{ id: "a", name: "Say \"hi\" \\", type: "rich_text" }]))
      .toBe("prop(\"Say \\\"hi\\\" \\\\\")");
  });

  it("falls back to an empty string when only computed properties exist", () => {
    expect(notesDataSourceDefaultFormulaExpression("x", [score, { ...score, id: "r", type: "rollup" }])).toBe("\"\"");
  });
});
