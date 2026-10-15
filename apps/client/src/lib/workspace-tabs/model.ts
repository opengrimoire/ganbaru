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
  /** Last known project or item name, rendered while the tab is inactive. */
  title: string | null;
  /** Shown in its own window; the main window keeps its place so a quit never loses it. */
  detached: boolean;
}

/** A closed tab kept for reopening at its former position. */
export interface ClosedWorkspaceTab {
  level: WorkspaceTabLevel;
  locked: boolean;
  location: WorkspaceTabLocation;
  title: string | null;
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

/** Return whether a kind has projects or items to point at; a kind without them needs only one tab. */
export function kindHasLocations(kind: View): boolean {
  return ITEM_TYPES_BY_KIND[kind].length > 0;
}

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

export function createWorkspaceTab(
  id: string,
  location: WorkspaceTabLocation,
  options: { level?: WorkspaceTabLevel; locked?: boolean; title?: string | null } = {},
): WorkspaceTab {
  const level = supportedTabLevel(options.level ?? "kind", location);
  return {
    id,
    level,
    locked: level !== "kind" && options.locked === true,
    location,
    title: options.title ?? null,
    detached: false,
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

export function visibleWorkspaceTabs(state: WorkspaceTabsState): WorkspaceTab[] {
  return state.tabs.filter((tab) => !tab.detached);
}

export function activeWorkspaceTab(state: WorkspaceTabsState): WorkspaceTab {
  return state.tabs.find((tab) => tab.id === state.activeTabId)
    ?? visibleWorkspaceTabs(state)[0]
    ?? state.tabs[0];
}

export function activateWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  if (state.activeTabId === id) return state;
  const tab = state.tabs.find((candidate) => candidate.id === id);
  return tab && !tab.detached ? { ...state, activeTabId: id } : state;
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
    && next.detached === current.detached
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
    index,
  };
  return [entry, ...closed].slice(0, MAX_CLOSED_WORKSPACE_TABS);
}

/** Pick the visible tab that takes focus when one closes or leaves: its right neighbor, else its left one. */
function neighborTabId(tabs: readonly WorkspaceTab[], id: string): string | null {
  const visible = tabs.filter((tab) => !tab.detached || tab.id === id);
  const index = visible.findIndex((tab) => tab.id === id);
  const neighbor = visible[index + 1] ?? visible[index - 1];
  return neighbor && neighbor.id !== id ? neighbor.id : null;
}

/**
 * Close a tab and remember it for reopening. Returns null when it is the last visible tab, which the
 * caller turns into the app close confirmation.
 */
export function closeWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState | null {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  const tab = state.tabs[index];
  if (!tab.detached && visibleWorkspaceTabs(state).length <= 1) return null;
  return removeWorkspaceTab(state, id, tab.location);
}

/** Remove a tab even while it is detached, remembering its final location for reopening. */
export function removeWorkspaceTab(
  state: WorkspaceTabsState,
  id: string,
  finalLocation: WorkspaceTabLocation,
): WorkspaceTabsState {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return state;
  const tab = { ...state.tabs[index], location: finalLocation };
  const activeTabId = state.activeTabId === id
    ? neighborTabId(state.tabs, id) ?? state.activeTabId
    : state.activeTabId;
  const tabs = state.tabs.filter((candidate) => candidate.id !== id);
  if (tabs.length === 0) return state;
  return { tabs, activeTabId, closed: rememberClosed(state.closed, tab, index) };
}

/** Close every visible tab except one, keeping detached tabs in their windows. */
export function closeOtherWorkspaceTabs(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  if (!state.tabs.some((tab) => tab.id === id && !tab.detached)) return state;
  let closed = state.closed;
  state.tabs.forEach((tab, index) => {
    if (tab.id !== id && !tab.detached) closed = rememberClosed(closed, tab, index);
  });
  return {
    tabs: state.tabs.filter((tab) => tab.id === id || tab.detached),
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
  });
  const tabs = [...state.tabs];
  tabs.splice(Math.min(entry.index, tabs.length), 0, tab);
  const closed = state.closed.filter((_, index) => index !== closedIndex);
  return { state: { tabs, activeTabId: tab.id, closed }, tab };
}

