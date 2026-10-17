import { invoke } from "@tauri-apps/api/core";
import { ensureDbUrl } from "$lib/api/db";
import type { RecurringScope } from "$lib/calendar/types";
import type { CalendarEventUpdatePayload, CalendarUpdateField } from "$lib/stores/calendar/event-payloads";
import {
  mapNativeCalendarWindow,
  type MappedNativeCalendarWindow,
  type NativeCalendarWindowRequest,
} from "$lib/stores/calendar/native-window";

type DerivedField = "startTime" | "endTime" | "timezone" | "allDay" | "rrule"
  | "repeatUntil" | "exceptions" | "rdate" | "sourceUid" | "sequence";

/** User intent only. Rust derives partitions, exceptions, protection and execution transfers. */
export interface CalendarEditIntent {
  action?: "save" | "end_now" | "enable_focus";
  selection: { templateId: string; recurrenceDate: string; scope: RecurringScope };
  draft: {
    timing?: { startTime?: string; endTime?: string; timezone?: string; inputZone?: string; allDay?: boolean };
    recurrence?: { kind: "unchanged" } | { kind: "clear" } | { kind: "set"; value: string };
    fields?: Exclude<CalendarUpdateField, { field: DerivedField }>[];
    attendees?: CalendarEventUpdatePayload["attendees"];
    alarms?: CalendarEventUpdatePayload["alarms"];
    pomodoroConfig?: CalendarEventUpdatePayload["pomodoroConfig"];
  };
}

/** New source intent contains no persisted IDs, timestamps or execution evidence. */
export interface CalendarCreateIntent {
  kind: "create";
  draft: CalendarEditIntent["draft"];
}

/** Canonical Project defaults and task rows are loaded by Rust in the owner transaction. */
export interface CalendarTaskScheduleIntent {
  kind: "schedule_tasks";
  projectId: string;
  tasks: { id: string; revision: number }[];
  startTime: string;
  timezone: string;
  durationMinutes: number;
  globalIdleTimeoutMinutes: number | null;
}

export interface ScheduledTaskIdentity { taskId: string; eventId: string }

/** Rust derives deletion scope, protection, archives and exact Focus stop identity. */
export interface CalendarDeleteIntent {
  kind: "delete";
  selection: CalendarEditIntent["selection"];
  stopActive: boolean;
}

/** Undo names the accepted deletion; its complete preimage remains in Rust. */
export interface CalendarDeleteUndoIntent {
  kind: "undo_delete";
  deleteCommandId: string;
}

export type CalendarDeleteOutcome = "delete" | "archive" | "mixed";
export type CalendarMutationIntent = CalendarEditIntent | CalendarCreateIntent | CalendarTaskScheduleIntent
  | CalendarDeleteIntent | CalendarDeleteUndoIntent;

export interface CalendarPreviewRequest {
  commandId: string;
  edit: CalendarMutationIntent;
  window: NativeCalendarWindowRequest;
}

export interface CalendarEditPreview {
  vaultId: string;
  vaultGeneration: number;
  commandId: string;
  sourceId: string;
  editedId: string;
  reviewRevision: string;
  changed: boolean;
  scope: {
    effectiveScope: RecurringScope;
    selectedStarted: boolean;
    selectedHasHistory: boolean;
    selectedActive: boolean;
  };
  window: MappedNativeCalendarWindow;
  previewedIds: ReadonlySet<string>;
  editingId: string | undefined;
  scheduledTasks?: ScheduledTaskIdentity[];
  deletion?: { outcome: CalendarDeleteOutcome; requiresActiveStop: boolean; historyOnly: boolean };
}

export interface CalendarCommitRequest {
  vaultId: string;
  vaultGeneration: number;
  commandId: string;
  reviewRevision: string;
  edit: CalendarMutationIntent;
}

export interface CalendarCommitReceipt {
  commandId: string;
  editedId: string;
  changed: boolean;
  preservedIds: [string, string][];
  scheduledTasks?: ScheduledTaskIdentity[];
  undoReviewRevision?: string;
  undoAvailableForMs?: number;
}

/** Native rejection proves this request did not commit; transport failures do not. */
export class CalendarCommitFailure extends Error {
  constructor(readonly outcome: "rejected" | "unknown", message: string) {
    super(message);
    this.name = "CalendarCommitFailure";
  }
}

function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Invalid native Calendar edit response");
  }
  return value as Record<string, unknown>;
}

function text(value: unknown, maximum = 1_036): string {
  if (typeof value !== "string" || value.length > maximum || !value.trim()
    || /[\u0000-\u001f\u007f]/u.test(value)) throw new Error("Invalid native Calendar edit identity");
  return value;
}

function flag(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Error("Invalid native Calendar edit flag");
  return value;
}

