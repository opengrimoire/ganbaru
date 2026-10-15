import { untrack } from "svelte";
import { availableAppViews, type View } from "$lib/navigation";
import { registerNavigationRouter, type NavigationRouterRegistration } from "$lib/stores/navigation.svelte";
import { activeVaultIdentity, onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import type { TabWindowTransfer, WorkspaceTabWindows } from "$lib/windows/tab-windows";
import {
  activateWorkspaceTab,
  activeWorkspaceTab,
  canAddWorkspaceTab,
  closeOtherWorkspaceTabs,
  closeWorkspaceTab,
  createWorkspaceTab,
  defaultWorkspaceTabsState,
  insertWorkspaceTab,
  isWithinTabScope,
  kindLocation,
  moveWorkspaceTab,
  parseWorkspaceTabsState,
  relativeWorkspaceTabId,
  rememberClosedWorkspaceTabs,
  reopenWorkspaceTab,
  sameTabLocation,
  serializeWorkspaceTabsState,
  tabMatchesTarget,
  takeWorkspaceTab,
  updateWorkspaceTab,
  workspaceTabIdAtPosition,
  workspaceTabLockLevel,
  type ClosedWorkspaceTab,
  type WorkspaceTab,
  type WorkspaceTabLevel,
  type WorkspaceTabLocation,
  type WorkspaceTabPlacement,
  type WorkspaceTabsState,
} from "$lib/workspace-tabs/model";

/** Reads and restores the singleton view stores that a tab location describes. */
export interface WorkspaceLocationAdapter {
  /** Where the stores behind a kind are now, or null while they are not loaded, so nothing is recorded yet. */
  read(kind: View): WorkspaceTabLocation | null;
  /** Name of the item a location points at, or null when it has none or it is not loaded. */
  itemTitle(location: WorkspaceTabLocation): string | null;
  /** Name of a project, or null when it is not loaded. */
  projectName(projectId: string): string | null;
  /** Restore a location, stopping between steps once `isCurrent` turns false. */
  apply(location: WorkspaceTabLocation, isCurrent: () => boolean): Promise<void>;
}

export interface WorkspaceTabsConnection {
  adapter: WorkspaceLocationAdapter;
  /** Windows the tabs can move between. Without them every tab stays in the one window. */
  windows?: WorkspaceTabWindows;
  /** Tab a new window opened with, used when the window has no saved tabs yet. */
  initialTab?: WorkspaceTab | null;
  /** Save edits that are still pending, so a tab that moves shows them in its new window. */
  flushPendingEdits?(): Promise<void>;
  /** Ask to close the app from the main window, or this window from any other. */
  requestWindowClose(): void;
}

export type WorkspaceTabDetachBlock = "unsupported" | "last-tab";

const STORAGE_PREFIX = "ganbaru.workspace-tabs.v1";

function createTabId(): string {
  return crypto.randomUUID();
}

let tabsState = $state.raw<WorkspaceTabsState>(defaultWorkspaceTabsState(availableAppViews(), createTabId));
let restoring = $state(false);
let launcherRequests = $state(0);
let restoreSequence = 0;
let activationOrder: string[] = [];
let connection: WorkspaceTabsConnection | null = null;
/** The last tab opened in the background and the tab it was opened from, so a run of them keeps its order. */
let lastBackgroundOpen: { fromId: string; tabId: string } | null = null;
let navigation: NavigationRouterRegistration | null = null;
let storageVaultId: string | null = null;
/** Set once this window is closing, so it stops saving tabs it has handed off or reported as closed. */
let windowReleased = false;
/** Tabs that moved to another window, which a closing window must not report as closed. */
const departedTabIds = new Set<string>();

/** Whether this is the main window. A window without window operations is the only one, so it is main. */
function isMainWindow(): boolean {
  return connection !== null && (connection.windows?.isMain ?? true);
}

/**
 * Every window saves its tabs under its own key per vault. The main window's key has no label so the
 * tabs of a single-window setup keep one stable place.
 */
function storageKey(vaultId: string): string | null {
  if (!connection) return null;
  const windows = connection.windows;
  return windows && !windows.isMain
    ? `${STORAGE_PREFIX}:${vaultId}:${windows.label}`
    : `${STORAGE_PREFIX}:${vaultId}`;
}

function readStoredState(vaultId: string | null): WorkspaceTabsState | null {
  const key = vaultId ? storageKey(vaultId) : null;
  if (!key || typeof localStorage === "undefined") return null;
  try {
    const raw = localStorage.getItem(key);
    return raw ? parseWorkspaceTabsState(JSON.parse(raw) as unknown, availableAppViews()) : null;
  } catch (error) {
    console.warn("Failed to read workspace tabs:", error);
    return null;
  }
}

function persist(state: WorkspaceTabsState): void {
  const key = storageVaultId ? storageKey(storageVaultId) : null;
  if (windowReleased || !key || typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(key, serializeWorkspaceTabsState(state));
  } catch (error) {
    console.warn("Failed to save workspace tabs:", error);
  }
}

function forgetStoredState(): void {
  const key = storageVaultId ? storageKey(storageVaultId) : null;
  if (!key || typeof localStorage === "undefined") return;
  try {
    localStorage.removeItem(key);
  } catch (error) {
    console.warn("Failed to forget workspace tabs:", error);
  }
}

function setState(next: WorkspaceTabsState): boolean {
  if (next === tabsState) return false;
  const ids = new Set(next.tabs.map((tab) => tab.id));
  activationOrder = [next.activeTabId, ...activationOrder.filter((id) => id !== next.activeTabId && ids.has(id))];
  tabsState = next;
  persist(next);
  return true;
}

function currentTab(): WorkspaceTab {
  return activeWorkspaceTab(tabsState);
}

/**
 * Current names for a tab's item and project. Only item tabs carry an item name, and Calendar and tabs
 * without a project carry no project name. A name that is not loaded is left out so the cached one stays.
 */
function tabNames(
  adapter: WorkspaceLocationAdapter,
  tab: WorkspaceTab,
): Partial<Pick<WorkspaceTab, "title" | "projectTitle">> {
  const names: Partial<Pick<WorkspaceTab, "title" | "projectTitle">> = {};
  const title = tab.level === "item" ? adapter.itemTitle(tab.location) : null;
  if (tab.level !== "item" || title !== null) names.title = title;
  const projectId = tab.location.kind === "calendar" ? null : tab.location.projectId;
  const projectTitle = projectId ? adapter.projectName(projectId) : null;
  if (!projectId || projectTitle !== null) names.projectTitle = projectTitle;
  return names;
}

/** Stop any pending restore so the recording effect adopts what the stores show now. */
function cancelRestore(): void {
  restoreSequence += 1;
  restoring = false;
}

async function restoreActive(): Promise<void> {
  const host = connection;
  if (!host) return;
  const tab = currentTab();
  const sequence = ++restoreSequence;
  restoring = true;
  navigation?.commit(tab.location.kind);
  const isCurrent = () => sequence === restoreSequence && tabsState.activeTabId === tab.id;
  try {
    await host.adapter.apply(tab.location, isCurrent);
  } catch (error) {
    console.error("Failed to restore workspace tab:", error);
  } finally {
    if (sequence === restoreSequence) {
      const shown = host.adapter.read(tab.location.kind);
      if (shown) unlockIfUnreachable(tab.id, shown);
      restoring = false;
    }
  }
}

/**
 * Drop the lock of a tab whose project or item could not be shown, for example because it was deleted, so
 * the tab follows the stores again instead of opening a new tab every time it is selected.
 */
function unlockIfUnreachable(tabId: string, shown: WorkspaceTabLocation): void {
  const tab = tabsState.tabs.find((candidate) => candidate.id === tabId);
  if (tab?.locked && !isWithinTabScope(tab, shown)) setState(updateWorkspaceTab(tabsState, tabId, { locked: false }));
}

function commitState(next: WorkspaceTabsState): void {
  const previousActive = tabsState.activeTabId;
  if (setState(next) && next.activeTabId !== previousActive) void restoreActive();
}

/**
 * Adopt what the stores show into the active tab. A locked tab keeps its scope: leaving it lands the new
 * location in another tab whose scope holds it, or in a new tab right after the locked one. A location with
 * neither project nor item is a passing reset, not a destination, so it never leaves a locked tab.
 */
function recordLocation(tab: WorkspaceTab, location: WorkspaceTabLocation): void {
  if (isWithinTabScope(tab, location)) {
    setState(updateWorkspaceTab(tabsState, tab.id, { location }));
    return;
  }
  if (!meaningfulLocation(location)) return;
  const target = tabForLocation(location);
  cancelRestore();
  setState(target
    ? activateWorkspaceTab(tabsState, target.id)
    : insertWorkspaceTab(tabsState, createWorkspaceTab(createTabId(), location), "after-active"));
}

function mostRecent(candidates: readonly WorkspaceTab[]): WorkspaceTab | null {
  for (const id of activationOrder) {
    const tab = candidates.find((candidate) => candidate.id === id);
    if (tab) return tab;
  }
  return candidates[0] ?? null;
}

/** Pick the tab a location should land in: an exact match, else one on the same project, else the most recent. */
function tabForLocation(location: WorkspaceTabLocation): WorkspaceTab | null {
  const candidates = tabsState.tabs.filter((tab) => isWithinTabScope(tab, location));
  return candidates.find((tab) => sameTabLocation(tab.location, location))
    ?? mostRecent(candidates.filter((tab) => location.projectId !== null && tab.location.projectId === location.projectId))
    ?? mostRecent(candidates);
}

function meaningfulLocation(location: WorkspaceTabLocation): WorkspaceTabLocation | null {
  return location.projectId || location.item ? location : null;
}

/**
 * Land a navigation request in a tab: the active tab when it already shows the kind, else a tab whose scope
 * holds the location (an exact match first, then the same project, then the most recent), else a new tab. The views already changed
 * the stores, so the chosen tab adopts them instead of restoring its own location.
 */
function routeNavigation(kind: View): boolean {
  const host = connection;
  if (!host) return false;
  if (currentTab().location.kind === kind) return navigation?.commit(kind) ?? false;
  const location = host.adapter.read(kind) ?? kindLocation(kind);
  const target = tabForLocation(location);
  const next = target
    ? activateWorkspaceTab(tabsState, target.id)
    : insertWorkspaceTab(tabsState, createWorkspaceTab(createTabId(), location), "after-active");
  if (currentTab().location.kind !== kind && next === tabsState) return false;
  cancelRestore();
  setState(next);
  return navigation?.commit(kind) ?? false;
}

/** Where a tab is now: the live stores for the shown tab, else its recorded location. */
function liveLocation(tab: WorkspaceTab): WorkspaceTabLocation {
  if (tab.id !== tabsState.activeTabId || restoring) return tab.location;
  return connection?.adapter.read(tab.location.kind) ?? tab.location;
}

/**
 * Adopt a tab another window offers, then tell that window to drop its copy. A full strip leaves the tab
 * where it is. A tab this window already holds is only confirmed, so a repeated offer cannot duplicate it.
 */
function adoptTransfer(transfer: TabWindowTransfer): void {
  const windows = connection?.windows;
  if (!windows || transfer.vaultId !== storageVaultId) return;
  const existing = tabsState.tabs.find((tab) => tab.id === transfer.tab.id);
  if (existing) {
    commitState(activateWorkspaceTab(tabsState, existing.id));
  } else if (canAddWorkspaceTab(tabsState)) {
    commitState(insertWorkspaceTab(tabsState, transfer.tab, "end"));
  } else {
    return;
  }
  void windows.release(transfer.sourceLabel, transfer.vaultId, transfer.tab.id).catch((error: unknown) => {
    console.error("Failed to confirm a moved tab:", error);
  });
}

/** Drop a tab that now lives in another window, closing this window when it was the last one. */
function releaseTab(tabId: string, vaultId: string): void {
  const host = connection;
  if (!host || vaultId !== storageVaultId || !tabsState.tabs.some((tab) => tab.id === tabId)) return;
  departedTabIds.add(tabId);
  const next = takeWorkspaceTab(tabsState, tabId);
  if (next) commitState(next);
  else host.requestWindowClose();
}

function loadForVault(vaultId: string | null, initialTab: WorkspaceTab | null): void {
  storageVaultId = vaultId;
  const initial = initialTab ? { tabs: [initialTab], activeTabId: initialTab.id, closed: [] } : null;
  const loaded = readStoredState(vaultId) ?? initial ?? defaultWorkspaceTabsState(availableAppViews(), createTabId);
  activationOrder = [];
  setState(loaded);
}

function connectWindows(host: WorkspaceTabsConnection): () => void {
  if (!host.windows) return () => undefined;
  return host.windows.listen({
    transfer: adoptTransfer,
    release: (release) => releaseTab(release.tabId, release.vaultId),
    closed: (closed) => {
      if (closed.vaultId === storageVaultId) setState(rememberClosedWorkspaceTabs(tabsState, closed.tabs));
    },
  });
}

/**
 * Bind the tabs to this window. Call during the shell component's initialization so the recording
 * effect lives and dies with it.
 */
function connect(host: WorkspaceTabsConnection): void {
  connection = host;
  windowReleased = false;
  departedTabIds.clear();
  navigation = registerNavigationRouter(routeNavigation);
  loadForVault(activeVaultIdentity(), host.initialTab ?? null);
  const cleanups = [connectWindows(host)];
  cleanups.push(onActiveVaultIdentityChange((_previous, next) => {
    if (next === storageVaultId) return;
    loadForVault(next, null);
    void restoreActive();
  }));
  void restoreActive();

  $effect(() => {
    if (restoring) return;
    const tab = currentTab();
    const location = host.adapter.read(tab.location.kind);
    if (location) untrack(() => recordLocation(tab, location));
  });

  // Keep cached names current, including renames of what inactive tabs point at. A name that is not
  // loaded keeps the cached one.
  $effect(() => {
    const names = tabsState.tabs.map((tab) => ({ id: tab.id, ...tabNames(host.adapter, tab) }));
    untrack(() => {
      let next = tabsState;
      for (const { id, ...patch } of names) next = updateWorkspaceTab(next, id, patch);
      setState(next);
    });
  });

  $effect(() => () => {
    cleanups.forEach((cleanup) => cleanup());
    navigation?.unregister();
    navigation = null;
    if (connection === host) connection = null;
  });
}

function activate(id: string): void {
  if (id === tabsState.activeTabId) return;
  commitState(activateWorkspaceTab(tabsState, id));
}

/** Open a new tab of a kind from the launcher. */
function openKind(kind: View): void {
  const host = connection;
  if (!host) return;
  const location = host.adapter.read(kind) ?? kindLocation(kind);
  const tab = createWorkspaceTab(createTabId(), location);
  commitState(insertWorkspaceTab(tabsState, { ...tab, ...tabNames(host.adapter, tab) }, "end"));
}

export interface OpenWorkspaceLocationOptions {
  /** Leave the current tab in front. */
  background?: boolean;
}

/** Open a project or item in a tab of its own. A tab that already shows it at that level is focused instead. */
function openLocation(
  level: WorkspaceTabLevel,
  location: WorkspaceTabLocation,
  options: OpenWorkspaceLocationOptions = {},
): void {
  const host = connection;
  if (!host) return;
  const created = createWorkspaceTab(createTabId(), location, { level });
  const tab = { ...created, ...tabNames(host.adapter, created) };
  const existing = tabsState.tabs.find((candidate) => tabMatchesTarget(candidate, tab.level, location));
  if (existing) {
    activate(existing.id);
    return;
  }
  const activeId = tabsState.activeTabId;
  if (options.background) {
    const position = lastBackgroundOpen?.fromId === activeId ? { afterId: lastBackgroundOpen.tabId } : "after-active";
    const next = insertWorkspaceTab(tabsState, tab, position, false);
    if (setState(next)) lastBackgroundOpen = { fromId: activeId, tabId: tab.id };
    return;
  }
  commitState(insertWorkspaceTab(tabsState, tab, "after-active"));
}

/** Close a tab. Closing the last one asks to close the app, or this window when it is not the main one. */
function close(id: string): void {
  const host = connection;
  if (!host) return;
  const next = closeWorkspaceTab(tabsState, id);
  if (next === null) host.requestWindowClose();
  else commitState(next);
}

function reopen(closedIndex = 0): void {
  const reopened = connection && reopenWorkspaceTab(tabsState, createTabId, closedIndex);
  if (reopened) commitState(reopened.state);
}

/** Lock a tab to the project or item it shows now, or let it follow navigation again. */
function setLocked(id: string, locked: boolean): void {
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!tab) return;
  const level = locked ? workspaceTabLockLevel(tab) : tab.level;
  if (level) setState(updateWorkspaceTab(tabsState, id, { level, locked }));
}

