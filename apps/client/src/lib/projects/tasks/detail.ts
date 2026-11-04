import { Temporal } from "@js-temporal/polyfill";
import type { CalendarEvent } from "$lib/calendar/types";
import { canonicalProjectCustomFieldNumber, projectCustomFieldUsesTextValue } from "$lib/projects/custom-fields";
import { compareTaskSectionOrder } from "$lib/projects/order";
import type {
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldValue,
  ProjectCustomFieldValueUpdate,
  ProjectLinkableEvent,
  ProjectTag,
  ProjectTask,
  ProjectTaskDependency,
} from "$lib/projects/types";
import type { ProjectTaskUpdatePatch } from "$lib/projects/snapshot/update-payloads";
import { offeredDateReminderMinutes } from "$lib/utils/date-reminders";

export interface ProjectTaskDetailDraft {
  title: string;
  description: string;
  sectionId: string;
  statusId: string;
  priority: ProjectTask["priority"];
  taskType: ProjectTask["taskType"];
  estimateMinutes: string;
  startDate: string;
  /** The `HH:MM` start time, or empty for a whole day. */
  startTime: string;
  /** Minutes before the start date its reminder fires, or null for no reminder. */
  startReminderMinutes: number | null;
  dueDate: string;
  /** The `HH:MM` due time, or empty for a whole day. */
  dueTime: string;
  /** Minutes before the due date its reminder fires, or null for no reminder. */
  dueReminderMinutes: number | null;
  targetEndDate: string;
  blockerReason: string;
  changeReason: string;
  milestone: boolean;
}

export interface ProjectTaskDetailCustomFieldDrafts {
  textDrafts: Record<string, string>;
  numberDrafts: Record<string, string>;
  dateDrafts: Record<string, string>;
  checkboxDrafts: Record<string, boolean>;
  selectDrafts: Record<string, string>;
  multiDrafts: Record<string, string[]>;
}

export type ProjectTaskDetailCustomFieldSaveDraft =
  Omit<ProjectCustomFieldValueUpdate, "taskId" | "fieldId">;

export type ProjectTaskDetailCustomFieldRawDraft =
  | { kind: "text" | "number" | "date" | "select"; value: string }
  | { kind: "checkbox"; value: boolean }
  | { kind: "multi"; value: string[] };

export type ProjectTaskDetailCustomFieldSaveDraftResult =
  | { ok: true; value: ProjectTaskDetailCustomFieldSaveDraft }
  | { ok: false; reason: "invalid-number" | "invalid-date" };

export type ProjectTaskDetailDraftPatchResult =
  | { ok: true; patch: ProjectTaskUpdatePatch }
  | { ok: false; reason: "title-required" | "missing-section-or-status" | "invalid-estimate" | "invalid-date" };

export type ProjectTaskDetailEventLinkDateRangeResult =
  | { ok: true; startDate: string | undefined; endDate: string | undefined }
  | { ok: false; reason: "invalid-date" | "invalid-range" };

function normalizeOptionalDetailDate(value: string): string | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  return Temporal.PlainDate.from(trimmed).toString();
}

function normalizeOptionalDetailPositiveInteger(value: string): number | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const parsed = Number(trimmed);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error("invalid-estimate");
  }
  return parsed;
}

/**
 * A detail date's reminder as it would be saved: none without the date, and otherwise an offset the date's time
 * still offers.
 *
 * @param date The draft `YYYY-MM-DD` date, or empty.
 * @param time The draft `HH:MM` time, or empty for a whole day.
 * @param minutes The chosen minutes before the date, or null for no reminder.
 */
export function projectTaskDetailDateReminder(date: string, time: string, minutes: number | null): number | null {
  if (!date.trim() || minutes === null) return null;
  return offeredDateReminderMinutes(minutes, Boolean(time));
}

export function projectTaskDetailDraftFromTask(task: ProjectTask): ProjectTaskDetailDraft {
  return {
    title: task.title,
    description: task.description,
    sectionId: task.sectionId,
    statusId: task.statusId,
    priority: task.priority,
    taskType: task.taskType,
    estimateMinutes: String(task.estimateMinutes ?? ""),
    startDate: task.startDate ?? "",
    startTime: task.startTime ?? "",
    startReminderMinutes: task.startReminderMinutes ?? null,
    dueDate: task.dueDate ?? "",
    dueTime: task.dueTime ?? "",
    dueReminderMinutes: task.dueReminderMinutes ?? null,
    targetEndDate: task.targetEndDate ?? "",
    blockerReason: task.blockerReason ?? "",
    changeReason: "",
    milestone: task.milestone,
  };
}

