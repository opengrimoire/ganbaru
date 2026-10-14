import type { TranslationKey } from "$lib/i18n/translator.svelte";

/** Orders a sidebar can list its items in, shared by Notes, Projects, and Chat. */
export const SIDEBAR_SORT_ORDERS = [
  "name-asc",
  "name-desc",
  "modified-desc",
  "modified-asc",
  "created-desc",
  "created-asc",
  "custom",
] as const;

/** One sidebar order: by name, last modification, or creation, in either direction, or the order people drag items into. */
export type SidebarSortOrder = (typeof SIDEBAR_SORT_ORDERS)[number];

/** Order used until the user picks another one. */
export const DEFAULT_SIDEBAR_SORT_ORDER: SidebarSortOrder = "name-asc";

/** Label key for each order, in menu order. */
export const SIDEBAR_SORT_ORDER_LABEL_KEYS = {
  "name-asc": "common.sortOrder.nameAscending",
  "name-desc": "common.sortOrder.nameDescending",
  "modified-desc": "common.sortOrder.modifiedDescending",
  "modified-asc": "common.sortOrder.modifiedAscending",
  "created-desc": "common.sortOrder.createdDescending",
  "created-asc": "common.sortOrder.createdAscending",
  custom: "common.sortOrder.custom",
} as const satisfies Record<SidebarSortOrder, TranslationKey>;

/**
 * Fields a sidebar item is ordered by. Timestamps are ISO 8601 strings, so they order lexically. `rank` places the item
 * in the custom order; items without one follow the ranked items.
 */
export interface SidebarSortFields {
  id: string;
  name: string;
  createdAt: string;
  modifiedAt: string;
  rank?: number;
}

const nameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Whether a stored value is a known sidebar order. */
export function isSidebarSortOrder(value: unknown): value is SidebarSortOrder {
  return typeof value === "string" && (SIDEBAR_SORT_ORDERS as readonly string[]).includes(value);
}

/** Compare display names the way people read them: case-insensitive, accent-insensitive, numbers by value. */
export function compareSidebarNames(left: string, right: string): number {
  return nameCollator.compare(left, right);
}

function compareText(left: string, right: string): number {
  if (left === right) return 0;
  return left < right ? -1 : 1;
}

/** Ranked items come first, lower ranks before higher ones. */
function compareRanks(left: number | undefined, right: number | undefined): number {
  if (left === right) return 0;
  if (left === undefined) return 1;
  if (right === undefined) return -1;
  return left - right;
}

/** Compare two items by `order`; ties fall back to name, then id, so the result is stable. */
export function compareSidebarItems(left: SidebarSortFields, right: SidebarSortFields, order: SidebarSortOrder): number {
  const nameComparison = compareSidebarNames(left.name, right.name);
  const idComparison = compareText(left.id, right.id);
  if (order === "name-asc") return nameComparison || idComparison;
  if (order === "name-desc") return -nameComparison || idComparison;
  if (order === "custom") return compareRanks(left.rank, right.rank) || nameComparison || idComparison;
  const timeComparison = order.startsWith("modified")
    ? compareText(left.modifiedAt, right.modifiedAt)
    : compareText(left.createdAt, right.createdAt);
  const directedTimeComparison = order.endsWith("asc") ? timeComparison : -timeComparison;
  return directedTimeComparison || nameComparison || idComparison;
}

/** Sort a copy of `items` by `order`, putting starred items first when `starred` is given. */
export function sortSidebarItems<T>(
  items: readonly T[],
  order: SidebarSortOrder,
  fields: (item: T) => SidebarSortFields,
  starred?: (item: T) => boolean,
): T[] {
  return [...items].sort((left, right) =>
    compareStarredFirst(starred?.(left) ?? false, starred?.(right) ?? false)
      || compareSidebarItems(fields(left), fields(right), order)
  );
}

/** Sort a copy of `items` so starred ones come first, keeping the given order within each part. */
export function starredFirst<T>(items: readonly T[], starred: (item: T) => boolean): T[] {
  return [...items.filter(starred), ...items.filter((item) => !starred(item))];
}

