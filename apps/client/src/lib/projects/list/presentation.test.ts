import { describe, expect, it } from "vitest";
import type { ProjectCustomField, ProjectTask } from "$lib/projects/types";
import { defaultProjectListPresentation, parseProjectListPresentation, projectListColumnGroupMode, projectListDateLabel, projectListFrozenOffsets, projectListNumberLabel, projectListPresentationForView, projectListRowColor, projectListTimeLabel } from "./presentation";

const fields: ProjectCustomField[] = [
  { id: "text", projectId: "project", name: "Text", fieldType: "text", sortOrder: 1, icon: null, createdAt: "", updatedAt: "" },
  { id: "number", projectId: "project", name: "Number", fieldType: "number", sortOrder: 2, icon: null, createdAt: "", updatedAt: "" },
  { id: "date", projectId: "project", name: "Date", fieldType: "date", sortOrder: 3, icon: null, createdAt: "", updatedAt: "" },
];

describe("Projects table presentation", () => {
  it("removes stale properties, invalid calculations, and incompatible formats from durable preferences", () => {
    const parsed = parseProjectListPresentation({
      wrappedColumns: ["name", "custom:text", "custom:removed", "name", 1], frozenThrough: "custom:removed",
      calculations: { "custom:text": "sum", "custom:number": "average", name: "count", due: "injected" },
      dateFormats: { due: "iso", "custom:date": "relative", "custom:number": "relative" },
      timeFormats: { start: "hidden", "custom:date": "hidden" },
      numberFormats: { "custom:number": "percent", estimate: "percent" },
      colorRules: [{id: "valid", property: "status", value: "active", color: 0}, {id: "valid", property: "priority", value: "high", color: 3}, {id: "invalid", property: "status", value: "active", color: -1}],
    }, fields);
    expect(parsed).toEqual({ wrappedColumns: ["name", "custom:text"], frozenThrough: null,
      calculations: { name: "count", "custom:number": "average" },
      dateFormats: { due: "iso", "custom:date": "relative" }, timeFormats: { start: "hidden" }, numberFormats: { "custom:number": "percent" },
      colorRules: [{id: "valid", property: "status", value: "active", color: 0}] });
  });

  it("restores only the current list view preference and tolerates malformed JSON", () => {
    const preference = {projectId: "project", viewId: "view-list", preferenceKey: "list-presentation", preferenceValue: "broken", updatedAt: ""};
    expect(projectListPresentationForView([preference], "view-list", fields)).toEqual(defaultProjectListPresentation());
    preference.preferenceValue = JSON.stringify({ frozenThrough: "custom:text" });
    expect(projectListPresentationForView([preference], "view-other", fields).frozenThrough).toBeNull();
    expect(projectListPresentationForView([preference], "view-list", fields).frozenThrough).toBe("custom:text");
  });

  it("freezes the leading selection, name, and visible columns through the stable selected property", () => {
    const presentation = { ...defaultProjectListPresentation(), frozenThrough: "due" as const };
    const input = {columns: ["status", "due", "priority"] as const, columnWidths: { name: 30, status: 10, due: 12 }};
    expect(projectListFrozenOffsets(presentation, input)).toEqual({ selection: 0, expand: 1.5, name: 3.25, status: 33.25, due: 43.25 });
    expect(projectListFrozenOffsets(presentation, { ...input, columnWidths: { ...input.columnWidths, name: 25 } }).due).toBe(38.25);
    expect(projectListFrozenOffsets(presentation, { columns: ["priority"] })).toEqual({});
    expect(projectListFrozenOffsets(presentation, input, 22)).toEqual({selection: 0, expand: 1.5});
    expect(projectListFrozenOffsets(presentation, input, 80)).toEqual(projectListFrozenOffsets(presentation, input));
  });

  it("applies the first matching row color so moving a condition changes precedence", () => {
    const task = { statusId: "active", priority: "high" } as ProjectTask;
    const status = { id: "status-rule", property: "status" as const, value: "active", color: 0 };
    const priority = { id: "priority-rule", property: "priority" as const, value: "high", color: 4 };
    expect(projectListRowColor(task, [status, priority])).toBe(0);
    expect(projectListRowColor(task, [priority, status])).toBe(4);
    expect(projectListRowColor(task, [{...status, value: "other"}])).toBeUndefined();
  });

  it("formats date, time, and number displays without changing their canonical values", () => {
    expect(projectListDateLabel("2026-07-11", "iso", "es", "2026-07-11")).toBe("2026-07-11");
    expect(projectListDateLabel("2026-07-12", "relative", "en", "2026-07-11")).toBe("tomorrow");
    expect(projectListDateLabel("2026-07-11", "locale", "en", "2026-07-11")).toBe("Jul 11, 2026");
    expect(projectListTimeLabel("14:30", "24_hour", "en")).toBe("14:30");
    expect(projectListTimeLabel("14:30", "12_hour", "en")).toContain("2:30");
    expect(projectListTimeLabel("14:30", "hidden", "es")).toBe("");
    expect(projectListNumberLabel(1234.5, "commas", "en")).toBe("1,234.5");
    expect(projectListNumberLabel(0.25, "percent", "es")).toContain("25");
    expect(projectListNumberLabel(1234.5, "number", "en")).toBe("1234.5");
  });

  it("groups only by columns that have a task grouping", () => {
    expect(projectListColumnGroupMode("status")).toBe("status");
    expect(projectListColumnGroupMode("priority")).toBe("priority");
    expect(projectListColumnGroupMode("due")).toBe("due");
    expect(projectListColumnGroupMode("scheduled")).toBe("scheduled");
    expect(projectListColumnGroupMode("name")).toBeNull();
    expect(projectListColumnGroupMode("start")).toBeNull();
    expect(projectListColumnGroupMode("custom:points")).toBeNull();
  });
});