/** Builds the empty draft shown before a new task exists. */
export function projectTaskNewDetailDraft(placement: {
  sectionId: string;
  statusId: string;
}): ProjectTaskDetailDraft {
  return {
    title: "",
    description: "",
    sectionId: placement.sectionId,
    statusId: placement.statusId,
    priority: "normal",
    taskType: "task",
    estimateMinutes: "",
    startDate: "",
    startTime: "",
    startReminderMinutes: null,
    dueDate: "",
    dueTime: "",
    dueReminderMinutes: null,
    targetEndDate: "",
    blockerReason: "",
    changeReason: "",
    milestone: false,
  };
}

/** Reports whether two detail drafts hold the same field values. */
export function projectTaskDetailDraftsEqual(
  left: ProjectTaskDetailDraft,
  right: ProjectTaskDetailDraft,
): boolean {
  return (Object.keys(left) as (keyof ProjectTaskDetailDraft)[])
    .every((key) => left[key] === right[key]);
}

/**
 * Reports whether a new-task draft sets fields beyond the title and placement
 * that task creation accepts, so creation must follow up with an update.
 */
export function projectTaskNewDetailDraftHasExtras(
  initial: ProjectTaskDetailDraft,
  draft: ProjectTaskDetailDraft,
): boolean {
  return !projectTaskDetailDraftsEqual(
    { ...draft, title: initial.title, sectionId: initial.sectionId, statusId: initial.statusId },
    initial,
  );
}

export function projectTaskDetailDraftDirty(task: ProjectTask, draft: ProjectTaskDetailDraft): boolean {
  return draft.title !== task.title
    || draft.description !== task.description
    || draft.sectionId !== task.sectionId
    || draft.statusId !== task.statusId
    || draft.priority !== task.priority
    || draft.taskType !== task.taskType
    || draft.estimateMinutes !== String(task.estimateMinutes ?? "")
    || draft.startDate !== (task.startDate ?? "")
    || (draft.startDate ? draft.startTime : "") !== (task.startTime ?? "")
    || projectTaskDetailDateReminder(draft.startDate, draft.startTime, draft.startReminderMinutes)
      !== (task.startReminderMinutes ?? null)
    || draft.dueDate !== (task.dueDate ?? "")
    || (draft.dueDate ? draft.dueTime : "") !== (task.dueTime ?? "")
    || projectTaskDetailDateReminder(draft.dueDate, draft.dueTime, draft.dueReminderMinutes)
      !== (task.dueReminderMinutes ?? null)
    || draft.targetEndDate !== (task.targetEndDate ?? "")
    || draft.blockerReason !== (task.blockerReason ?? "")
    || draft.milestone !== task.milestone;
}

export function projectTaskDetailDraftPatch(draft: ProjectTaskDetailDraft): ProjectTaskDetailDraftPatchResult {
  const title = draft.title.trim();
  if (!title) return { ok: false, reason: "title-required" };
  if (!draft.sectionId || !draft.statusId) return { ok: false, reason: "missing-section-or-status" };
  let estimateMinutes: number | undefined;
  let startDate: string | undefined;
  let dueDate: string | undefined;
  let targetEndDate: string | undefined;
  try {
    estimateMinutes = normalizeOptionalDetailPositiveInteger(draft.estimateMinutes);
  } catch {
    return { ok: false, reason: "invalid-estimate" };
  }
  try {
    startDate = normalizeOptionalDetailDate(draft.startDate);
    dueDate = normalizeOptionalDetailDate(draft.dueDate);
    targetEndDate = normalizeOptionalDetailDate(draft.targetEndDate);
  } catch {
    return { ok: false, reason: "invalid-date" };
  }
  return {
    ok: true,
    patch: {
      title,
      description: draft.description.trim(),
      sectionId: draft.sectionId,
      statusId: draft.statusId,
      priority: draft.priority,
      taskType: draft.taskType,
      estimateMinutes,
      startDate,
      startTime: startDate ? draft.startTime || undefined : undefined,
      startReminderMinutes: projectTaskDetailDateReminder(startDate ?? "", draft.startTime, draft.startReminderMinutes)
        ?? undefined,
      dueDate,
      dueTime: dueDate ? draft.dueTime || undefined : undefined,
      dueReminderMinutes: projectTaskDetailDateReminder(dueDate ?? "", draft.dueTime, draft.dueReminderMinutes)
        ?? undefined,
      targetEndDate,
      blockerReason: draft.blockerReason.trim() || undefined,
      milestone: draft.milestone,
      changeReason: draft.changeReason.trim() || null,
    },
  };
}