/** Negative when only `left` is starred, positive when only `right` is. */
export function compareStarredFirst(leftStarred: boolean, rightStarred: boolean): number {
  if (leftStarred === rightStarred) return 0;
  return leftStarred ? -1 : 1;
}

/** A per-project order wins over the global one; `null` or a missing entry means use the global order. */
export function resolveSidebarSortOrder(
  globalOrder: SidebarSortOrder,
  projectOrder: SidebarSortOrder | null | undefined,
): SidebarSortOrder {
  return projectOrder ?? globalOrder;
}

/** Read a stored map of project id to order, dropping entries that are not valid orders. */
export function parseSidebarSortOrderOverrides(value: unknown): Record<string, SidebarSortOrder> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const overrides: Record<string, SidebarSortOrder> = {};
  for (const [projectId, order] of Object.entries(value)) {
    if (projectId.trim() && isSidebarSortOrder(order)) overrides[projectId] = order;
  }
  return overrides;
}

/** Copy of `overrides` with `projectId` set to `order`, or removed when `order` is `null`. */
export function withSidebarSortOrderOverride(
  overrides: Readonly<Record<string, SidebarSortOrder>>,
  projectId: string,
  order: SidebarSortOrder | null,
): Record<string, SidebarSortOrder> {
  const next = { ...overrides };
  if (order) next[projectId] = order;
  else delete next[projectId];
  return next;
}

/** Read a stored list of starred ids, dropping blanks and duplicates. */
export function parseStarredIds(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const ids: string[] = [];
  for (const id of value) {
    if (typeof id === "string" && id.trim() && !ids.includes(id)) ids.push(id);
  }
  return ids;
}

/** Copy of `ids` with `id` starred (added first) or unstarred. */
export function withStarredId(ids: readonly string[], id: string, starred: boolean): string[] {
  const rest = ids.filter((existing) => existing !== id);
  return starred ? [id, ...rest] : rest;
}

/** Rank of each id in a stored custom order, for `SidebarSortFields.rank`. */
export function customOrderRanks(ids: readonly string[]): Map<string, number> {
  return new Map(ids.map((id, index) => [id, index]));
}

/**
 * Copy of the stored custom order `ids` with `orderedSiblingIds` in their new order. Ranks are only compared among
 * siblings, so the siblings go first and every other id keeps its place relative to the rest.
 */
export function withCustomSiblingOrder(ids: readonly string[], orderedSiblingIds: readonly string[]): string[] {
  const siblings = new Set(orderedSiblingIds);
  return [...orderedSiblingIds, ...ids.filter((id) => !siblings.has(id))];
}

/**
 * Write `reordered`, a reordered part of `full`, back into the slots its ids held in `full`, so the ids outside it (for
 * example hidden or starred items) keep their positions.
 */
export function mergeReorderedSubset(full: readonly string[], reordered: readonly string[]): string[] {
  const subset = new Set(reordered);
  const queue = [...reordered];
  return full.map((id) => (subset.has(id) ? queue.shift() ?? id : id));
}

/** Copy of `ids` with `id` moved to `targetIndex`, or the same order when `id` is missing. */
export function movedToIndex(ids: readonly string[], id: string, targetIndex: number): string[] {
  const sourceIndex = ids.indexOf(id);
  if (sourceIndex < 0) return [...ids];
  const next = ids.filter((existing) => existing !== id);
  next.splice(Math.min(Math.max(targetIndex, 0), next.length), 0, id);
  return next;
}

/** Read a stored map of scope id to custom id order, dropping invalid scopes and ids. */
export function parseCustomOrders(value: unknown): Record<string, string[]> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const orders: Record<string, string[]> = {};
  for (const [scopeId, ids] of Object.entries(value)) {
    const parsed = parseStarredIds(ids);
    if (scopeId.trim() && parsed.length > 0) orders[scopeId] = parsed;
  }
  return orders;
}

/** The ids among `ids` that share the starred state of `id`, since starred items stay ahead and only move among themselves. */
export function sameStarredSiblings(ids: readonly string[], id: string, starred: (id: string) => boolean): string[] {
  const idStarred = starred(id);
  return ids.filter((other) => starred(other) === idStarred);
}
