import { describe, expect, it } from "vitest";
import type { ProjectPriorityConfig, ProjectStatus } from "$lib/projects/types";
import {
  projectListDueDateForGroup,
  projectListGroupQuickAddPlan,
  projectListGroupTaskCreateTarget,
  projectListGroupTaskDraftKey,
  projectListSectionTaskCreateTarget,
} from "./quick-add";

function status(id: string, terminal = false): ProjectStatus {
  return {
    id,
    projectId: "project-a",
    name: id,
    category: terminal ? "done" : "active",
    color: 0,
    sortOrder: 1000,
    terminal,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

function priority(id: string): ProjectPriorityConfig {
  return {
    id,
    projectId: "project-a",
    name: id,
    color: 0,
    sortOrder: 1000,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

const today = Temporal.PlainDate.from("2026-06-21");

describe("project list quick-add helpers", () => {
  it("builds stable draft keys and create targets", () => {
    expect(projectListSectionTaskCreateTarget("section-a")).toBe("section:section-a");
    expect(projectListGroupTaskDraftKey("priority", { value: "urgent" })).toBe("priority:urgent");
    expect(projectListGroupTaskCreateTarget("priority:urgent")).toBe("group:priority:urgent");
  });

  it("maps due groups to add-task dates from the provided day", () => {
    expect(projectListDueDateForGroup("overdue", today)).toBe("2026-06-20");
    expect(projectListDueDateForGroup("earlier", today)).toBe("2026-06-20");
    expect(projectListDueDateForGroup("today", today)).toBe("2026-06-21");
    expect(projectListDueDateForGroup("week", today)).toBe("2026-06-28");
    expect(projectListDueDateForGroup("later", today)).toBe("2026-06-29");
    expect(projectListDueDateForGroup("none", today)).toBeUndefined();
  });

  it("plans status and priority grouped quick-adds from existing project configuration", () => {
    expect(projectListGroupQuickAddPlan({
      groupBy: "status",
      group: { value: "status-active" },
      statuses: [status("status-active")],
      priorities: [],
      today,
    })).toEqual({
      enabled: true,
      statusId: "status-active",
      patch: {},
    });

    expect(projectListGroupQuickAddPlan({
      groupBy: "priority",
      group: { value: "urgent" },
      statuses: [],
      priorities: [priority("urgent")],
      today,
    })).toEqual({
      enabled: true,
      patch: { priority: "urgent" },
    });
  });

  it("rejects status and priority quick-adds when their configuration row is missing", () => {
    expect(projectListGroupQuickAddPlan({
      groupBy: "status",
      group: { value: "missing-status" },
      statuses: [status("status-active")],
      priorities: [],
      today,
    })).toEqual({
      enabled: false,
      statusId: undefined,
      patch: {},
    });

    expect(projectListGroupQuickAddPlan({
      groupBy: "priority",
      group: { value: "missing-priority" },
      statuses: [],
      priorities: [priority("urgent")],
      today,
    })).toEqual({
      enabled: false,
      patch: {},
    });
  });

  it("plans due grouped quick-adds and requires a terminal status for earlier done tasks", () => {
    expect(projectListGroupQuickAddPlan({
      groupBy: "due",
      group: { value: "today" },
      statuses: [],
      priorities: [],
      today,
    })).toEqual({
      enabled: true,
      statusId: undefined,
      patch: { dueDate: "2026-06-21" },
    });

    expect(projectListGroupQuickAddPlan({
      groupBy: "due",
      group: { value: "earlier" },
      statuses: [status("done", true)],
      priorities: [],
      today,
    })).toEqual({
      enabled: true,
      statusId: "done",
      patch: { dueDate: "2026-06-20" },
    });

    expect(projectListGroupQuickAddPlan({
      groupBy: "due",
      group: { value: "earlier" },
      statuses: [status("active")],
      priorities: [],
      today,
    }).enabled).toBe(false);
  });

  it("allows quick-add only for the unscheduled schedule group", () => {
    expect(projectListGroupQuickAddPlan({
      groupBy: "scheduled",
      group: { value: "unscheduled" },
      statuses: [],
      priorities: [],
      today,
    })).toEqual({
      enabled: true,
      patch: {},
    });

    expect(projectListGroupQuickAddPlan({
      groupBy: "scheduled",
      group: { value: "scheduled" },
      statuses: [],
      priorities: [],
      today,
    })).toEqual({
      enabled: false,
      patch: {},
    });
  });
});