export function projectTaskDetailEventLinkDateRange({
  startDateDraft,
  endDateDraft,
}: {
  startDateDraft: string;
  endDateDraft: string;
}): ProjectTaskDetailEventLinkDateRangeResult {
  let startDate: string | undefined;
  let endDate: string | undefined;
  try {
    startDate = normalizeOptionalDetailDate(startDateDraft);
    endDate = normalizeOptionalDetailDate(endDateDraft);
  } catch {
    return { ok: false, reason: "invalid-date" };
  }
  if (startDate && endDate && startDate > endDate) {
    return { ok: false, reason: "invalid-range" };
  }
  return { ok: true, startDate, endDate };
}

export function projectTaskDetailCustomFieldDrafts(input: {
  fields: readonly ProjectCustomField[];
  valueForField: (field: ProjectCustomField) => ProjectCustomFieldValue | undefined;
  optionValuesForField: (field: ProjectCustomField) => readonly ProjectCustomFieldOption[];
}): ProjectTaskDetailCustomFieldDrafts {
  const textDrafts: Record<string, string> = {};
  const numberDrafts: Record<string, string> = {};
  const dateDrafts: Record<string, string> = {};
  const checkboxDrafts: Record<string, boolean> = {};
  const selectDrafts: Record<string, string> = {};
  const multiDrafts: Record<string, string[]> = {};
  for (const field of input.fields) {
    const value = input.valueForField(field);
    const optionValues = input.optionValuesForField(field);
    textDrafts[field.id] = value?.textValue ?? "";
    numberDrafts[field.id] = value?.numberValue ?? "";
    dateDrafts[field.id] = value?.dateValue ?? "";
    checkboxDrafts[field.id] = value?.checkboxValue ?? false;
    selectDrafts[field.id] = optionValues[0]?.id ?? "none";
    multiDrafts[field.id] = optionValues.map((option) => option.id);
  }
  return {
    textDrafts,
    numberDrafts,
    dateDrafts,
    checkboxDrafts,
    selectDrafts,
    multiDrafts,
  };
}

export function projectTaskDetailCustomFieldDirty(input: {
  field: ProjectCustomField;
  drafts: ProjectTaskDetailCustomFieldDrafts;
  value: ProjectCustomFieldValue | undefined;
  optionValues: readonly ProjectCustomFieldOption[];
}): boolean {
  if (projectCustomFieldUsesTextValue(input.field.fieldType)) {
    return (input.drafts.textDrafts[input.field.id] ?? "") !== (input.value?.textValue ?? "");
  }
  if (input.field.fieldType === "number") {
    return (input.drafts.numberDrafts[input.field.id] ?? "") !== (input.value?.numberValue ?? "");
  }
  if (input.field.fieldType === "date") {
    return (input.drafts.dateDrafts[input.field.id] ?? "") !== (input.value?.dateValue ?? "");
  }
  if (input.field.fieldType === "checkbox") {
    return (input.drafts.checkboxDrafts[input.field.id] ?? false) !== (input.value?.checkboxValue ?? false);
  }
  if (input.field.fieldType === "select" || input.field.fieldType === "status") {
    return (input.drafts.selectDrafts[input.field.id] ?? "none") !== (input.optionValues[0]?.id ?? "none");
  }
  const currentIds = input.optionValues.map((option) => option.id).sort();
  const draftIds = [...(input.drafts.multiDrafts[input.field.id] ?? [])].sort();
  return currentIds.join("\u0000") !== draftIds.join("\u0000");
}

