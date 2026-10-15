import { isView, type View } from "$lib/navigation";

/** How specific a tab is: a whole kind, one project inside a kind, or one item. */
export type WorkspaceTabLevel = "kind" | "project" | "item";

/** A single place inside a kind that a tab can point at. */
export type WorkspaceTabItem =
  | { type: "project-view"; id: string }
  | { type: "notes-page"; id: string }
  | { type: "notes-folder"; id: string }
  | { type: "chat-channel"; id: string };

export type WorkspaceTabItemType = WorkspaceTabItem["type"];

/** Serializable location a tab restores into the singleton view stores. */
export interface WorkspaceTabLocation {
  kind: View;
  projectId: string | null;
  item: WorkspaceTabItem | null;
}

export interface WorkspaceTab {
  id: string;
  level: WorkspaceTabLevel;
  /** A locked tab keeps its scope; navigating outside it opens another tab. */
  locked: boolean;
  location: WorkspaceTabLocation;
  /** Last known item name, rendered while the tab is inactive. Kind and project tabs have none and show their kind. */
  title: string | null;
  /** Last known name of the project the tab shows, rendered beside its label. */
  projectTitle: string | null;
}

/** A closed tab kept for reopening at its former position. */
export interface ClosedWorkspaceTab {
  level: WorkspaceTabLevel;
  locked: boolean;
  location: WorkspaceTabLocation;
  title: string | null;
  projectTitle: string | null;
  index: number;
}

export interface WorkspaceTabsState {
  tabs: WorkspaceTab[];
  activeTabId: string;
  /** Most recently closed first. */
  closed: ClosedWorkspaceTab[];
}

export type WorkspaceTabPlacement = "before" | "after";
export type WorkspaceTabIdFactory = () => string;

export const MAX_WORKSPACE_TABS = 32;
export const MAX_CLOSED_WORKSPACE_TABS = 10;
const MAX_ID_LENGTH = 1_024;
const MAX_TITLE_LENGTH = 200;
/** Kinds opened when a vault has no saved tabs yet. */
export const DEFAULT_WORKSPACE_TAB_KINDS: readonly View[] = ["calendar", "projects"];

const ITEM_TYPES_BY_KIND: Readonly<Record<View, readonly WorkspaceTabItemType[]>> = {
  calendar: [],
  projects: ["project-view"],
  notes: ["notes-page", "notes-folder"],
  chat: ["chat-channel"],
};
const LEVELS: readonly WorkspaceTabLevel[] = ["kind", "project", "item"];

/** Return the location a kind opens at before any project or item is chosen. */
export function kindLocation(kind: View): WorkspaceTabLocation {
  return { kind, projectId: null, item: null };
}

export function sameTabItem(a: WorkspaceTabItem | null, b: WorkspaceTabItem | null): boolean {
  if (!a || !b) return a === b;
  return a.type === b.type && a.id === b.id;
}

export function sameTabLocation(a: WorkspaceTabLocation, b: WorkspaceTabLocation): boolean {
  return a.kind === b.kind && a.projectId === b.projectId && sameTabItem(a.item, b.item);
}

/** Return whether a location stays inside the scope a tab's level and lock define. */
export function isWithinTabScope(tab: WorkspaceTab, location: WorkspaceTabLocation): boolean {
  if (tab.location.kind !== location.kind) return false;
  if (!tab.locked || tab.level === "kind") return true;
  if (tab.level === "project") return tab.location.projectId === location.projectId;
  return sameTabItem(tab.location.item, location.item);
}

/** Return whether an open tab already shows what a new tab at this level and location would show. */
export function tabMatchesTarget(
  tab: WorkspaceTab,
  level: WorkspaceTabLevel,
  location: WorkspaceTabLocation,
): boolean {
  if (tab.level !== level || tab.location.kind !== location.kind) return false;
  if (level === "kind") return true;
  if (level === "project") return tab.location.projectId === location.projectId;
  return sameTabItem(tab.location.item, location.item);
}

/** Lower a level its location cannot support: items need an item and projects need a project. */
export function supportedTabLevel(level: WorkspaceTabLevel, location: WorkspaceTabLocation): WorkspaceTabLevel {
  if (level === "item" && location.item) return "item";
  if (level !== "kind" && location.projectId) return "project";
  return "kind";
}

/**
 * Return the level a tab keeps once locked, or null when it shows nothing to lock to. A kind tab narrows to
 * its project, or to its item when it has no project, so locking always pins what the tab shows now.
 */
export function workspaceTabLockLevel(tab: Pick<WorkspaceTab, "level" | "location">): WorkspaceTabLevel | null {
  if (tab.level !== "kind") return tab.level;
  if (tab.location.projectId) return "project";
  return tab.location.item ? "item" : null;
}

