import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { DbCalendarEvent } from "$lib/calendar/db-rows";
import {
  CalendarCommitFailure, commitCalendarEdit, parseCalendarCommitReceipt, parseCalendarEditPreview, previewCalendarEdit,
  type CalendarPreviewRequest,
} from "./calendar-edit";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:calendar-edit") }));

const request: CalendarPreviewRequest = {
  commandId: "save", edit: {
    selection: { templateId: "source", recurrenceDate: "2024-03-11", scope: "this" },
    draft: { fields: [{ field: "title", value: "Edited" }] },
  },
  window: { windowStartDate: "2024-03-09", windowEndDate: "2024-03-15", renderZone: "Asia/Tokyo", includeTotalEventCount: false },
};

function response() {
  const event: DbCalendarEvent = {
    id: "edited", title: "Edited", start_time: "2024-03-11T13:00:00Z", end_time: "2024-03-11T14:00:00Z",
    timezone: "America/New_York", calendar_id: "calendar", project_id: null, environment_id: null, playlist_id: null,
    color: null, rrule: null, notifications: null, exceptions: null, repeat_until: null,
    all_day: 0, location: "", has_call_link: 0, meeting_enabled: 0, transparency: "opaque", status: "confirmed",
    local_rsvp_status: null, created_at: "2024-03-01T09:00:00Z", rdate: null, rhythm_kind: null,
    rhythm_source: null, preset_key: null, count_focus_duration_minutes: null, count_short_break_minutes: null,
    count_long_break_minutes: null, count_long_break_after_focus_count: null, sequence_steps: null, idle_timeout_minutes: null,
  };
  return {
    vaultId: "vault", vaultGeneration: 2,
    preview: {
      commandId: "save", sourceId: "source", editedId: "edited", reviewRevision: "1".repeat(64), changed: true,
      scope: { effectiveScope: "this", selectedStarted: false, selectedHasHistory: false, selectedActive: false },
      previewedIds: ["edited"], editingId: "edited",
      window: {
        events: [event], overrides: [], attendees: [], diagnostics: [], total_event_count: null,
        occurrences: [{ template_id: "edited", id: "edited", recurring_parent_id: null, recurrence_date: "2024-03-11",
          start_time: "2024-03-11T13:00:00.000Z", end_time: "2024-03-11T14:00:00.000Z", override_id: null }],
      },
    },
  };
}

