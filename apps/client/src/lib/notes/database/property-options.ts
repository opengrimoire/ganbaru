import {
  NOTES_DATA_SOURCE_STATUS_GROUPS,
  type NotesDataSourceSelectColor,
  type NotesDataSourceStatusGroup,
} from "$lib/notes/types";
import type { NotesDataSourceSchemaOptionDraft, NotesDataSourceSchemaPropertyDraft } from "./data-source-schema";

/** Longest option name the native schema accepts, in characters. */
export const NOTES_DATA_SOURCE_OPTION_NAME_MAX_CHARACTERS = 120;

/** Most options one select, multi-select, or status property can hold. */
export const NOTES_DATA_SOURCE_MAX_OPTIONS = 100;

/** The option fields of a property draft that option edits change. */
export type NotesDataSourceOptionPatch = Pick<NotesDataSourceSchemaPropertyDraft, "options" | "statusDefaultOptionId">;

/** Outcome of one option edit: the changed option fields, or why nothing changed. */
export type NotesDataSourceOptionEdit =
  | { status: "changed"; patch: NotesDataSourceOptionPatch }
  | { status: "unchanged" | "missing" | "duplicate" | "limit" };

/** Where a dragged option lands relative to the option under the pointer. */
export type NotesDataSourceOptionPlacement = "before" | "after";

/** The property fields that option edits read. */
export type NotesDataSourceOptionProperty = Pick<NotesDataSourceSchemaPropertyDraft, "type" | "options" | "statusDefaultOptionId">;

/**
 * One option edit, applied to the newest saved property rather than to what an editor last showed, so edits made in
 * quick succession build on each other.
 */
export type NotesDataSourceOptionEditor = (property: NotesDataSourceOptionProperty) => NotesDataSourceOptionEdit;

/** Colors given to new options in turn, vivid ones first, so options added one after another are easy to tell apart. */
const NEW_OPTION_COLORS: readonly NotesDataSourceSelectColor[] = ["blue", "green", "yellow", "orange", "purple", "pink", "red", "gray", "brown"];

type OptionProperty = NotesDataSourceOptionProperty;

/** Trim a typed option name and cut it to the native limit, counting characters rather than UTF-16 units. */
export function notesDataSourceOptionName(rawName: string): string {
  return Array.from(rawName.trim()).slice(0, NOTES_DATA_SOURCE_OPTION_NAME_MAX_CHARACTERS).join("").trim();
}

/** Whether another option of the property already uses the name, ignoring case like the native schema. */
function nameTaken(options: readonly NotesDataSourceSchemaOptionDraft[], name: string, exceptId?: string): boolean {
  const key = name.toLowerCase();
  return options.some((option) => option.id !== exceptId && option.name.trim().toLowerCase() === key);
}

function changed(property: OptionProperty, options: NotesDataSourceSchemaOptionDraft[], statusDefaultOptionId = property.statusDefaultOptionId): NotesDataSourceOptionEdit {
  return { status: "changed", patch: { options, statusDefaultOptionId } };
}

/**
 * Status options in the order people see them: by group, then by saved order within the group.
 * Other option types keep their saved order.
 */
export function notesDataSourceVisibleOptions(property: Pick<NotesDataSourceSchemaPropertyDraft, "type" | "options">): NotesDataSourceSchemaOptionDraft[] {
  if (property.type !== "status") return [...property.options];
  return NOTES_DATA_SOURCE_STATUS_GROUPS.flatMap((group) => property.options.filter((option) => option.group === group));
}

/**
 * Add an option with the next color in turn. A status option joins the end of the given group.
 *
 * @param property Property whose options change.
 * @param rawName Typed name; blank names change nothing.
 * @param group Status group for the new option; ignored by other types.
 * @param id Identity for the new option.
 */
export function notesDataSourceAddOption(
  property: OptionProperty,
  rawName: string,
  group: NotesDataSourceStatusGroup = "To-do",
  id: string = crypto.randomUUID(),
): NotesDataSourceOptionEdit {
  const name = notesDataSourceOptionName(rawName);
  if (!name) return { status: "unchanged" };
  if (nameTaken(property.options, name)) return { status: "duplicate" };
  if (property.options.length >= NOTES_DATA_SOURCE_MAX_OPTIONS) return { status: "limit" };
  const color = NEW_OPTION_COLORS[property.options.length % NEW_OPTION_COLORS.length] ?? "default";
  const option: NotesDataSourceSchemaOptionDraft = { id, name, color, group: property.type === "status" ? group : "To-do" };
  if (property.type !== "status") return changed(property, [...property.options, option]);
  const visible = notesDataSourceVisibleOptions(property);
  const lastInGroup = visible.findLastIndex((candidate) => candidate.group === group);
  const groupIndex = NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(group);
  const insertAt = lastInGroup >= 0
    ? lastInGroup + 1
    : visible.findIndex((candidate) => NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(candidate.group) > groupIndex);
  const next = [...visible];
  next.splice(insertAt < 0 ? next.length : insertAt, 0, option);
  return changed(property, next);
}

/** Rename one option; blank and unchanged names change nothing, and another option's name is rejected ignoring case. */
export function notesDataSourceRenameOption(property: OptionProperty, optionId: string, rawName: string): NotesDataSourceOptionEdit {
  const target = property.options.find((option) => option.id === optionId);
  if (!target) return { status: "missing" };
  const name = notesDataSourceOptionName(rawName);
  if (!name || name === target.name) return { status: "unchanged" };
  if (nameTaken(property.options, name, optionId)) return { status: "duplicate" };
  return changed(property, property.options.map((option) => option.id === optionId ? { ...option, name } : option));
}

