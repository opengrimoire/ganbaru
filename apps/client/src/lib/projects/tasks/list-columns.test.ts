import { describe, expect, it } from "vitest";
import {
  customTaskListColumn,
  DEFAULT_TASK_LIST_COLUMNS,
  parseTaskListColumns,
  taskListColumnsMatch,
  taskListColumnsForProject,
  taskListColumnsPreferenceValue,
  TASK_LIST_COLUMNS_PREFERENCE_KEY,
} from "./list-columns";
import type { ProjectViewPreference } from "$lib/projects/types";

function preference(value: string): ProjectViewPreference {
  return {
    projectId: "project",
    viewId: "list",
    preferenceKey: TASK_LIST_COLUMNS_PREFERENCE_KEY,
    preferenceValue: value,
    updatedAt: "2026-06-12T00:00:00.000Z",
  };
}

describe("task list columns", () => {
  it("defaults to status, dates, priority, assignee, and reviewer after the fixed name column", () => {
    expect(DEFAULT_TASK_LIST_COLUMNS).toEqual(["status", "start", "due", "priority", "assignee", "reviewer"]);
  });

  it("round-trips selected columns", () => {
    const columns = ["status", "due", "scheduled"] as const;

    expect(parseTaskListColumns(taskListColumnsPreferenceValue(columns))).toEqual(columns);
  });

  it("matches task list columns by order and value", () => {
    expect(taskListColumnsMatch(["status", "due"], ["status", "due"])).toBe(true);
    expect(taskListColumnsMatch(["status", "due"], ["due", "status"])).toBe(false);
  });

  it("deduplicates columns and drops unknown values", () => {
    expect(parseTaskListColumns(JSON.stringify({
      visibleColumns: ["status", "unknown", "status", "dependencies"],
    }))).toEqual(["status", "dependencies"]);
  });

  it("keeps custom field columns when the field still exists", () => {
    const riskColumn = customTaskListColumn("field-risk");
    const confidenceColumn = customTaskListColumn("field-confidence");

    expect(parseTaskListColumns(JSON.stringify({
      visibleColumns: ["status", riskColumn, confidenceColumn, riskColumn],
    }), new Set(["field-risk", "field-confidence"]))).toEqual([
      "status",
      riskColumn,
      confidenceColumn,
    ]);
  });

  it("drops stale custom field columns when loading project preferences", () => {
    const columns = taskListColumnsPreferenceValue([
      "status",
      customTaskListColumn("field-risk"),
      customTaskListColumn("deleted-field"),
    ]);

    expect(taskListColumnsForProject([preference(columns)], "project", new Set(["field-risk"]))).toEqual([
      "status",
      customTaskListColumn("field-risk"),
    ]);
  });

  it("defaults malformed values", () => {
    expect(parseTaskListColumns(undefined)).toEqual(DEFAULT_TASK_LIST_COLUMNS);
    expect(parseTaskListColumns("{")).toEqual(DEFAULT_TASK_LIST_COLUMNS);
    expect(parseTaskListColumns(JSON.stringify({ visibleColumns: "status" }))).toEqual(DEFAULT_TASK_LIST_COLUMNS);
  });

  it("loads the matching project list preference", () => {
    expect(taskListColumnsForProject([
      preference(taskListColumnsPreferenceValue(["priority"])),
      { ...preference(taskListColumnsPreferenceValue(["due"])), projectId: "other" },
      { ...preference(taskListColumnsPreferenceValue(["scheduled"])), viewId: "kanban" },
    ], "project")).toEqual(["priority"]);
  });
});