/** Return the visible tab next to the active one, wrapping around the ends. */
export function relativeWorkspaceTabId(state: WorkspaceTabsState, direction: -1 | 1): string | null {
  const visible = visibleWorkspaceTabs(state);
  if (visible.length === 0) return null;
  const index = Math.max(0, visible.findIndex((tab) => tab.id === state.activeTabId));
  return visible[(index + direction + visible.length) % visible.length].id;
}

/** Return the visible tab at a zero-based position; the ninth position always means the last tab. */
export function workspaceTabIdAtPosition(state: WorkspaceTabsState, index: number): string | null {
  const visible = visibleWorkspaceTabs(state);
  if (index === 8) return visible.at(-1)?.id ?? null;
  return visible[index]?.id ?? null;
}

/** Mark a tab as shown in its own window, moving focus to a neighbor when it was active. */
export function detachWorkspaceTab(state: WorkspaceTabsState, id: string): WorkspaceTabsState {
  const tab = state.tabs.find((candidate) => candidate.id === id);
  if (!tab || tab.detached) return state;
  const activeTabId = state.activeTabId === id ? neighborTabId(state.tabs, id) : state.activeTabId;
  if (!activeTabId) return state;
  return { ...updateWorkspaceTab(state, id, { detached: true }), activeTabId };
}

/** Show again every detached tab whose kind no longer has its own window. */
export function attachWorkspaceTabs(
  state: WorkspaceTabsState,
  isKindDetached: (kind: View) => boolean,
): WorkspaceTabsState {
  if (!state.tabs.some((tab) => tab.detached && !isKindDetached(tab.location.kind))) return state;
  const tabs = state.tabs.map((tab) => (
    tab.detached && !isKindDetached(tab.location.kind) ? { ...tab, detached: false } : tab
  ));
  return { ...state, tabs };
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

function parseTab(value: unknown, availableKinds: readonly View[]): WorkspaceTab | null {
  if (!isRecord(value)) return null;
  const id = boundedId(value.id);
  const level = parseLevel(value.level);
  const location = parseWorkspaceTabLocation(value.location);
  if (!id || !level || !location || !availableKinds.includes(location.kind)) return null;
  return createWorkspaceTab(id, location, {
    level,
    locked: value.locked === true,
    title: parseTitle(value.title),
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
    index,
  };
}

/**
 * Validate persisted tabs. Detached flags are dropped because detached windows never outlive the app.
 * Returns null when nothing usable remains.
 */
export function parseWorkspaceTabsState(
  value: unknown,
  availableKinds: readonly View[],
): WorkspaceTabsState | null {
  if (!isRecord(value) || !Array.isArray(value.tabs)) return null;
  const seen = new Set<string>();
  const tabs = value.tabs.slice(0, MAX_WORKSPACE_TABS).flatMap((entry) => {
    const tab = parseTab(entry, availableKinds);
    if (!tab || seen.has(tab.id)) return [];
    seen.add(tab.id);
    return [tab];
  });
  if (tabs.length === 0) return null;
  const closed = Array.isArray(value.closed)
    ? value.closed.slice(0, MAX_CLOSED_WORKSPACE_TABS).flatMap((entry) => {
      const parsed = parseClosedTab(entry, availableKinds);
      return parsed ? [parsed] : [];
    })
    : [];
  const activeTabId = tabs.find((tab) => tab.id === value.activeTabId)?.id ?? tabs[0].id;
  return { tabs, activeTabId, closed };
}

/** Return the persisted shape, without the runtime-only detached flag. */
export function serializeWorkspaceTabsState(state: WorkspaceTabsState): string {
  return JSON.stringify({
    tabs: state.tabs.map(({ detached: _detached, ...tab }) => tab),
    activeTabId: state.activeTabId,
    closed: state.closed,
  });
}