export function createWorkspaceTab(
  id: string,
  location: WorkspaceTabLocation,
  options: { level?: WorkspaceTabLevel; locked?: boolean; title?: string | null; projectTitle?: string | null } = {},
): WorkspaceTab {
  const level = supportedTabLevel(options.level ?? "kind", location);
  return {
    id,
    level,
    locked: level !== "kind" && options.locked === true,
    location,
    title: options.title ?? null,
    projectTitle: options.projectTitle ?? null,
  };
}

/** Build the first-run tabs from the kinds this platform exposes. */
export function defaultWorkspaceTabsState(
  availableKinds: readonly View[],
  createId: WorkspaceTabIdFactory,
): WorkspaceTabsState {
  const defaults = DEFAULT_WORKSPACE_TAB_KINDS.filter((kind) => availableKinds.includes(kind));
  const kinds = defaults.length > 0 ? defaults : availableKinds.slice(0, 1);
  const tabs = (kinds.length > 0 ? kinds : ["calendar" as const])
    .map((kind) => createWorkspaceTab(createId(), kindLocation(kind)));
  return { tabs, activeTabId: tabs[0].id, closed: [] };
}

export function activeWorkspaceTab(state: WorkspaceTabsState): WorkspaceTab {
  return state.tabs.find((tab) => tab.id === state.activeTabId) ?? state.tabs[0];
}

export function activateWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  if (state.activeTabId === id) return state;
  const tab = state.tabs.find((candidate) => candidate.id === id);
  return tab ? { ...state, activeTabId: id } : state;
}

export function canAddWorkspaceTab(state: WorkspaceTabsState): boolean {
  return state.tabs.length < MAX_WORKSPACE_TABS;
}

/** Where a new tab goes: the end, right after the active tab, or right after a given tab. */
export type WorkspaceTabInsertPosition = "end" | "after-active" | { afterId: string };

/**
 * Insert a tab, activating it unless `activate` is false. A position after a tab that no longer exists
 * falls back to after the active tab. Returns the same state when the tab limit is reached.
 */
export function insertWorkspaceTab(
  state: WorkspaceTabsState,
  tab: WorkspaceTab,
  position: WorkspaceTabInsertPosition,
  activate = true,
): WorkspaceTabsState {
  if (!canAddWorkspaceTab(state)) return state;
  const tabs = [...state.tabs];
  const anchorId = typeof position === "object" ? position.afterId : state.activeTabId;
  const anchorIndex = tabs.findIndex((candidate) => candidate.id === anchorId);
  const activeIndex = tabs.findIndex((candidate) => candidate.id === state.activeTabId);
  const afterIndex = anchorIndex >= 0 ? anchorIndex : activeIndex;
  const index = position !== "end" && afterIndex >= 0 ? afterIndex + 1 : tabs.length;
  tabs.splice(index, 0, tab);
  return { ...state, tabs, activeTabId: activate ? tab.id : state.activeTabId };
}

/** Replace one tab, returning the same state when nothing changed. */
export function updateWorkspaceTab(
  state: WorkspaceTabsState,
  id: string,
  patch: Partial<Omit<WorkspaceTab, "id">>,
): WorkspaceTabsState {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  const current = state.tabs[index];
  const merged = { ...current, ...patch };
  const level = supportedTabLevel(merged.level, merged.location);
  const next = { ...merged, level, locked: level !== "kind" && merged.locked };
  if (
    next.level === current.level
    && next.locked === current.locked
    && next.title === current.title
    && next.projectTitle === current.projectTitle
    && sameTabLocation(next.location, current.location)
  ) return state;
  const tabs = [...state.tabs];
  tabs[index] = next;
  return { ...state, tabs };
}

function rememberClosed(
  closed: readonly ClosedWorkspaceTab[],
  tab: WorkspaceTab,
  index: number,
): ClosedWorkspaceTab[] {
  const entry: ClosedWorkspaceTab = {
    level: tab.level,
    locked: tab.locked,
    location: tab.location,
    title: tab.title,
    projectTitle: tab.projectTitle,
    index,
  };
  return [entry, ...closed].slice(0, MAX_CLOSED_WORKSPACE_TABS);
}

/** Remember closed tabs at the end of the list, for example the tabs of a window that closed. */
export function rememberClosedWorkspaceTabs(
  state: WorkspaceTabsState,
  tabs: readonly WorkspaceTab[],
): WorkspaceTabsState {
  if (tabs.length === 0) return state;
  let closed = state.closed;
  for (const tab of tabs) closed = rememberClosed(closed, tab, state.tabs.length);
  return { ...state, closed };
}

