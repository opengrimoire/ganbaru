import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { applyProjectTaskBulk, reorderProjectItem, type ProjectTaskBulkRequest, type ProjectReorderRequest } from "./projects";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:test") }));

const request: ProjectTaskBulkRequest = {
  operationId: "operation-1", projectId: "project-1",
  tasks: [{ id: "task-1", value: "normal" }], change: { kind: "priority", priority: "high" },
};

function result() {
  return {
    groups: [], projects: [], sections: [], statuses: [], priorities: [], checklist_items: [],
    tags: [], task_tag_links: [], custom_fields: [], custom_field_options: [], custom_field_values: [],
    custom_field_option_values: [], dependencies: [], event_links: [], view_preferences: [], custom_emojis: [],
    removals: [], calendar_event_project_assignments: [], task_change_events: [],
    tasks: [{
      id: "task-1", project_id: "project-1", revision: 7, section_id: "section-1", status_id: "status-1",
      parent_task_id: null, title: "Current native title", description: "Description", priority: "high",
      task_type: "task", section_sort_order: 1000, status_sort_order: 1000, estimate_minutes: null,
      due_date: null, due_time: null, due_reminder_minutes: null, start_date: null, start_time: null,
      start_reminder_minutes: null, target_end_date: null,
      completed_at: null, archived_at: null, blocker_reason: null, milestone: 0,
      created_at: "2026-10-02T00:00:00Z", updated_at: "2026-10-02T00:01:00Z",
    }],
  };
}

describe("applyProjectTaskBulk", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends immutable intent once and maps validated canonical revisions", async () => {
    vi.mocked(invoke).mockResolvedValue(result());
    const mutation = await applyProjectTaskBulk(request);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("projects_apply_task_bulk", { dbUrl: "sqlite:test", request });
    expect(mutation.changed.tasks[0]).toMatchObject({ revision: 7, priority: "high", title: "Current native title" });
  });

  it.each([
    { revision: -1 }, { revision: Number.MAX_SAFE_INTEGER + 1 }, { project_id: "other-project" },
    { task_type: "unknown" }, { title: null }, { status_sort_order: Number.NaN },
  ])("rejects malformed native task fields before reconciliation: %j", async (patch) => {
    const response = result();
    vi.mocked(invoke).mockResolvedValue({ ...response, tasks: [{ ...response.tasks[0], ...patch }] });
    await expect(applyProjectTaskBulk(request)).rejects.toThrow("Invalid project bulk");
  });

  it("rejects duplicate rows and history for a task outside the operation", async () => {
    const response = result();
    vi.mocked(invoke).mockResolvedValue({ ...response, tasks: [...response.tasks, ...response.tasks] });
    await expect(applyProjectTaskBulk(request)).rejects.toThrow("Duplicate");
    vi.mocked(invoke).mockResolvedValue({ ...response, task_change_events: [{ event_type: "updated", task_id: "other" }] });
    await expect(applyProjectTaskBulk(request)).rejects.toThrow("history owner");
  });
});

describe("reorderProjectItem", () => {
  const reorder: ProjectReorderRequest = {
    operationId: "reorder-1", projectId: "project-1", direction: 1,
    item: { kind: "custom_field_option", id: "option-1", fieldId: "field-1", expectedOrder: 1000 },
  };
  const option = { id: "option-1", field_id: "field-1", revision: 2, name: "Current name",
    sort_order: 2000, created_at: "created", updated_at: "updated" };

  beforeEach(() => vi.clearAllMocks());

  it("validates native option ownership and revision before applying a reorder receipt", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...result(), tasks: [], custom_field_options: [option] });
    const mutation = await reorderProjectItem(reorder);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("projects_reorder_item", { dbUrl: "sqlite:test", request: reorder });
    expect(mutation.changed.customFieldOptions[0]).toMatchObject({ revision: 2, name: "Current name", sortOrder: 2000 });
  });

  it.each([{ field_id: "foreign" }, { revision: -1 }, { revision: undefined }, { sort_order: 1.5 }])(
    "rejects invalid native option data: %j", async (patch) => {
      vi.mocked(invoke).mockResolvedValue({ ...result(), tasks: [], custom_field_options: [{ ...option, ...patch }] });
      await expect(reorderProjectItem(reorder)).rejects.toThrow();
    },
  );

  it("rejects unrelated rows and duplicate option identities", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...result(), custom_field_options: [option] });
    await expect(reorderProjectItem(reorder)).rejects.toThrow();
    vi.mocked(invoke).mockResolvedValue({ ...result(), tasks: [], custom_field_options: [option, option] });
    await expect(reorderProjectItem(reorder)).rejects.toThrow("Duplicate");
  });
});
