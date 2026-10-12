import { getEventColor } from "$lib/calendar/utils";
import type { EventColor } from "$lib/calendar/types";
import {
  blendHex,
  contrastRatio,
  pickReadableForeground,
  relativeLuminance,
} from "$lib/color/math";
import type { Translate } from "$lib/i18n/translator.svelte";
import { resolveAppTokens, type Theme } from "$lib/themes";
import type {
  ProjectCustomFieldType,
  ProjectLifecycleStatus,
  ProjectPriority,
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectTask,
  ProjectTaskChangeEvent,
  ProjectTaskType,
} from "./types";

export function projectPriorityLabel(priority: ProjectPriority, t: Translate): string {
  if (priority === "low") return t("projects.priority.low");
  if (priority === "high") return t("projects.priority.high");
  if (priority === "urgent") return t("projects.priority.urgent");
  return t("projects.priority.normal");
}

export function projectPriorityById(
  priorities: readonly ProjectPriorityConfig[],
  priorityId: ProjectPriority,
): ProjectPriorityConfig | undefined {
  return priorities.find((priority) => priority.id === priorityId);
}

export function projectPriorityDisplayLabel(
  priorityId: ProjectPriority,
  priorities: readonly ProjectPriorityConfig[],
  t: Translate,
): string {
  return projectPriorityById(priorities, priorityId)?.name ?? projectPriorityLabel(priorityId, t);
}

export function projectPriorityDisplayColor(
  priorityId: ProjectPriority,
  priorities: readonly ProjectPriorityConfig[],
): EventColor {
  const priority = projectPriorityById(priorities, priorityId);
  if (priority) return priority.color;
  if (priorityId === "urgent") return 2;
  if (priorityId === "high") return 7;
  if (priorityId === "normal") return 19;
  return 30;
}

export function projectLifecycleLabel(status: ProjectLifecycleStatus, t: Translate): string {
  if (status === "hidden") return t("projects.lifecycle.hidden");
  if (status === "archived") return t("projects.lifecycle.archived");
  return t("projects.lifecycle.active");
}

export function projectLifecycleBadgeClass(status: ProjectLifecycleStatus): string {
  if (status === "hidden") return "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  if (status === "archived") return "border-muted-foreground/30 bg-muted/50 text-muted-foreground";
  return "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
}

export function projectStatusBadgeClass(status: ProjectStatus | undefined): string {
  return status === undefined
    ? "bg-muted/50 text-muted-foreground"
    : "";
}

const STATUS_TEXT_CONTRAST_TARGET = 4.5;
const STATUS_DARK_TEXT_COLOR_WEIGHT = 0.22;
const STATUS_LIGHT_TEXT_COLOR_WEIGHT = 0.28;

function projectStatusForeground(
  background: string,
  color: string,
  appFg: string,
  darkSurface: boolean,
): string {
  const textAnchor = blendHex(
    color,
    appFg,
    darkSurface ? STATUS_DARK_TEXT_COLOR_WEIGHT : STATUS_LIGHT_TEXT_COLOR_WEIGHT,
  );
  if (contrastRatio(background, textAnchor) >= STATUS_TEXT_CONTRAST_TARGET) {
    return textAnchor;
  }

  return pickReadableForeground(background, {
    ink: textAnchor,
    canvas: appFg,
    target: STATUS_TEXT_CONTRAST_TARGET,
  });
}

function projectStatusPalette(color: EventColor, theme: Theme): {
  background: string;
  foreground: string;
  dot: string;
} {
  const tokens = resolveAppTokens(theme);
  const appBg = tokens["--background"];
  const appFg = tokens["--foreground"];
  const entry = getEventColor(color, theme);
  const darkSurface = relativeLuminance(appBg) < 0.45;
  const background = blendHex(entry.bg, appBg, darkSurface ? 0.48 : 0.18);
  const foreground = projectStatusForeground(background, entry.bg, appFg, darkSurface);
  return { background, foreground, dot: entry.bg };
}

export function projectStatusBadgeStyle(
  status: ProjectStatus | undefined,
  theme: Theme,
): string {
  if (!status) return "";
  const palette = projectStatusPalette(status.color, theme);
  return `background-color: ${palette.background}; color: ${palette.foreground};`;
}

export function projectStatusBadgeDotStyle(
  status: ProjectStatus | undefined,
  theme: Theme,
): string {
  if (!status) return "";
  const palette = projectStatusPalette(status.color, theme);
  return `background-color: ${palette.dot};`;
}

export function projectTaskArchivedBadgeClass(task: Pick<ProjectTask, "archivedAt">): string {
  return task.archivedAt
    ? "border-muted-foreground/30 bg-muted/50 text-muted-foreground"
    : "border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
}