function detachBlock(id: string): WorkspaceTabDetachBlock | null {
  if (!connection?.windows || !tabsState.tabs.some((tab) => tab.id === id)) return "unsupported";
  return tabsState.tabs.length <= 1 ? "last-tab" : null;
}

/** Take a tab out of this window with its live location, after saving its pending edits. */
async function prepareDeparture(id: string): Promise<WorkspaceTab | null> {
  const host = connection;
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!host || !tab) return null;
  await host.flushPendingEdits?.();
  const location = liveLocation(tab);
  setState(updateWorkspaceTab(tabsState, id, { location }));
  return tabsState.tabs.find((candidate) => candidate.id === id) ?? null;
}

/** Move a tab into a new window of its own. */
async function detach(id: string): Promise<void> {
  const windows = connection?.windows;
  if (!windows || detachBlock(id)) return;
  const tab = await prepareDeparture(id);
  if (!tab) return;
  await windows.open(tab);
  const next = takeWorkspaceTab(tabsState, id);
  if (next) {
    departedTabIds.add(id);
    commitState(next);
  }
}

/** Offer a tab to the main window, which adopts it and then releases it from this one. */
async function moveToMainWindow(id: string): Promise<void> {
  const windows = connection?.windows;
  if (!windows || windows.isMain || !storageVaultId) return;
  const vaultId = storageVaultId;
  const tab = await prepareDeparture(id);
  if (!tab) return;
  await windows.offer(windows.mainLabel, vaultId, tab);
  await windows.focusMain();
}

