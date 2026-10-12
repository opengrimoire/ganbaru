import { describe, expect, it } from "vitest";
import type { ProjectSavedTaskView, ProjectViewPreference } from "$lib/projects/types";
import { customFieldReference, customTaskListColumn } from "./list-columns";
import {
  createProjectSavedTaskViewSnapshot,
  parseSavedTaskViewPreference,
  projectCustomFieldFilterStillExists,
  projectTaskFilterStateFromSavedTaskView,
  savedTaskViewPreferenceKey,
  savedTaskViewPreferenceValue,
} from "./saved-views";

function preference(overrides: Partial<ProjectViewPreference>): ProjectViewPreference {
  return {
    projectId: "project",
    viewId: "list",
    preferenceKey: savedTaskViewPreferenceKey("view"),
    preferenceValue: "{}",
    updatedAt: "2026-06-12T00:00:00.000Z",
    ...overrides,
  };
}

function savedView(overrides: Partial<ProjectSavedTaskView> = {}): ProjectSavedTaskView {
  return {
    id: "view",
    projectId: "project",
    name: "Blocked this week",
    viewId: "kanban",
    search: "api",
    statusFilter: "blocked",
    sectionFilter: "section-a",
    priorityFilter: "urgent",
    dueFilter: "week",
    dueRangeStart: "",
    dueRangeEnd: "",
    scheduleFilter: "unscheduled",
    dependencyFilter: "blocked_by",
    tagFilter: "tag-backend",
    customFieldFilters: [],
    sortMode: "priority",
    sortDirection: "desc",
    groupBy: "section",
    collapsedSectionIds: ["section-a", "section-b"],
    showArchivedTasks: true,
    visibleColumns: ["status", "due", "scheduled"],
    updatedAt: "2026-06-12T00:00:00.000Z",
    ...overrides,
  };
}