/** Pick the tab that takes focus when one closes or leaves: its right neighbor, else its left one. */
function neighborTabId(tabs: readonly WorkspaceTab[], id: string): string | null {
  const index = tabs.findIndex((tab) => tab.id === id);
  const neighbor = tabs[index + 1] ?? tabs[index - 1];
  return neighbor && neighbor.id !== id ? neighbor.id : null;
}

/**
 * Show the neighbor of the active tab `id` while `id` is pulled out of the strip, keeping its slot. Any other
 * tab, or a tab alone in its window, leaves the state unchanged.
 */
export function showWorkspaceTabNeighbor(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  if (state.activeTabId !== id) return state;
  const neighbor = neighborTabId(state.tabs, id);
  return neighbor ? activateWorkspaceTab(state, neighbor) : state;
}

/**
 * Close a tab and remember it for reopening. Returns null when it is the window's last tab, which the
 * caller turns into closing the window.
 */
export function closeWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState | null {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  if (state.tabs.length <= 1) return null;
  const tab = state.tabs[index];
  const without = takeWorkspaceTab(state, id);
  return without && { ...without, closed: rememberClosed(state.closed, tab, index) };
}

/**
 * Remove a tab without remembering it, as when it moves to another window. Returns null when it is the
 * window's last tab, so the window has nothing left to show.
 */
export function takeWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState | null {
  if (!state.tabs.some((tab) => tab.id === id)) return state;
  const tabs = state.tabs.filter((candidate) => candidate.id !== id);
  if (tabs.length === 0) return null;
  const activeTabId = state.activeTabId === id ? neighborTabId(state.tabs, id) ?? tabs[0].id : state.activeTabId;
  return { ...state, tabs, activeTabId };
}

/** Close every tab except one. */
export function closeOtherWorkspaceTabs(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  if (!state.tabs.some((tab) => tab.id === id)) return state;
  let closed = state.closed;
  state.tabs.forEach((tab, index) => {
    if (tab.id !== id) closed = rememberClosed(closed, tab, index);
  });
  return {
    tabs: state.tabs.filter((tab) => tab.id === id),
    activeTabId: id,
    closed,
  };
}

/** Move a tab before or after another one. */
export function moveWorkspaceTab(
  state: WorkspaceTabsState,
  id: string,
  targetId: string,
  placement: WorkspaceTabPlacement,
): WorkspaceTabsState {
  if (id === targetId) return state;
  const moving = state.tabs.find((tab) => tab.id === id);
  if (!moving) return state;
  const tabs = state.tabs.filter((tab) => tab.id !== id);
  const targetIndex = tabs.findIndex((tab) => tab.id === targetId);
  if (targetIndex < 0) return state;
  tabs.splice(placement === "before" ? targetIndex : targetIndex + 1, 0, moving);
  if (tabs.every((tab, index) => tab === state.tabs[index])) return state;
  return { ...state, tabs };
}

/** Reopen a remembered tab at its former position, the most recent one by default. */
export function reopenWorkspaceTab(
  state: WorkspaceTabsState,
  createId: WorkspaceTabIdFactory,
  closedIndex = 0,
): { state: WorkspaceTabsState; tab: WorkspaceTab } | null {
  const entry = state.closed[closedIndex];
  if (!entry || !canAddWorkspaceTab(state)) return null;
  const tab = createWorkspaceTab(createId(), entry.location, {
    level: entry.level,
    locked: entry.locked,
    title: entry.title,
    projectTitle: entry.projectTitle,
  });
  const tabs = [...state.tabs];
  tabs.splice(Math.min(entry.index, tabs.length), 0, tab);
  const closed = state.closed.filter((_, index) => index !== closedIndex);
  return { state: { tabs, activeTabId: tab.id, closed }, tab };
}

/** Return the tab next to the active one, wrapping around the ends. */
export function relativeWorkspaceTabId(state: WorkspaceTabsState, direction: -1 | 1): string | null {
  const { tabs } = state;
  if (tabs.length === 0) return null;
  const index = Math.max(0, tabs.findIndex((tab) => tab.id === state.activeTabId));
  return tabs[(index + direction + tabs.length) % tabs.length].id;
}

/** Return the tab at a zero-based position; the ninth position always means the last tab. */
export function workspaceTabIdAtPosition(state: WorkspaceTabsState, index: number): string | null {
  const { tabs } = state;
  if (index === 8) return tabs.at(-1)?.id ?? null;
  return tabs[index]?.id ?? null;
}

function boundedId(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_ID_LENGTH ? value : null;
}

