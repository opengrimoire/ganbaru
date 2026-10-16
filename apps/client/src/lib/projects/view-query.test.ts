import { describe, expect, it } from "vitest";
import {
  defaultProjectViewQuery,
  parseProjectViewQuery,
  PROJECT_VIEW_QUERY_PREFERENCE_KEY,
  serializeProjectViewQuery,
  storedProjectViewQuery,
  type ProjectViewQuery,
} from "./view-query";

function customized(): ProjectViewQuery {
  const query = defaultProjectViewQuery();
  return {
    filters: {
      ...query.filters,
      statusFilter: "blocked",
      sectionFilter: "section-1",
      dueFilter: "range",
      dueRangeStart: "2026-10-01",
      dueRangeEnd: "2026-10-31",
      tagFilter: "none",
      customFieldFilters: [
        { fieldId: "field-1", mode: "checkbox", checked: false },
        { fieldId: "field-2", mode: "option", optionId: "option-1" },
      ],
      groupBy: "priority",
      sortMode: "custom:field-3",
      sortDirection: "desc",
    },
    showArchivedTasks: true,
    showInactiveSections: false,
    calendarMode: "month",
  };
}

describe("project view query", () => {
  it("round-trips a customized query through its stored JSON", () => {
    const query = customized();
    expect(parseProjectViewQuery(serializeProjectViewQuery(query))).toEqual(query);
  });

  it("starts unsaved and unreadable views from the defaults", () => {
    expect(parseProjectViewQuery(null)).toEqual(defaultProjectViewQuery());
    expect(parseProjectViewQuery("not json")).toEqual(defaultProjectViewQuery());
    expect(parseProjectViewQuery("[1, 2]")).toEqual(defaultProjectViewQuery());
    expect(defaultProjectViewQuery().filters).not.toHaveProperty("search");
  });

  it("resets only the malformed fields and keeps the rest of the view's setup", () => {
    const stored = JSON.parse(serializeProjectViewQuery(customized()));
    stored.filters.statusFilter = "sleeping";
    stored.filters.dueRangeStart = "October";
    stored.filters.sortMode = "custom:";
    stored.filters.groupBy = 4;
    stored.filters.customFieldFilters.push({ fieldId: "field-4", mode: "option" }, "field-5");
    stored.calendarMode = "year";

    const parsed = parseProjectViewQuery(JSON.stringify(stored));
    expect(parsed.filters).toMatchObject({
      statusFilter: "all",
      dueRangeStart: "",
      dueRangeEnd: "2026-10-31",
      sortMode: "manual",
      sortDirection: "desc",
      groupBy: "section",
      sectionFilter: "section-1",
    });
    expect(parsed.filters.customFieldFilters).toEqual(customized().filters.customFieldFilters);
    expect(parsed.showArchivedTasks).toBe(true);
    expect(parsed.calendarMode).toBeNull();
  });

  it("serializes equal queries to equal strings whatever their key order", () => {
    const query = customized();
    const reordered: ProjectViewQuery = {
      calendarMode: query.calendarMode,
      showInactiveSections: query.showInactiveSections,
      showArchivedTasks: query.showArchivedTasks,
      filters: { ...query.filters },
    };
    expect(serializeProjectViewQuery(reordered)).toBe(serializeProjectViewQuery(query));
  });

  it("reads only the query preference of the requested view", () => {
    const preference = (viewId: string, preferenceKey: string, preferenceValue: string) =>
      ({ projectId: "project-1", viewId, preferenceKey, preferenceValue, updatedAt: "u" });
    const preferences = [
      preference("view-a", "columns", "[]"),
      preference("view-b", PROJECT_VIEW_QUERY_PREFERENCE_KEY, "{\"b\":1}"),
      preference("view-a", PROJECT_VIEW_QUERY_PREFERENCE_KEY, "{\"a\":1}"),
    ];
    expect(storedProjectViewQuery(preferences, "view-a")).toBe("{\"a\":1}");
    expect(storedProjectViewQuery(preferences, "view-c")).toBeNull();
  });
});