/**
 * Stop saving this window's tabs and report the ones the user closed with it to the main window, so they
 * can be reopened there. Call right before a window other than the main one closes.
 */
async function releaseWindow(): Promise<void> {
  const windows = connection?.windows;
  if (!windows || windows.isMain || windowReleased) return;
  windowReleased = true;
  forgetStoredState();
  const tabs = tabsState.tabs
    .filter((tab) => !departedTabIds.has(tab.id))
    .map((tab) => ({ ...tab, location: liveLocation(tab) }));
  if (!storageVaultId || tabs.length === 0) return;
  await windows.reportClosed(storageVaultId, tabs);
}

export function getWorkspaceTabs() {
  return {
    get tabs(): readonly WorkspaceTab[] {
      return tabsState.tabs;
    },
    get activeTabId(): string {
      return tabsState.activeTabId;
    },
    get activeTab(): WorkspaceTab {
      return currentTab();
    },
    get closed(): readonly ClosedWorkspaceTab[] {
      return tabsState.closed;
    },
    /** Whether this window shows workspace tabs at all. */
    get canOpenTabs(): boolean {
      return connection !== null;
    },
    /** Whether this window is the main one, which owns the app and outlives the others. */
    get isMainWindow(): boolean {
      return isMainWindow();
    },
    get launcherRequests(): number {
      return launcherRequests;
    },
    connect,
    activate,
    openKind,
    openLocation,
    close,
    closeOthers(id: string): void {
      commitState(closeOtherWorkspaceTabs(tabsState, id));
    },
    reopen,
    setLocked,
    move(id: string, targetId: string, placement: WorkspaceTabPlacement): void {
      setState(moveWorkspaceTab(tabsState, id, targetId, placement));
    },
    activateRelative(direction: -1 | 1): void {
      const id = relativeWorkspaceTabId(tabsState, direction);
      if (id) activate(id);
    },
    activateAt(index: number): void {
      const id = workspaceTabIdAtPosition(tabsState, index);
      if (id) activate(id);
    },
    detachBlock,
    detach,
    moveToMainWindow,
    releaseWindow,
    requestLauncher(): void {
      launcherRequests += 1;
    },
  };
}

export type WorkspaceTabs = ReturnType<typeof getWorkspaceTabs>;