export function projectPriorityBadgeClass(priority: ProjectPriority): string {
  if (priority === "urgent") return "border-destructive/40 bg-destructive/10 text-destructive";
  if (priority === "high") return "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  if (priority === "low") return "border-muted-foreground/20 bg-muted/30 text-muted-foreground";
  return "border-border bg-background/70 text-foreground";
}

export function projectTagColorDotStyle(color: EventColor | undefined, theme: Theme): string {
  if (color === undefined) return "";
  return `background-color: ${getEventColor(color, theme).bg};`;
}

export function projectTagColorSwatchClass(color: EventColor | undefined): string {
  return color === undefined ? "border-border bg-muted/50" : "border-transparent";
}

export function projectTaskTypeLabel(taskType: ProjectTaskType, t: Translate): string {
  if (taskType === "milestone") return t("projects.taskType.milestone");
  if (taskType === "bug") return t("projects.taskType.bug");
  if (taskType === "habit") return t("projects.taskType.habit");
  return t("projects.taskType.task");
}

export function projectCustomFieldTypeLabel(fieldType: ProjectCustomFieldType, t: Translate): string {
  if (fieldType === "number") return t("projects.customFields.typeNumber");
  if (fieldType === "select") return t("projects.customFields.typeSelect");
  if (fieldType === "multi_select") return t("projects.customFields.typeMultiSelect");
  if (fieldType === "status") return t("projects.customFields.typeStatus");
  if (fieldType === "date") return t("projects.customFields.typeDate");
  if (fieldType === "person") return t("projects.customFields.typePerson");
  if (fieldType === "files") return t("projects.customFields.typeFiles");
  if (fieldType === "checkbox") return t("projects.customFields.typeCheckbox");
  if (fieldType === "url") return t("projects.customFields.typeUrl");
  if (fieldType === "phone") return t("projects.customFields.typePhone");
  if (fieldType === "email") return t("projects.customFields.typeEmail");
  return t("projects.customFields.typeText");
}

export function projectTaskHistoryFieldLabel(fieldName: string, t: Translate): string {
  if (fieldName.startsWith("custom_field:")) return fieldName.slice("custom_field:".length);
  if (fieldName === "title") return t("projects.history.fields.title");
  if (fieldName === "description") return t("projects.history.fields.description");
  if (fieldName === "status") return t("projects.history.fields.status");
  if (fieldName === "section") return t("projects.history.fields.section");
  if (fieldName === "parent") return t("projects.history.fields.parent");
  if (fieldName === "priority") return t("projects.history.fields.priority");
  if (fieldName === "type") return t("projects.history.fields.type");
  if (fieldName === "estimate") return t("projects.history.fields.estimate");
  if (fieldName === "due_date") return t("projects.history.fields.dueDate");
  if (fieldName === "due_time") return t("projects.history.fields.dueTime");
  if (fieldName === "start_date") return t("projects.history.fields.startDate");
  if (fieldName === "start_time") return t("projects.history.fields.startTime");
  if (fieldName === "target_date") return t("projects.history.fields.targetDate");
  if (fieldName === "archived_at") return t("projects.history.fields.archiveState");
  if (fieldName === "blocker_reason") return t("projects.history.fields.blockerReason");
  if (fieldName === "milestone") return t("projects.history.fields.milestone");
  if (fieldName === "tags") return t("projects.history.fields.tags");
  if (fieldName === "checklist") return t("projects.history.fields.checklist");
  if (fieldName === "event_id") return t("projects.history.fields.event");
  if (fieldName === "blocking_task_id") return t("projects.history.fields.dependency");
  return fieldName;
}

export function projectTaskHistoryEventLabel(event: ProjectTaskChangeEvent, t: Translate): string {
  const field = event.fieldName ? projectTaskHistoryFieldLabel(event.fieldName, t) : "";
  const oldValue = event.oldValue ?? t("projects.history.emptyValue");
  const newValue = event.newValue ?? t("projects.history.emptyValue");
  if (event.eventType === "created") return t("projects.history.created");
  if (event.eventType === "scheduled") return t("projects.history.scheduled");
  if (event.eventType === "event_unlinked") return t("projects.history.eventUnlinked");
  if (event.eventType === "dependency_added") return t("projects.history.dependencyAdded", newValue);
  if (event.eventType === "dependency_removed") return t("projects.history.dependencyRemoved", oldValue);
  if (event.eventType === "completed") return t("projects.history.completed", oldValue, newValue);
  if (event.eventType === "reopened") return t("projects.history.reopened", oldValue, newValue);
  if (event.eventType === "archived") return t("projects.history.archived");
  if (event.fieldName) return t("projects.history.fieldChanged", field, oldValue, newValue);
  return t("projects.history.updated");
}