describe("saved task views", () => {
  it("round-trips a saved task view through a preference row", () => {
    const view = savedView();
    const result = parseSavedTaskViewPreference(preference({
      viewId: "kanban",
      preferenceValue: savedTaskViewPreferenceValue(view),
    }));

    expect(result).toEqual(view);
  });

  it("builds saved task view snapshots from current filter state", () => {
    expect(createProjectSavedTaskViewSnapshot({
      id: "view-a",
      projectId: "project-a",
      name: "Urgent work",
      viewId: "list",
      search: "launch",
      statusFilter: "open",
      sectionFilter: "section-a",
      priorityFilter: "urgent",
      dueFilter: "range",
      dueRangeStart: "2026-06-21",
      dueRangeEnd: "2026-06-30",
      scheduleFilter: "scheduled",
      dependencyFilter: "linked",
      tagFilter: "tag-a",
      customFieldFilters: [{ fieldId: "field-a", mode: "filled" }],
      groupBy: "status",
      sortMode: "due",
      sortDirection: "desc",
      collapsedSectionIds: ["section-a"],
      showArchivedTasks: true,
      visibleColumns: ["status", "due"],
      updatedAt: "2026-06-21T00:00:00.000Z",
    })).toEqual({
      id: "view-a",
      projectId: "project-a",
      name: "Urgent work",
      viewId: "list",
      search: "launch",
      statusFilter: "open",
      sectionFilter: "section-a",
      priorityFilter: "urgent",
      dueFilter: "range",
      dueRangeStart: "2026-06-21",
      dueRangeEnd: "2026-06-30",
      scheduleFilter: "scheduled",
      dependencyFilter: "linked",
      tagFilter: "tag-a",
      customFieldFilters: [{ fieldId: "field-a", mode: "filled" }],
      sortMode: "due",
      sortDirection: "desc",
      groupBy: "status",
      collapsedSectionIds: ["section-a"],
      showArchivedTasks: true,
      visibleColumns: ["status", "due"],
      updatedAt: "2026-06-21T00:00:00.000Z",
    });
  });

  it("derives task filter state from a saved view", () => {
    expect(projectTaskFilterStateFromSavedTaskView(savedView({
      customFieldFilters: [{ fieldId: "field-risk", mode: "filled" }],
      groupBy: "priority",
    }))).toEqual({
      search: "api",
      statusFilter: "blocked",
      sectionFilter: "section-a",
      priorityFilter: "urgent",
      dueFilter: "week",
      dueRangeStart: "",
      dueRangeEnd: "",
      scheduleFilter: "unscheduled",
      dependencyFilter: "blocked_by",
      tagFilter: "tag-backend",
      customFieldFilters: [{ fieldId: "field-risk", mode: "filled" }],
      groupBy: "priority",
      sortMode: "priority",
      sortDirection: "desc",
    });
  });

  it("checks whether saved custom field filters still reference live values", () => {
    const fieldIds = new Set(["field-risk"]);
    const optionIds = new Set(["option-high"]);

    expect(projectCustomFieldFilterStillExists({ fieldId: "field-risk", mode: "filled" }, fieldIds, optionIds)).toBe(
      true,
    );
    expect(projectCustomFieldFilterStillExists({
      fieldId: "field-risk",
      mode: "option",
      optionId: "option-high",
    }, fieldIds, optionIds)).toBe(true);
    expect(projectCustomFieldFilterStillExists({
      fieldId: "field-risk",
      mode: "option",
      optionId: "option-deleted",
    }, fieldIds, optionIds)).toBe(false);
  });

  it("defaults stale enum values instead of trusting persisted data", () => {
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: JSON.stringify({
        name: "Stale",
        viewId: "unknown",
        statusFilter: "later",
        sectionFilter: "",
        priorityFilter: "maximum",
        dueFilter: "soon",
        scheduleFilter: "maybe",
        dependencyFilter: "crossed",
        tagFilter: "",
        sortMode: "weight",
        sortDirection: "sideways",
        groupBy: "custom",
        collapsedSectionIds: ["section-a", "", "section-a", 12],
        showArchivedTasks: "yes",
        visibleColumns: ["status", "unknown", "status"],
      }),
    }));

    expect(result).toMatchObject({
      name: "Stale",
      viewId: "list",
      search: "",
      statusFilter: "all",
      sectionFilter: "all",
      priorityFilter: "maximum",
      dueFilter: "all",
      dueRangeStart: "",
      dueRangeEnd: "",
      scheduleFilter: "all",
      dependencyFilter: "all",
      tagFilter: "all",
      sortMode: "manual",
      sortDirection: "asc",
      groupBy: "section",
      collapsedSectionIds: ["section-a"],
      customFieldFilters: [],
      showArchivedTasks: false,
      visibleColumns: ["status"],
    });
  });

  it("keeps saved custom field filters and sorting when the field still exists", () => {
    const customSortMode = customFieldReference("field-risk");
    const view = savedView({
      customFieldFilters: [{ fieldId: "field-risk", mode: "option", optionId: "option-high" }],
      sortMode: customSortMode,
    });
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: savedTaskViewPreferenceValue(view),
    }), new Set(["field-risk"]), new Set(["option-high"]));

    expect(result?.customFieldFilters).toEqual([
      { fieldId: "field-risk", mode: "option", optionId: "option-high" },
    ]);
    expect(result?.sortMode).toBe(customSortMode);
  });

  it("keeps valid due date ranges and drops malformed range dates", () => {
    const view = savedView({
      dueFilter: "range",
      dueRangeStart: "2026-06-01",
      dueRangeEnd: "2026-06-30",
    });
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: savedTaskViewPreferenceValue(view),
    }));

    expect(result?.dueFilter).toBe("range");
    expect(result?.dueRangeStart).toBe("2026-06-01");
    expect(result?.dueRangeEnd).toBe("2026-06-30");

    const staleResult = parseSavedTaskViewPreference(preference({
      preferenceValue: JSON.stringify({
        ...JSON.parse(savedTaskViewPreferenceValue(view)),
        dueRangeStart: "2026-02-30",
        dueRangeEnd: "bad",
      }),
    }));

    expect(staleResult?.dueRangeStart).toBe("");
    expect(staleResult?.dueRangeEnd).toBe("");
  });

  it("keeps saved list grouping modes", () => {
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: savedTaskViewPreferenceValue(savedView({
        groupBy: "status",
      })),
    }));

    expect(result?.groupBy).toBe("status");
  });

  it("drops stale custom field filters and custom sort modes", () => {
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: savedTaskViewPreferenceValue(savedView({
        customFieldFilters: [
          { fieldId: "field-risk", mode: "filled" },
          { fieldId: "field-stage", mode: "option", optionId: "option-deleted" },
        ],
        sortMode: customFieldReference("field-deleted"),
      })),
    }), new Set(["field-risk", "field-stage"]), new Set(["option-live"]));

    expect(result?.customFieldFilters).toEqual([
      { fieldId: "field-risk", mode: "filled" },
    ]);
    expect(result?.sortMode).toBe("manual");
  });

  it("keeps saved custom field columns only when the field still exists", () => {
    const riskColumn = customTaskListColumn("field-risk");
    const deletedColumn = customTaskListColumn("field-deleted");
    const result = parseSavedTaskViewPreference(preference({
      preferenceValue: savedTaskViewPreferenceValue(savedView({
        visibleColumns: ["status", riskColumn, deletedColumn],
      })),
    }), new Set(["field-risk"]));

    expect(result?.visibleColumns).toEqual(["status", riskColumn]);
  });

  it("ignores malformed preference rows", () => {
    expect(parseSavedTaskViewPreference(preference({ preferenceKey: "other" }))).toBeUndefined();
    expect(parseSavedTaskViewPreference(preference({ preferenceKey: savedTaskViewPreferenceKey("") }))).toBeUndefined();
    expect(parseSavedTaskViewPreference(preference({ preferenceValue: "{" }))).toBeUndefined();
    expect(parseSavedTaskViewPreference(preference({ preferenceValue: JSON.stringify({ name: "" }) }))).toBeUndefined();
  });
});