function scheduledIdentities(value: unknown): ScheduledTaskIdentity[] {
  if (!Array.isArray(value) || value.length === 0 || value.length > 1_000) throw new Error("Invalid scheduled task identities");
  const result = value.map((item: unknown) => {
    const row = object(item);
    const eventId = text(row.eventId);
    if (!/^calendar-schedule-[0-9a-f]{64}$/u.test(eventId)) throw new Error("Invalid scheduled event identity");
    return { taskId: text(row.taskId), eventId };
  });
  if (new Set(result.map((row) => row.taskId)).size !== result.length
    || new Set(result.map((row) => row.eventId)).size !== result.length) throw new Error("Duplicate scheduled task identity");
  return result;
}

/** Validate consumed fields and visible ownership before retaining a native review. */
export function parseCalendarEditPreview(value: unknown, request: CalendarPreviewRequest): CalendarEditPreview {
  const envelope = object(value);
  const row = object(envelope.preview);
  const creating = "kind" in request.edit && request.edit.kind === "create";
  const scheduling = "kind" in request.edit && request.edit.kind === "schedule_tasks";
  const deleting = "kind" in request.edit && request.edit.kind === "delete";
  if ("kind" in request.edit && request.edit.kind === "undo_delete") throw new Error("Calendar Undo has no authored preview");
  const createsSources = creating || scheduling;
  if (row.commandId !== request.commandId || (!createsSources && "selection" in request.edit
    && row.sourceId !== request.edit.selection.templateId)) {
    throw new Error("Calendar preview belongs to another edit");
  }
  const vaultGeneration = envelope.vaultGeneration;
  if (typeof vaultGeneration !== "number" || !Number.isSafeInteger(vaultGeneration) || vaultGeneration <= 0) {
    throw new Error("Invalid Calendar preview generation");
  }
  const reviewRevision = text(row.reviewRevision, 64);
  if (!/^[0-9a-f]{64}$/u.test(reviewRevision)) throw new Error("Invalid Calendar review revision");
  const window = mapNativeCalendarWindow(row.window, request.window.renderZone);
  if (window.diagnostics.length > 0) throw new Error("Calendar review contains an expansion failure");
  const editedId = text(row.editedId);
  const sourceId = text(row.sourceId);
  if (creating && (sourceId !== editedId || !/^calendar-create-[0-9a-f]{64}$/u.test(sourceId))) {
    throw new Error("Invalid native Calendar creation identity");
  }
  if (!deleting && !window.sourceEvents.some((event) => event.id === editedId)) throw new Error("Calendar preview has no edited source");
  if (deleting && (sourceId !== editedId || window.sourceEvents.length > 1
    || window.sourceEvents.some((event) => event.id !== sourceId)
    || window.windowEvents.some((event) => (event.recurringParentId ?? event.id) !== sourceId))) {
    throw new Error("Calendar deletion preview contains another source");
  }
  if (creating && (window.sourceEvents.length !== 1
    || window.windowEvents.some((event) => (event.recurringParentId ?? event.id) !== editedId))) {
    throw new Error("Calendar creation preview contains another source");
  }
  const scheduledTasks = scheduling ? scheduledIdentities(row.scheduledTasks) : undefined;
  if (scheduledTasks && "tasks" in request.edit) {
    const sourceIds = new Set(scheduledTasks.map((task) => task.eventId));
    if (scheduledTasks.length !== request.edit.tasks.length || sourceId !== editedId
      || sourceId !== scheduledTasks[0]?.eventId
      || scheduledTasks.some((task, index) => task.taskId !== ("tasks" in request.edit ? request.edit.tasks[index]?.id : undefined))
      || window.sourceEvents.length !== sourceIds.size || new Set(window.sourceEvents.map((event) => event.id)).size !== sourceIds.size
      || window.sourceEvents.some((event) => !sourceIds.has(event.id))
      || window.windowEvents.some((event) => !sourceIds.has(event.id) || event.recurringParentId !== undefined)) {
      throw new Error("Calendar scheduling preview contains a different task selection");
    }
  }
  const visibleIds = new Set(window.windowEvents.map((event) => event.id));
  if (!Array.isArray(row.previewedIds) || row.previewedIds.length > 10_000) throw new Error("Invalid Calendar preview contours");
  const previewedIds = new Set(row.previewedIds.map((value: unknown) => text(value)));
  if (previewedIds.size !== row.previewedIds.length || [...previewedIds].some((id) => !visibleIds.has(id))) {
    throw new Error("Calendar preview contour has no visible occurrence");
  }
  const editingId = row.editingId === null ? undefined : text(row.editingId);
  if (editingId !== undefined && !visibleIds.has(editingId)) throw new Error("Calendar editing identity is not visible");
  const scope = object(row.scope);
  const effectiveScope = scope.effectiveScope;
  if (effectiveScope !== "this" && effectiveScope !== "following" && effectiveScope !== "all") {
    throw new Error("Invalid native Calendar scope");
  }
  const selectedStarted = flag(scope.selectedStarted);
  const selectedHasHistory = flag(scope.selectedHasHistory);
  const selectedActive = flag(scope.selectedActive);
  if (createsSources && (effectiveScope !== "this" || selectedStarted || selectedHasHistory || selectedActive
    || !flag(row.changed) || previewedIds.size !== visibleIds.size)) {
    throw new Error("Invalid native Calendar creation projection");
  }
  let deletion: CalendarEditPreview["deletion"];
  if (deleting) {
    const outcome = row.outcome;
    if (outcome !== "delete" && outcome !== "archive" && outcome !== "mixed") throw new Error("Invalid Calendar deletion outcome");
    if (!flag(row.changed) || editingId !== undefined || previewedIds.size !== 0) throw new Error("Invalid Calendar deletion projection");
    deletion = { outcome, requiresActiveStop: flag(row.requiresActiveStop), historyOnly: flag(row.historyOnly) };
  }
  return {
    vaultId: text(envelope.vaultId, 1_024), vaultGeneration, commandId: request.commandId,
    sourceId, editedId, reviewRevision, changed: flag(row.changed),
    scope: { effectiveScope, selectedStarted, selectedHasHistory, selectedActive },
    window, previewedIds, editingId,
    ...(scheduledTasks ? { scheduledTasks } : {}),
    ...(deletion ? { deletion } : {}),
  };
}

