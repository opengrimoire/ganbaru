import { describe, expect, it } from "vitest";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { ProjectTask } from "$lib/projects/types";
import {
  PROJECT_TASK_FILTER_DEFAULTS,
  projectTaskActiveFilterChips,
  projectTaskDataFiltersActive,
  projectTaskFiltersActive,
  selectedProjectTaskIdsInView,
} from "./view";

function task(id: string): ProjectTask {
  return {
    id,
    projectId: "project-a",
    sectionId: "section-a",
    statusId: "status-a",
    title: id,
    description: "",
    priority: "normal",
    taskType: "task",
    sectionOrderKey: "1000",
    statusOrderKey: "1000",
    milestone: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

const t = ((key: string, ...args: unknown[]) => [key, ...args].join("|")) as Translate;

describe("project list view helpers", () => {
  it("removes selected tasks that are no longer visible", () => {
    expect(selectedProjectTaskIdsInView(["task-a", "task-c"], [task("task-a"), task("task-b")])).toEqual(["task-a"]);
  });

  it("separates data filters from presentation filters", () => {
    expect(projectTaskDataFiltersActive(PROJECT_TASK_FILTER_DEFAULTS)).toBe(false);
    expect(projectTaskFiltersActive(PROJECT_TASK_FILTER_DEFAULTS)).toBe(false);
    expect(projectTaskDataFiltersActive({
      ...PROJECT_TASK_FILTER_DEFAULTS,
      search: "release",
    })).toBe(true);
    expect(projectTaskDataFiltersActive({
      ...PROJECT_TASK_FILTER_DEFAULTS,
      groupBy: "status",
    })).toBe(false);
    expect(projectTaskFiltersActive({
      ...PROJECT_TASK_FILTER_DEFAULTS,
      groupBy: "status",
    })).toBe(true);
  });

  it("builds active filter chips with lookup labels", () => {
    expect(projectTaskActiveFilterChips({
      ...PROJECT_TASK_FILTER_DEFAULTS,
      search: "release",
      sectionFilter: "section-a",
      priorityFilter: "urgent",
      dueFilter: "range",
      dueRangeStart: "invalid",
      dueRangeEnd: "invalid",
      tagFilter: "tag-a",
      customFieldFilters: [{ fieldId: "field-a", mode: "option", optionId: "option-a" }],
      sections: [{
        id: "section-a",
        projectId: "project-a",
        name: "Backlog",
        orderKey: "1000",
        collapsed: false,
        createdAt: "2026-06-21T00:00:00.000Z",
        updatedAt: "2026-06-21T00:00:00.000Z",
      }],
      tags: [{
        id: "tag-a",
        projectId: "project-a",
        name: "Design",
        color: 2,
        orderKey: "1000",
        createdAt: "2026-06-21T00:00:00.000Z",
        updatedAt: "2026-06-21T00:00:00.000Z",
      }],
      customFields: [{
        id: "field-a",
        projectId: "project-a",
        name: "Stage",
        fieldType: "select",
        orderKey: "1000",
        icon: null,
        createdAt: "2026-06-21T00:00:00.000Z",
        updatedAt: "2026-06-21T00:00:00.000Z",
      }],
      customFieldOptionsForField: () => [{
        id: "option-a",
        fieldId: "field-a",
        name: "Discovery",
        orderKey: "1000",
        createdAt: "2026-06-21T00:00:00.000Z",
        updatedAt: "2026-06-21T00:00:00.000Z",
      }],
      normalizedDueRangeStart: "2026-06-21",
      normalizedDueRangeEnd: "2026-06-22",
      t,
    }).map((chip) => chip.label)).toEqual([
      "release",
      "Backlog",
      "projects.priority.urgent",
      "projects.filters.dueRangeChip|2026-06-21|2026-06-22",
      "Design",
      "projects.filters.customFieldChip|Stage|Discovery",
    ]);
  });
});
