import type { EventColor } from "$lib/calendar/types";
import type {
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectStatusCategory,
  ProjectTag,
} from "$lib/projects/types";

export type CollectionDraftError = "name_required" | "name_exists";

export type CollectionDraftResult<T> =
  | { ok: true; drafts: T[] }
  | { ok: false; error: CollectionDraftError };

export interface StatusDraftState {
  nameDrafts: Readonly<Record<string, string>>;
  categoryDrafts: Readonly<Record<string, ProjectStatusCategory>>;
  colorDrafts: Readonly<Record<string, EventColor>>;
  fallbackColor: EventColor;
}

export interface PriorityDraftState {
  nameDrafts: Readonly<Record<string, string>>;
  colorDrafts: Readonly<Record<string, EventColor>>;
  fallbackColor: EventColor;
}

export interface TagDraftState {
  nameDrafts: Readonly<Record<string, string>>;
  colorDrafts: Readonly<Record<string, EventColor>>;
  fallbackColor: EventColor;
}

export interface StatusSaveDraft {
  status: ProjectStatus;
  name: string;
  category: ProjectStatusCategory;
  color: EventColor;
}

export interface PrioritySaveDraft {
  priority: ProjectPriorityConfig;
  name: string;
  color: EventColor;
}

export interface TagSaveDraft {
  tag: ProjectTag;
  name: string;
  color: EventColor;
}

function normalizedDraftName(value: string): string {
  return value.trim().toLowerCase();
}

export function draftStatusName(
  status: ProjectStatus,
  state: StatusDraftState,
): string {
  return state.nameDrafts[status.id] ?? status.name;
}

export function draftStatusCategory(
  status: ProjectStatus,
  state: StatusDraftState,
): ProjectStatusCategory {
  return state.categoryDrafts[status.id] ?? status.category;
}

export function draftStatusColor(
  status: ProjectStatus,
  state: StatusDraftState,
): EventColor {
  return state.colorDrafts[status.id] ?? status.color ?? state.fallbackColor;
}

export function statusDraftDirty(
  status: ProjectStatus,
  state: StatusDraftState,
): boolean {
  return draftStatusName(status, state) !== status.name
    || draftStatusCategory(status, state) !== status.category
    || draftStatusColor(status, state) !== status.color;
}

export function statusSaveDrafts(
  statuses: readonly ProjectStatus[],
  state: StatusDraftState,
): CollectionDraftResult<StatusSaveDraft> {
  const drafts: StatusSaveDraft[] = [];
  for (const status of statuses) {
    if (!statusDraftDirty(status, state)) continue;
    const name = draftStatusName(status, state).trim();
    if (!name) return { ok: false, error: "name_required" };
    drafts.push({
      status,
      name,
      category: draftStatusCategory(status, state),
      color: draftStatusColor(status, state),
    });
  }
  return { ok: true, drafts };
}

export function draftPriorityName(
  priority: ProjectPriorityConfig,
  state: PriorityDraftState,
): string {
  return state.nameDrafts[priority.id] ?? priority.name;
}

export function draftPriorityColor(
  priority: ProjectPriorityConfig,
  state: PriorityDraftState,
): EventColor {
  return state.colorDrafts[priority.id] ?? priority.color ?? state.fallbackColor;
}

export function priorityDraftDirty(
  priority: ProjectPriorityConfig,
  state: PriorityDraftState,
): boolean {
  return draftPriorityName(priority, state) !== priority.name
    || draftPriorityColor(priority, state) !== priority.color;
}

export function prioritySaveDrafts(
  priorities: readonly ProjectPriorityConfig[],
  state: PriorityDraftState,
): CollectionDraftResult<PrioritySaveDraft> {
  const drafts: PrioritySaveDraft[] = [];
  for (const priority of priorities) {
    if (!priorityDraftDirty(priority, state)) continue;
    const name = draftPriorityName(priority, state).trim();
    if (!name) return { ok: false, error: "name_required" };
    drafts.push({
      priority,
      name,
      color: draftPriorityColor(priority, state),
    });
  }
  return { ok: true, drafts };
}

export function draftTagName(
  tag: ProjectTag,
  state: TagDraftState,
): string {
  return state.nameDrafts[tag.id] ?? tag.name;
}

export function draftTagColor(
  tag: ProjectTag,
  state: TagDraftState,
): EventColor {
  return state.colorDrafts[tag.id] ?? tag.color ?? state.fallbackColor;
}

export function tagDraftDirty(
  tag: ProjectTag,
  state: TagDraftState,
): boolean {
  return draftTagName(tag, state) !== tag.name
    || draftTagColor(tag, state) !== (tag.color ?? state.fallbackColor);
}

export function tagNameTaken(input: {
  tags: readonly ProjectTag[];
  name: string;
  ignoredTagId?: string;
}): boolean {
  const normalized = normalizedDraftName(input.name);
  if (!normalized) return false;
  return input.tags.some((tag) =>
    tag.id !== input.ignoredTagId && tag.name.trim().toLowerCase() === normalized
  );
}

export function tagSaveDrafts(
  tags: readonly ProjectTag[],
  state: TagDraftState,
): CollectionDraftResult<TagSaveDraft> {
  const drafts: TagSaveDraft[] = [];
  const seenNames = new Set<string>();
  for (const tag of tags) {
    const name = draftTagName(tag, state).trim();
    if (!name) return { ok: false, error: "name_required" };
    const normalizedName = normalizedDraftName(name);
    if (seenNames.has(normalizedName)) return { ok: false, error: "name_exists" };
    seenNames.add(normalizedName);
    if (!tagDraftDirty(tag, state)) continue;
    drafts.push({
      tag,
      name,
      color: draftTagColor(tag, state),
    });
  }
  return { ok: true, drafts };
}
