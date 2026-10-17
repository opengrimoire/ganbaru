import { describe, expect, it } from "vitest";
import type { ProjectTask } from "$lib/projects/types";
import {
  projectListDueDateEditPlan,
  projectListStartDateEditPlan,
} from "./date-edit";

function task(fields: Partial<ProjectTask> = {}): Pick<ProjectTask, "id" | "archivedAt" | "startDate" | "dueDate"> {
  return {
    id: "task-a",
    ...fields,
  };
}

describe("project list date edit helpers", () => {
  it("closes the edited menu without updates for archived tasks", () => {
    expect(projectListStartDateEditPlan({
      task: task({ archivedAt: "2026-06-21T00:00:00.000Z" }),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "start",
      nextOpenMenu: null,
    });

    expect(projectListDueDateEditPlan({
      task: task({ archivedAt: "2026-06-21T00:00:00.000Z" }),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "due",
      nextOpenMenu: null,
    });
  });

  it("clears only existing start or due dates", () => {
    expect(projectListStartDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: undefined,
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "start",
      nextOpenMenu: null,
      patch: { startDate: undefined },
    });

    expect(projectListDueDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: undefined,
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "due",
      nextOpenMenu: null,
      patch: { dueDate: undefined },
    });

    expect(projectListStartDateEditPlan({
      task: task(),
      selectedDate: undefined,
      rangeDateColumnsVisible: true,
    }).patch).toBeUndefined();
    expect(projectListDueDateEditPlan({
      task: task(),
      selectedDate: undefined,
      rangeDateColumnsVisible: true,
    }).patch).toBeUndefined();
  });

  it("selects start and due date ranges while preserving valid opposite endpoints", () => {
    expect(projectListStartDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "start",
      nextOpenMenu: null,
      patch: {
        startDate: "2026-06-22",
        dueDate: "2026-06-24",
      },
    });

    expect(projectListDueDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "due",
      nextOpenMenu: null,
      patch: {
        startDate: "2026-06-20",
        dueDate: "2026-06-22",
      },
    });
  });

  it("moves crossed opposite endpoints to keep the range valid", () => {
    expect(projectListStartDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-26",
      rangeDateColumnsVisible: true,
    }).patch).toEqual({
      startDate: "2026-06-26",
      dueDate: "2026-06-26",
    });

    expect(projectListDueDateEditPlan({
      task: task({ startDate: "2026-06-20", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-18",
      rangeDateColumnsVisible: true,
    }).patch).toEqual({
      startDate: "2026-06-18",
      dueDate: "2026-06-18",
    });
  });

  it("prompts for the missing opposite endpoint only when both date columns are visible", () => {
    expect(projectListStartDateEditPlan({
      task: task(),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "start",
      nextOpenMenu: "due",
      patch: {
        startDate: "2026-06-22",
        dueDate: undefined,
      },
    });

    expect(projectListDueDateEditPlan({
      task: task(),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    })).toEqual({
      closeMenu: "due",
      nextOpenMenu: "start",
      patch: {
        startDate: undefined,
        dueDate: "2026-06-22",
      },
    });

    expect(projectListStartDateEditPlan({
      task: task(),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: false,
    }).nextOpenMenu).toBeNull();
    expect(projectListDueDateEditPlan({
      task: task(),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: false,
    }).nextOpenMenu).toBeNull();
  });

  it("does not produce update patches for unchanged selections", () => {
    expect(projectListStartDateEditPlan({
      task: task({ startDate: "2026-06-22", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-22",
      rangeDateColumnsVisible: true,
    }).patch).toBeUndefined();

    expect(projectListDueDateEditPlan({
      task: task({ startDate: "2026-06-22", dueDate: "2026-06-24" }),
      selectedDate: "2026-06-24",
      rangeDateColumnsVisible: true,
    }).patch).toBeUndefined();
  });
});