/** Preserve the exact receipt identity after an uncertain transport result. */
export function parseCalendarCommitReceipt(value: unknown, commandId: string): CalendarCommitReceipt {
  const row = object(value);
  if (row.commandId !== commandId || !Array.isArray(row.preservedIds) || row.preservedIds.length > 10_000) {
    throw new Error("Invalid native Calendar edit receipt");
  }
  const preservedIds = row.preservedIds.map((value: unknown): [string, string] => {
    if (!Array.isArray(value) || value.length !== 2) throw new Error("Invalid Calendar preserved identity");
    const date = text(value[0], 10);
    const parsed = new Date(`${date}T00:00:00Z`);
    if (!/^\d{4}-\d{2}-\d{2}$/u.test(date) || date.startsWith("0000")
      || !Number.isFinite(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== date) {
      throw new Error("Invalid Calendar preserved date");
    }
    return [date, text(value[1])];
  });
  if (new Set(preservedIds.map(([date]) => date)).size !== preservedIds.length
    || new Set(preservedIds.map(([, id]) => id)).size !== preservedIds.length) {
    throw new Error("Duplicate Calendar preserved identity");
  }
  const undoReviewRevision = row.undoReviewRevision === undefined ? undefined : text(row.undoReviewRevision, 64);
  if (undoReviewRevision !== undefined && !/^[0-9a-f]{64}$/u.test(undoReviewRevision)) throw new Error("Invalid Calendar Undo review");
  const undoAvailableForMs = row.undoAvailableForMs;
  if (undoAvailableForMs !== undefined && (undoReviewRevision === undefined || typeof undoAvailableForMs !== "number"
    || !Number.isSafeInteger(undoAvailableForMs) || undoAvailableForMs <= 0 || undoAvailableForMs > 5_000)) {
    throw new Error("Invalid native Calendar Undo availability");
  }
  return { commandId, editedId: text(row.editedId), changed: flag(row.changed), preservedIds,
    ...(row.scheduledTasks === undefined ? {} : { scheduledTasks: scheduledIdentities(row.scheduledTasks) }),
    ...(undoReviewRevision === undefined ? {} : { undoReviewRevision }),
    ...(undoAvailableForMs === undefined ? {} : { undoAvailableForMs: undoAvailableForMs as number }) };
}

/** Request a bounded visible projection without sending source rows or browser clocks. */
export async function previewCalendarEdit(request: CalendarPreviewRequest): Promise<CalendarEditPreview> {
  const response = await invoke<unknown>("calendar_preview_edit", { dbUrl: await ensureDbUrl(), request });
  return parseCalendarEditPreview(response, request);
}

/** Commit the reviewed semantic intent in the native Calendar and Focus transaction. */
export async function commitCalendarEdit(request: CalendarCommitRequest): Promise<CalendarCommitReceipt> {
  let response: unknown;
  try {
    response = await invoke<unknown>("calendar_commit_edit", { request });
  } catch (error) {
    if (typeof error === "object" && error !== null && "outcome" in error && "message" in error
      && (error.outcome === "rejected" || error.outcome === "unknown") && typeof error.message === "string") {
      throw new CalendarCommitFailure(error.outcome, error.message);
    }
    throw error;
  }
  return parseCalendarCommitReceipt(response, request.commandId);
}

/** Release only this toast's native preimage without touching durable history. */
export async function dismissCalendarDeleteUndo(request: {
  vaultId: string; vaultGeneration: number; deleteCommandId: string;
}): Promise<void> {
  await invoke("calendar_dismiss_delete_undo", { request });
}
