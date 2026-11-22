import { describe, expect, it } from "vitest";
import type { ProjectCustomField, ProjectCustomFieldType, ProjectStatus, ProjectTask } from "$lib/projects/types";
import {
  doubleClickProjectTaskListColumnResizeWidths,
  keyboardProjectTaskListColumnResizeWidths,
  moveProjectTaskListColumnResize,
  parseProjectTaskListColumnWidths,
  projectTaskListAutoColumnWidths,
  projectTaskListColumnTrack,
  projectTaskListColumnWidthRem,
  projectTaskListDoubleClickColumnWidthRem,
  projectTaskListGridMinWidth,
  projectTaskListGridTemplate,
  projectTaskListNameTrackWidth,
  projectTaskListLeadingGridTemplate,
  startProjectTaskListColumnResize,
  taskListColumnWidthsForView,
  taskListColumnWidthsPreferenceValue,
  TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY,
  type ProjectTaskListGridInput,
} from "./column-widths";

function task(id: string, patch: Partial<ProjectTask> = {}): ProjectTask {
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
    ...patch,
  };
}

function status(name: string): ProjectStatus {
  return {
    id: `status-${name}`,
    projectId: "project-a",
    name,
    category: "active",
    color: 0,
    orderKey: "1000",
    terminal: false,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

function customField(id: string, name: string, fieldType: ProjectCustomFieldType = "text"): ProjectCustomField {
  return {
    id,
    projectId: "project-a",
    name,
    fieldType,
    orderKey: "1000",
    icon: null,
    createdAt: "2026-06-21T00:00:00.000Z",
    updatedAt: "2026-06-21T00:00:00.000Z",
  };
}

/** Half a rem per character, so expected widths are easy to derive: 1rem padding plus 0.25rem slack around content. */
const measureText = (text: string) => text.length * 0.5;

function measured(input: ProjectTaskListGridInput): ProjectTaskListGridInput {
  return { measureText, columnLabel: (column) => column, ...input };
}

describe("project list column widths", () => {
  it("fits the whole header label, including its property icon", () => {
    const input = measured({ columns: ["priority"], columnLabel: () => "Priorität" });
    expect(projectTaskListColumnWidthRem("priority", input)).toBe(5.75);
    expect(projectTaskListColumnWidthRem("priority", { ...input, headerIcons: true })).toBe(7);
  });

  it("keeps short columns at a shared minimum that manual resizing also uses", () => {
    expect(projectTaskListColumnWidthRem("assignee", measured({ columns: ["assignee"], columnLabel: () => "Who" }))).toBe(4);
    expect(keyboardProjectTaskListColumnResizeWidths({
      column: "assignee",
      direction: -1,
      wideStep: true,
      widths: { assignee: 5 },
      gridInput: measured({ columns: ["assignee"], columnWidths: { assignee: 5 } }),
    })).toEqual({ assignee: 4 });
  });

  it("caps long headers until the column is resized", () => {
    const label = "Extremely long custom field header that should still have a limit";
    expect(projectTaskListColumnWidthRem("custom:field-a", measured({
      columns: ["custom:field-a"],
      customFields: [customField("field-a", label)],
      columnLabel: () => label,
    }))).toBe(16);
  });

  it("fits every status badge so changing status never truncates", () => {
    const input = measured({ columns: ["status"], statuses: [status("Todo"), status("In review")] });
    expect(projectTaskListColumnWidthRem("status", input)).toBeCloseTo(1 + 1.875 + 4.5 + 0.25, 1);
  });

  it("caps content at a per-column limit while double-click still fits it in full", () => {
    const input = measured({ columns: ["status"], statuses: [status("Needs outside review before release")] });
    expect(projectTaskListColumnWidthRem("status", input)).toBe(12);
    expect(projectTaskListDoubleClickColumnWidthRem("status", input)).toBeCloseTo(1 + 1.875 + 17.5 + 0.25);
  });

  it("fits every configured priority beside its flag", () => {
    const flag = 13 / 15 + 0.375;
    expect(projectTaskListColumnWidthRem("priority", measured({
      columns: ["priority"],
      priorityLabels: ["Low", "Sehr dringend"],
      tasks: [task("task-a")],
      priorityLabel: () => "Low",
    }))).toBeCloseTo(1 + flag + 6.5 + 0.25);
  });

  it("fits the widest month and the empty placeholder of date columns", () => {
    const input = measured({
      columns: ["due"],
      today: "2026-11-22",
      emptyDateLabel: () => "No due date",
      dateLabel: (_column, date, time) => {
        const label = date === "2026-09-28" ? "September 28, 2026" : "May 1";
        return time ? `${label} at ${time}` : label;
      },
    });
    expect(projectTaskListColumnWidthRem("due", input)).toBe(10.25);
    expect(projectTaskListColumnWidthRem("due", { ...input, today: undefined })).toBe(6.75);
    expect(projectTaskListColumnWidthRem("due", {
      ...input,
      today: undefined,
      tasks: [task("task-a", { dueDate: "2026-06-21", dueTime: "12:30" })],
    })).toBe(8.25);
  });

  it("sizes custom field cells for what each field type shows", () => {
    const fields = [
      customField("url", "Site", "url"),
      customField("done", "Done", "checkbox"),
      customField("stage", "Stage", "select"),
      customField("notes", "Notes"),
    ];
    const input = measured({
      columns: fields.map((field) => `custom:${field.id}` as const),
      customFields: fields,
      columnLabel: (column) => fields.find((field) => `custom:${field.id}` === column)?.name ?? column,
      tasks: [task("task-a")],
      emptyValueLabel: "Empty",
      customFieldCellLabel: (_task, field) => field.id === "url" ? "example.com" : field.id === "notes" ? "x".repeat(60) : undefined,
      customFieldOptionNames: () => ["Waiting on customer"],
    });
    expect(projectTaskListColumnWidthRem("custom:url", input)).toBeCloseTo(1 + 13 / 15 + 0.375 + 0.5 + 2 / 15 + 5.5 + 0.25, 1);
    expect(projectTaskListColumnWidthRem("custom:done", input)).toBe(4);
    expect(projectTaskListColumnWidthRem("custom:stage", input)).toBeCloseTo(1 + 0.75 + 0.5 + 13 / 15 + 9.5 + 0.25, 1);
    expect(projectTaskListColumnWidthRem("custom:notes", input)).toBe(14);
  });

  it("fits both dependency badges side by side", () => {
    const badge = 0.75 + 2 / 15;
    expect(projectTaskListColumnWidthRem("dependencies", measured({
      columns: ["dependencies"],
      tasks: [task("task-a")],
      dependencyLabels: () => ["Blocked by 2", "Blocks 1"],
    }))).toBeCloseTo(1 + 2 * badge + 0.25 + 10 + 0.25, 1);
  });

  it("estimates widths from character counts when text is not measured", () => {
    expect(projectTaskListColumnTrack("custom:field-a")).toBe("7.13rem");
    expect(projectTaskListGridTemplate(["status", "custom:field-a"])).toBe("1.5rem 1.75rem 16rem 4rem 7.13rem 2.25rem");
    expect(projectTaskListLeadingGridTemplate(["status", "custom:field-a"])).toBe("1.5rem 1.75rem minmax(0, 1fr)");
    expect(projectTaskListGridMinWidth([])).toBe("21.5rem");
  });

  it("sizes the name column from titles within its bounds", () => {
    expect(projectTaskListGridTemplate(measured({ columns: [], tasks: [task("Short")] }))).toBe("1.5rem 1.75rem 16rem 2.25rem");
    expect(projectTaskListGridTemplate(measured({ columns: [], tasks: [task("t".repeat(36))] }))).toBe("1.5rem 1.75rem 19.35rem 2.25rem");
    expect(projectTaskListNameTrackWidth(measured({ columns: [], tasks: [task("t".repeat(36))] }))).toBe("19.35rem");
    expect(projectTaskListGridTemplate(measured({ columns: [], tasks: [task("t".repeat(80))] }))).toBe("1.5rem 1.75rem 21.5rem 2.25rem");
  });

  it("reuses precomputed automatic widths and lets manual widths win over them", () => {
    const input = measured({ columns: ["status"], statuses: [status("In review")] });
    const autoColumnWidths = projectTaskListAutoColumnWidths({ ...input, columnWidths: { status: 20 } });
    expect(autoColumnWidths.status).toBe(projectTaskListColumnWidthRem("status", input));
    expect(projectTaskListColumnWidthRem("status", { ...input, statuses: [], autoColumnWidths })).toBe(autoColumnWidths.status);
    expect(projectTaskListColumnWidthRem("status", { ...input, autoColumnWidths, columnWidths: { status: 9 } })).toBe(9);
  });

  it("applies manual column widths over automatic list sizing", () => {
    expect(projectTaskListGridTemplate({
      columns: ["status", "due"],
      columnWidths: {
        name: 18,
        status: 9,
        due: 14,
      },
    })).toBe("1.5rem 1.75rem 18rem 9rem 14rem 2.25rem");
    expect(projectTaskListLeadingGridTemplate({
      columns: ["status", "due"],
      columnWidths: { name: 18 },
    })).toBe("1.5rem 1.75rem minmax(0, 1fr)");
  });

  it("starts column resize gestures from the measured width", () => {
    const widthsAtStart = { name: 18, status: 9 };

    expect(startProjectTaskListColumnResize({
      column: "name",
      pointerId: 2,
      startClientX: 100,
      startWidthRem: 11.234,
      rootFontSizePx: 0,
      widthsAtStart,
    })).toEqual({
      column: "name",
      pointerId: 2,
      startClientX: 100,
      startWidthRem: 11.234,
      rootFontSizePx: 16,
      widthsAtStart,
      draftWidths: { name: 12, status: 9 },
      moved: false,
    });
  });

  it("moves column resize gestures from pointer deltas", () => {
    const gesture = startProjectTaskListColumnResize({
      column: "status",
      pointerId: 3,
      startClientX: 100,
      startWidthRem: 8,
      rootFontSizePx: 16,
      widthsAtStart: { name: 18 },
    });

    const smallMove = moveProjectTaskListColumnResize(gesture, 100.5);
    expect(smallMove.draftWidths).toEqual({ name: 18, status: 8.03 });
    expect(smallMove.moved).toBe(false);

    const committedMove = moveProjectTaskListColumnResize(smallMove, 108);
    expect(committedMove.draftWidths).toEqual({ name: 18, status: 8.5 });
    expect(committedMove.moved).toBe(true);
  });

  it("updates column widths from keyboard resize steps", () => {
    expect(keyboardProjectTaskListColumnResizeWidths({
      column: "priority",
      direction: 1,
      widths: { name: 18, priority: 6 },
      gridInput: {
        columns: ["priority"],
        columnWidths: { priority: 6 },
      },
    })).toEqual({ name: 18, priority: 6.5 });

    expect(keyboardProjectTaskListColumnResizeWidths({
      column: "priority",
      direction: -1,
      wideStep: true,
      widths: { name: 18, priority: 7 },
      gridInput: {
        columns: ["priority"],
        columnWidths: { priority: 7 },
      },
    })).toEqual({ name: 18, priority: 5 });
  });

  it("updates column widths from double-click resize", () => {
    expect(doubleClickProjectTaskListColumnResizeWidths({
      column: "priority",
      widths: { name: 18, priority: 6 },
      gridInput: {
        columns: ["priority"],
        columnWidths: { priority: 6 },
      },
    })).toEqual({ name: 18 });

    expect(doubleClickProjectTaskListColumnResizeWidths({
      column: "status",
      widths: { name: 18, status: 9 },
      gridInput: measured({
        columns: ["status"],
        statuses: [status("Needs outside review before release")],
      }),
    }).status).toBeCloseTo(20.63, 1);
  });

  it("round-trips list column width preferences with validation", () => {
    const value = taskListColumnWidthsPreferenceValue({
      name: 11,
      status: 9,
      due: 80,
      "custom:field-a": 12,
    });
    expect(parseProjectTaskListColumnWidths(value, new Set(["field-a"]))).toEqual({
      "custom:field-a": 12,
      due: 64,
      name: 12,
      status: 9,
    });
    expect(parseProjectTaskListColumnWidths(value, new Set(["other"]))).toEqual({
      due: 64,
      name: 12,
      status: 9,
    });
  });

  it("loads list column widths from the matching list view preference", () => {
    expect(taskListColumnWidthsForView([
      {
        projectId: "project-a",
        viewId: "view-a-list",
        preferenceKey: TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY,
        preferenceValue: taskListColumnWidthsPreferenceValue({ name: 20, priority: 7 }),
        updatedAt: "2026-06-21T00:00:00.000Z",
      },
      {
        projectId: "project-b",
        viewId: "view-b-list",
        preferenceKey: TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY,
        preferenceValue: taskListColumnWidthsPreferenceValue({ status: 10 }),
        updatedAt: "2026-06-21T00:00:00.000Z",
      },
    ], "view-a-list")).toEqual({ name: 20, priority: 7 });
  });
});