export function projectTaskDetailCustomFieldSaveDraft(input: {
  field: ProjectCustomField;
  drafts: ProjectTaskDetailCustomFieldDrafts;
}): ProjectTaskDetailCustomFieldSaveDraftResult {
  let textValue: string | null = null;
  let numberValue: string | null = null;
  let dateValue: string | null = null;
  let checkboxValue: boolean | null = null;
  let optionIds: string[] = [];
  if (projectCustomFieldUsesTextValue(input.field.fieldType)) {
    const text = (input.drafts.textDrafts[input.field.id] ?? "").trim();
    textValue = text || null;
  } else if (input.field.fieldType === "number") {
    const numberText = (input.drafts.numberDrafts[input.field.id] ?? "").trim();
    if (numberText) {
      const canonical = canonicalProjectCustomFieldNumber(numberText);
      if (canonical === null) {
        return { ok: false, reason: "invalid-number" };
      }
      numberValue = canonical;
    }
  } else if (input.field.fieldType === "date") {
    const dateText = (input.drafts.dateDrafts[input.field.id] ?? "").trim();
    if (dateText) {
      try {
        dateValue = Temporal.PlainDate.from(dateText).toString();
      } catch {
        return { ok: false, reason: "invalid-date" };
      }
    }
  } else if (input.field.fieldType === "checkbox") {
    checkboxValue = input.drafts.checkboxDrafts[input.field.id] ?? false;
  } else if (input.field.fieldType === "select" || input.field.fieldType === "status") {
    const optionId = input.drafts.selectDrafts[input.field.id] ?? "none";
    optionIds = optionId === "none" ? [] : [optionId];
  } else {
    optionIds = input.drafts.multiDrafts[input.field.id] ?? [];
  }
  return {
    ok: true,
    value: {
      textValue,
      numberValue,
      dateValue,
      checkboxValue,
      optionIds,
    },
  };
}

/** Captures the exact editable value submitted for one custom field. */
export function projectTaskDetailCustomFieldRawDraft(input: {
  field: ProjectCustomField;
  drafts: ProjectTaskDetailCustomFieldDrafts;
}): ProjectTaskDetailCustomFieldRawDraft {
  const fieldId = input.field.id;
  if (projectCustomFieldUsesTextValue(input.field.fieldType)) {
    return { kind: "text", value: input.drafts.textDrafts[fieldId] ?? "" };
  }
  if (input.field.fieldType === "number") {
    return { kind: "number", value: input.drafts.numberDrafts[fieldId] ?? "" };
  }
  if (input.field.fieldType === "date") {
    return { kind: "date", value: input.drafts.dateDrafts[fieldId] ?? "" };
  }
  if (input.field.fieldType === "checkbox") {
    return { kind: "checkbox", value: input.drafts.checkboxDrafts[fieldId] ?? false };
  }
  if (input.field.fieldType === "select" || input.field.fieldType === "status") {
    return { kind: "select", value: input.drafts.selectDrafts[fieldId] ?? "none" };
  }
  return { kind: "multi", value: [...(input.drafts.multiDrafts[fieldId] ?? [])] };
}

function projectTaskDetailCustomFieldRawDraftEqual(
  left: ProjectTaskDetailCustomFieldRawDraft,
  right: ProjectTaskDetailCustomFieldRawDraft,
): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "multi" && right.kind === "multi") {
    return left.value.length === right.value.length
      && left.value.every((value, index) => value === right.value[index]);
  }
  return left.value === right.value;
}

/**
 * Applies server-normalized field data only to the latest unchanged submission.
 * Newer edits and newer overlapping saves keep ownership of the field draft.
 */
export function projectTaskDetailMergeSavedCustomFieldDraft(input: {
  field: ProjectCustomField;
  drafts: ProjectTaskDetailCustomFieldDrafts;
  saved: ProjectTaskDetailCustomFieldSaveDraft;
  submittedRawDraft: ProjectTaskDetailCustomFieldRawDraft;
  requestGeneration: number;
  latestRequestGeneration: number;
}): ProjectTaskDetailCustomFieldDrafts {
  if (input.requestGeneration !== input.latestRequestGeneration) return input.drafts;
  const currentRawDraft = projectTaskDetailCustomFieldRawDraft({
    field: input.field,
    drafts: input.drafts,
  });
  if (!projectTaskDetailCustomFieldRawDraftEqual(currentRawDraft, input.submittedRawDraft)) {
    return input.drafts;
  }
  const fieldId = input.field.id;
  if (projectCustomFieldUsesTextValue(input.field.fieldType)) {
    return {
      ...input.drafts,
      textDrafts: {
        ...input.drafts.textDrafts,
        [fieldId]: input.saved.textValue ?? "",
      },
    };
  }
  if (input.field.fieldType === "number") {
    return {
      ...input.drafts,
      numberDrafts: {
        ...input.drafts.numberDrafts,
        [fieldId]: input.saved.numberValue ?? "",
      },
    };
  }
  if (input.field.fieldType === "date") {
    return {
      ...input.drafts,
      dateDrafts: {
        ...input.drafts.dateDrafts,
        [fieldId]: input.saved.dateValue ?? "",
      },
    };
  }
  if (input.field.fieldType === "checkbox") {
    return {
      ...input.drafts,
      checkboxDrafts: {
        ...input.drafts.checkboxDrafts,
        [fieldId]: input.saved.checkboxValue ?? false,
      },
    };
  }
  if (input.field.fieldType === "select" || input.field.fieldType === "status") {
    return {
      ...input.drafts,
      selectDrafts: {
        ...input.drafts.selectDrafts,
        [fieldId]: input.saved.optionIds[0] ?? "none",
      },
    };
  }
  return {
    ...input.drafts,
    multiDrafts: {
      ...input.drafts.multiDrafts,
      [fieldId]: [...input.saved.optionIds],
    },
  };
}