/** Give one option another color. */
export function notesDataSourceRecolorOption(property: OptionProperty, optionId: string, color: NotesDataSourceSelectColor): NotesDataSourceOptionEdit {
  const target = property.options.find((option) => option.id === optionId);
  if (!target) return { status: "missing" };
  if (target.color === color) return { status: "unchanged" };
  return changed(property, property.options.map((option) => option.id === optionId ? { ...option, color } : option));
}

/** Remove one option; removing the status default lets the native schema choose the next default. */
export function notesDataSourceDeleteOption(property: OptionProperty, optionId: string): NotesDataSourceOptionEdit {
  if (!property.options.some((option) => option.id === optionId)) return { status: "missing" };
  return changed(
    property,
    property.options.filter((option) => option.id !== optionId),
    property.statusDefaultOptionId === optionId ? "" : property.statusDefaultOptionId,
  );
}

/** Make one status option the value new rows receive. */
export function notesDataSourceSetDefaultOption(property: OptionProperty, optionId: string): NotesDataSourceOptionEdit {
  if (!property.options.some((option) => option.id === optionId)) return { status: "missing" };
  if (property.statusDefaultOptionId === optionId) return { status: "unchanged" };
  return changed(property, [...property.options], optionId);
}

/**
 * Move one option next to another, or to the end of a status group when there is no target. A status option takes
 * the group of the option it lands next to, or the given group.
 *
 * @param property Property whose options change.
 * @param optionId Option being moved.
 * @param target Option it lands next to and on which side, or null to land at the end of `group`.
 * @param group Status group to land in when there is no target; ignored by other types.
 */
export function notesDataSourceMoveOption(
  property: OptionProperty,
  optionId: string,
  target: { optionId: string; placement: NotesDataSourceOptionPlacement } | null,
  group?: NotesDataSourceStatusGroup,
): NotesDataSourceOptionEdit {
  const visible = notesDataSourceVisibleOptions(property);
  const moved = visible.find((option) => option.id === optionId);
  if (!moved || target?.optionId === optionId) return moved ? { status: "unchanged" } : { status: "missing" };
  const remaining = visible.filter((option) => option.id !== optionId);
  let insertAt: number;
  let landingGroup = moved.group;
  if (target) {
    const targetIndex = remaining.findIndex((option) => option.id === target.optionId);
    if (targetIndex < 0) return { status: "missing" };
    insertAt = target.placement === "before" ? targetIndex : targetIndex + 1;
    landingGroup = remaining[targetIndex]?.group ?? moved.group;
  } else if (property.type === "status" && group) {
    landingGroup = group;
    const groupIndex = NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(group);
    const after = remaining.findIndex((option) => NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(option.group) > groupIndex);
    insertAt = after < 0 ? remaining.length : after;
  } else {
    insertAt = remaining.length;
  }
  const next = [...remaining];
  next.splice(insertAt, 0, property.type === "status" ? { ...moved, group: landingGroup } : moved);
  const unchanged = next.every((option, index) => option.id === visible[index]?.id && option.group === visible[index]?.group);
  return unchanged ? { status: "unchanged" } : changed(property, next);
}

/**
 * Move one option a single step, such as from a keyboard reorder. A status option at the edge of its group moves into
 * the neighboring group instead of past the next option.
 */
export function notesDataSourceStepOption(property: OptionProperty, optionId: string, direction: -1 | 1): NotesDataSourceOptionEdit {
  const visible = notesDataSourceVisibleOptions(property);
  const index = visible.findIndex((option) => option.id === optionId);
  const moved = visible[index];
  if (!moved) return { status: "missing" };
  const neighbor = visible[index + direction];
  if (property.type !== "status") {
    return neighbor ? notesDataSourceMoveOption(property, optionId, { optionId: neighbor.id, placement: direction < 0 ? "before" : "after" }) : { status: "unchanged" };
  }
  if (neighbor && neighbor.group === moved.group) {
    return notesDataSourceMoveOption(property, optionId, { optionId: neighbor.id, placement: direction < 0 ? "before" : "after" });
  }
  const groupIndex = NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(moved.group);
  const nextGroup = NOTES_DATA_SOURCE_STATUS_GROUPS[groupIndex + direction];
  if (!nextGroup) return { status: "unchanged" };
  const remaining = visible.filter((option) => option.id !== optionId);
  const next = [...remaining];
  const landing = { ...moved, group: nextGroup };
  if (direction > 0) {
    const firstOfGroup = remaining.findIndex((option) => option.group === nextGroup);
    const later = remaining.findIndex((option) => NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(option.group) > groupIndex + 1);
    next.splice(firstOfGroup >= 0 ? firstOfGroup : later >= 0 ? later : remaining.length, 0, landing);
  } else {
    const lastOfGroup = remaining.findLastIndex((option) => option.group === nextGroup);
    const earlier = remaining.findLastIndex((option) => NOTES_DATA_SOURCE_STATUS_GROUPS.indexOf(option.group) < groupIndex - 1);
    next.splice(lastOfGroup >= 0 ? lastOfGroup + 1 : earlier + 1, 0, landing);
  }
  return changed(property, next);
}