function optionalId(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;
  return boundedId(value) ?? undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseTabItem(value: unknown, kind: View): WorkspaceTabItem | null | undefined {
  if (value === null || value === undefined) return null;
  if (!isRecord(value)) return undefined;
  const id = boundedId(value.id);
  const type = ITEM_TYPES_BY_KIND[kind].find((candidate) => candidate === value.type);
  return id && type ? { type, id } : undefined;
}

/** Validate an untrusted location, rejecting items that do not belong to its kind. */
export function parseWorkspaceTabLocation(value: unknown): WorkspaceTabLocation | null {
  if (!isRecord(value) || !isView(value.kind)) return null;
  const kind = value.kind;
  if (kind === "calendar") return kindLocation(kind);
  const projectId = optionalId(value.projectId);
  const item = parseTabItem(value.item, kind);
  if (projectId === undefined || item === undefined) return null;
  return { kind, projectId, item };
}

function parseLevel(value: unknown): WorkspaceTabLevel | null {
  return LEVELS.find((level) => level === value) ?? null;
}

function parseTitle(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value.slice(0, MAX_TITLE_LENGTH) : null;
}

/** Validate one untrusted tab, such as a tab moved in from another window. */
export function parseWorkspaceTab(value: unknown, availableKinds: readonly View[]): WorkspaceTab | null {
  if (!isRecord(value)) return null;
  const id = boundedId(value.id);
  const level = parseLevel(value.level);
  const location = parseWorkspaceTabLocation(value.location);
  if (!id || !level || !location || !availableKinds.includes(location.kind)) return null;
  return createWorkspaceTab(id, location, {
    level,
    locked: value.locked === true,
    title: parseTitle(value.title),
    projectTitle: parseTitle(value.projectTitle),
  });
}

function parseClosedTab(value: unknown, availableKinds: readonly View[]): ClosedWorkspaceTab | null {
  if (!isRecord(value)) return null;
  const level = parseLevel(value.level);
  const location = parseWorkspaceTabLocation(value.location);
  const index = typeof value.index === "number" && Number.isInteger(value.index) && value.index >= 0
    ? Math.min(value.index, MAX_WORKSPACE_TABS)
    : null;
  if (!level || !location || index === null || !availableKinds.includes(location.kind)) return null;
  const supported = supportedTabLevel(level, location);
  return {
    level: supported,
    locked: supported !== "kind" && value.locked === true,
    location,
    title: parseTitle(value.title),
    projectTitle: parseTitle(value.projectTitle),
    index,
  };
}

/**
 * Labels of the windows besides the main one that saved tabs, read from the storage keys under `prefix`.
 * The main window's key and keys nested deeper than one label are left out.
 */
export function savedWindowLabels(keys: Iterable<string>, prefix: string): string[] {
  const labels: string[] = [];
  for (const key of keys) {
    if (!key.startsWith(prefix)) continue;
    const label = key.slice(prefix.length);
    if (label.length > 0 && !label.includes(":")) labels.push(label);
  }
  return labels;
}

/**
 * Validate a window's persisted tabs. Returns null when nothing usable remains. The reopen history is saved
 * apart, shared by every window, so the state comes back without it.
 */
export function parseWorkspaceTabsState(
  value: unknown,
  availableKinds: readonly View[],
): WorkspaceTabsState | null {
  if (!isRecord(value) || !Array.isArray(value.tabs)) return null;
  const seen = new Set<string>();
  const tabs = value.tabs.slice(0, MAX_WORKSPACE_TABS).flatMap((entry) => {
    const tab = parseWorkspaceTab(entry, availableKinds);
    if (!tab || seen.has(tab.id)) return [];
    seen.add(tab.id);
    return [tab];
  });
  if (tabs.length === 0) return null;
  const activeTabId = tabs.find((tab) => tab.id === value.activeTabId)?.id ?? tabs[0].id;
  return { tabs, activeTabId, closed: [] };
}

/** Return the persisted shape of a window's tabs, without the shared reopen history. */
export function serializeWorkspaceTabsState(state: WorkspaceTabsState): string {
  return JSON.stringify({
    tabs: state.tabs,
    activeTabId: state.activeTabId,
  });
}

/** Validate the persisted reopen history that every window shares, keeping only valid entries. */
export function parseClosedWorkspaceTabs(value: unknown, availableKinds: readonly View[]): ClosedWorkspaceTab[] {
  if (!Array.isArray(value)) return [];
  return value.slice(0, MAX_CLOSED_WORKSPACE_TABS).flatMap((entry) => {
    const parsed = parseClosedTab(entry, availableKinds);
    return parsed ? [parsed] : [];
  });
}

/** Return the persisted shape of the shared reopen history. */
export function serializeClosedWorkspaceTabs(closed: readonly ClosedWorkspaceTab[]): string {
  return JSON.stringify(closed.slice(0, MAX_CLOSED_WORKSPACE_TABS));
}