describe("native Calendar edit boundary", () => {
  it("validates removed and retained deletion families without requiring a removed source row", () => {
    const deleting: CalendarPreviewRequest = { ...request, edit: { kind: "delete",
      selection: { templateId: "source", recurrenceDate: "2024-03-11", scope: "all" }, stopActive: false } };
    const base = response();
    const removed = { ...base, preview: { ...base.preview, editedId: "source", previewedIds: [], editingId: null,
      outcome: "mixed", requiresActiveStop: true, historyOnly: false,
      window: { ...base.preview.window, events: [], occurrences: [] } } };
    expect(parseCalendarEditPreview(removed, deleting)).toMatchObject({ sourceId: "source", editingId: undefined,
      deletion: { outcome: "mixed", requiresActiveStop: true, historyOnly: false } });
    const retained = { ...removed, preview: { ...removed.preview, window: { ...base.preview.window,
      events: [{ ...base.preview.window.events[0], id: "source" }],
      occurrences: [{ ...base.preview.window.occurrences[0], template_id: "source", id: "source" }] } } };
    expect(parseCalendarEditPreview(retained, deleting).window.windowEvents).toHaveLength(1);
    for (const patch of [{ editedId: "other" }, { outcome: "unknown" }, { requiresActiveStop: "true" },
      { historyOnly: null }, { editingId: "source" }, { previewedIds: ["source"] }, { changed: false },
      { window: base.preview.window }]) {
      expect(() => parseCalendarEditPreview({ ...removed, preview: { ...removed.preview, ...patch } }, deleting)).toThrow();
    }
  });

  it("keeps native Undo availability separate from an accepted receipt's durable revision", () => {
    const accepted = { commandId: "delete", editedId: "source", changed: true, preservedIds: [], undoReviewRevision: "a".repeat(64) };
    expect(parseCalendarCommitReceipt(accepted, "delete")).toEqual(accepted);
    expect(parseCalendarCommitReceipt({ ...accepted, undoAvailableForMs: 2_000 }, "delete").undoAvailableForMs).toBe(2_000);
    for (const remaining of [0, -1, 5_001, 1.5, "2000", Number.POSITIVE_INFINITY]) {
      expect(() => parseCalendarCommitReceipt({ ...accepted, undoAvailableForMs: remaining }, "delete")).toThrow();
    }
    expect(() => parseCalendarCommitReceipt({ ...accepted, undoReviewRevision: "bad" }, "delete")).toThrow();
    expect(() => parseCalendarCommitReceipt({ ...accepted, undoReviewRevision: undefined, undoAvailableForMs: 1 }, "delete")).toThrow();
  });
  it("validates the complete task and event association for a scheduling review", () => {
    const id = `calendar-schedule-${"a".repeat(64)}`;
    const schedulingRequest: CalendarPreviewRequest = { ...request, edit: {
      kind: "schedule_tasks", projectId: "project", tasks: [{ id: "task", revision: 3 }],
      startTime: "2024-03-11T22:00", timezone: "Asia/Tokyo", durationMinutes: 60, globalIdleTimeoutMinutes: null,
    } };
    const value = response();
    value.preview.sourceId = value.preview.editedId = id;
    value.preview.window.events[0].id = id;
    value.preview.window.occurrences[0].id = value.preview.window.occurrences[0].template_id = id;
    value.preview.previewedIds = [id];
    value.preview.editingId = id;
    const envelope = { ...value, preview: { ...value.preview, scheduledTasks: [{ taskId: "task", eventId: id }] } };
    expect(parseCalendarEditPreview(envelope, schedulingRequest).scheduledTasks).toEqual([{ taskId: "task", eventId: id }]);
    for (const identities of [[], [{ taskId: "other", eventId: id }], [{ taskId: "task", eventId: "source" }],
      [{ taskId: "task", eventId: id }, { taskId: "task", eventId: id }]]) {
      expect(() => parseCalendarEditPreview({ ...envelope, preview: { ...envelope.preview, scheduledTasks: identities } }, schedulingRequest)).toThrow();
    }
    expect(() => parseCalendarEditPreview({ ...envelope, preview: { ...envelope.preview, previewedIds: [] } }, schedulingRequest)).toThrow();
    const accepted = { commandId: "save", editedId: id, changed: true, preservedIds: [], scheduledTasks: envelope.preview.scheduledTasks };
    expect(parseCalendarCommitReceipt(accepted, "save").scheduledTasks).toEqual(accepted.scheduledTasks);
    expect(() => parseCalendarCommitReceipt({ ...accepted, scheduledTasks: [{ taskId: "task", eventId: "raw" }] }, "save")).toThrow();
  });

  it("accepts only the native new source family for a creation review", () => {
    const createRequest: CalendarPreviewRequest = { ...request,
      edit: { kind: "create", draft: { timing: { startTime: "2024-03-11T22:00", endTime: "2024-03-11T23:00",
        timezone: "America/New_York", inputZone: "Asia/Tokyo" } } } };
    const value = response();
    const id = `calendar-create-${"a".repeat(64)}`;
    value.preview.sourceId = value.preview.editedId = id;
    value.preview.window.events[0].id = id;
    value.preview.window.occurrences[0].id = value.preview.window.occurrences[0].template_id = id;
    value.preview.previewedIds = [id];
    value.preview.editingId = id;
    expect(parseCalendarEditPreview(value, createRequest)).toMatchObject({ sourceId: id, editedId: id });
    expect(() => parseCalendarEditPreview({ ...value, preview: { ...value.preview, sourceId: "other" } }, createRequest)).toThrow();
    expect(() => parseCalendarEditPreview({ ...value, preview: { ...value.preview, previewedIds: [] } }, createRequest)).toThrow();
    expect(() => parseCalendarEditPreview({ ...value, preview: { ...value.preview,
      scope: { ...value.preview.scope, selectedHasHistory: true } } }, createRequest)).toThrow();
  });

  it("distinguishes confirmed native rejection from an unresolved native outcome", async () => {
    const commit = { vaultId: "vault", vaultGeneration: 2, commandId: "save", reviewRevision: "1".repeat(64), edit: request.edit };
    for (const outcome of ["rejected", "unknown"] as const) {
      vi.mocked(invoke).mockRejectedValueOnce({ outcome, message: "Native failure" });
      await expect(commitCalendarEdit(commit)).rejects.toEqual(new CalendarCommitFailure(outcome, "Native failure"));
    }
    vi.mocked(invoke).mockRejectedValueOnce({ outcome: "invalid", message: "Untrusted failure" });
    await expect(commitCalendarEdit(commit)).rejects.toEqual({ outcome: "invalid", message: "Untrusted failure" });
  });

  beforeEach(() => vi.clearAllMocks());

  it("requests semantic intent and maps native identities into display labels", async () => {
    vi.mocked(invoke).mockResolvedValue(response());
    const preview = await previewCalendarEdit(request);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("calendar_preview_edit", { dbUrl: "sqlite:calendar-edit", request });
    expect(preview.window.windowEvents[0]).toMatchObject({ id: "edited", start: "2024-03-11 22:00", recurrenceDate: "2024-03-11" });
    expect(preview.previewedIds).toEqual(new Set(["edited"]));
  });

  it.each([
    (value: ReturnType<typeof response>) => ({ ...value, vaultGeneration: Number.MAX_SAFE_INTEGER + 1 }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, commandId: "previous" } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, sourceId: "other" } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, reviewRevision: "invalid" } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, previewedIds: ["missing"] } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, previewedIds: ["edited", "edited"] } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, editingId: "missing" } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, editedId: "missing" } }),
    (value: ReturnType<typeof response>) => ({ ...value, preview: { ...value.preview, scope: { ...value.preview.scope, selectedActive: "false" } } }),
  ])("rejects mismatched reviews and invisible identities", (corrupt) => {
    expect(() => parseCalendarEditPreview(corrupt(response()), request)).toThrow();
  });

  it("retries the identical review and operation after a lost response", async () => {
    const accepted = { commandId: "save", editedId: "edited", changed: true, preservedIds: [["2024-03-10", "preserved"]] };
    const commit = { vaultId: "vault", vaultGeneration: 2, commandId: "save", reviewRevision: "1".repeat(64), edit: request.edit };
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Lost response")).mockResolvedValueOnce(accepted);
    await expect(commitCalendarEdit(commit)).rejects.toThrow("Lost response");
    await expect(commitCalendarEdit(commit)).resolves.toEqual(accepted);
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["calendar_commit_edit", { request: commit }], ["calendar_commit_edit", { request: commit }],
    ]);
    expect(() => parseCalendarCommitReceipt({ ...accepted, commandId: "other" }, "save")).toThrow();
    expect(() => parseCalendarCommitReceipt({ ...accepted, preservedIds: [...accepted.preservedIds, ...accepted.preservedIds] }, "save")).toThrow();
    expect(() => parseCalendarCommitReceipt({ ...accepted, preservedIds: [["2024-02-30", "preserved"]] }, "save")).toThrow();
  });
});