export function dependencyCandidateTasks({
  task,
  allProjectTasks,
  blockedByDependencies,
  dependencySearch,
}: {
  task: ProjectTask;
  allProjectTasks: readonly ProjectTask[];
  blockedByDependencies: readonly ProjectTaskDependency[];
  dependencySearch: string;
}): ProjectTask[] {
  const existingBlockingTaskIds = new Set(
    blockedByDependencies.map((dependency) => dependency.blockingTaskId),
  );
  const query = dependencySearch.trim().toLowerCase();
  return allProjectTasks
    .filter((candidate) =>
      candidate.id !== task.id
      && !existingBlockingTaskIds.has(candidate.id)
      && (!query
        || candidate.title.toLowerCase().includes(query)
        || candidate.description.toLowerCase().includes(query))
    )
    .slice(0, 8);
}

export function parentTaskCandidateTasks({
  task,
  activeProjectTasks,
  parentTaskSearch,
}: {
  task: ProjectTask;
  activeProjectTasks: readonly ProjectTask[];
  parentTaskSearch: string;
}): ProjectTask[] {
  const query = parentTaskSearch.trim().toLowerCase();
  return activeProjectTasks
    .filter((candidate) =>
      candidate.projectId === task.projectId
      && !candidate.parentTaskId
      && candidate.id !== task.id
      && (!query || candidate.title.toLowerCase().includes(query))
    )
    .sort(compareTaskSectionOrder)
    .slice(0, 8);
}

export function eventLinkCandidateEvents({
  taskEventIds,
  eventLinkResults,
}: {
  taskEventIds: readonly string[];
  eventLinkResults: readonly ProjectLinkableEvent[];
}): ProjectLinkableEvent[] {
  const linkedEventIds = new Set(taskEventIds);
  return eventLinkResults
    .filter((event) => !linkedEventIds.has(event.id))
    .slice(0, 8);
}

export function tagCandidateTags({
  tags,
  tagDraft,
}: {
  tags: readonly ProjectTag[];
  tagDraft: string;
}): ProjectTag[] {
  const query = tagDraft.trim().toLowerCase();
  return tags
    .filter((tag) => !query || tag.name.toLowerCase().includes(query))
    .slice(0, 8);
}

export function canCreateTag({
  projectTags,
  tagDraft,
}: {
  projectTags: readonly ProjectTag[];
  tagDraft: string;
}): boolean {
  const name = tagDraft.trim();
  if (!name) return false;
  return !projectTags
    .some((tag) => tag.name.trim().toLowerCase() === name.toLowerCase());
}

export function linkedEventRowsForTask({
  linkedEventIds,
  eventLinkResults,
  allProjectEvents,
}: {
  linkedEventIds: readonly string[];
  eventLinkResults: readonly ProjectLinkableEvent[];
  allProjectEvents: readonly CalendarEvent[];
}): ProjectLinkableEvent[] {
  const linkedEventIdSet = new Set(linkedEventIds);
  const rowsById = new Map(eventLinkResults.map((event) => [event.id, event]));
  for (const event of allProjectEvents) {
    if (linkedEventIdSet.has(event.id) && !rowsById.has(event.id)) {
      rowsById.set(event.id, calendarEventToLinkableEvent(event));
    }
  }
  return Array.from(rowsById.values())
    .filter((event) => linkedEventIdSet.has(event.id))
    .sort((a, b) => a.start.localeCompare(b.start));
}

export function calendarEventToLinkableEvent(event: CalendarEvent): ProjectLinkableEvent {
  return {
    id: event.id,
    projectId: event.projectId ?? "",
    title: event.title,
    start: event.start,
    end: event.end,
    timezone: event.timezone,
    calendarId: event.calendarId,
    color: event.color,
    allDay: event.allDay === true,
    status: event.status ?? "confirmed",
    linkedTasks: [],
  };
}
