import { untrack } from "svelte";
import { availableAppViews, type View } from "$lib/navigation";
import { registerNavigationRouter, type NavigationRouterRegistration } from "$lib/stores/navigation.svelte";
import { activeVaultIdentity, onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import type {
  ScreenPoint,
  TabWindowDrag,
  TabWindowRequest,
  TabWindowTransfer,
  WorkspaceTabWindows,
} from "$lib/windows/tab-windows";
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
  parseClosedWorkspaceTabs,
  parseWorkspaceTabsState,
  relativeWorkspaceTabId,
  rememberClosedWorkspaceTabs,
  reopenWorkspaceTab,
  savedWindowLabels,
  sameTabLocation,
  serializeClosedWorkspaceTabs,
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
  /** Ask to close this window, which closes the app when it is the last one open. */
  requestWindowClose(): void;
}

export type WorkspaceTabDetachBlock = "unsupported" | "last-tab";

const STORAGE_PREFIX = "ganbaru.workspace-tabs.v1";
/** Reopen history of a vault, shared by every window so a tab closed in one can be reopened in any. */
const CLOSED_STORAGE_PREFIX = "ganbaru.workspace-tabs-closed.v1";
/**
 * How long a tab dropped outside its window waits before it opens a window of its own. A drop on another
 * window asks for the tab within this time, and that move wins.
 */
const TEAR_OFF_SETTLE_MS = 250;

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
/** Set once this window closes or hides, so it stops saving tabs it has handed off or remembered as closed. */
let windowReleased = false;
/** Tabs that moved to another window, which a closing window must not remember as closed. */
const departedTabIds = new Set<string>();
/** Tabs on their way to another window, so a second move of the same tab cannot start meanwhile. */
const departingTabIds = new Set<string>();
/** Tabs another window asked for, so a tear-off of the same drag leaves them to that window. */
const requestedTabIds = new Set<string>();
/** Where a tab dropped from another window goes once it arrives, or null to keep it at the end. */
let pendingDrop: { tabId: string; target: { id: string; placement: WorkspaceTabPlacement } | null } | null = null;

/**
 * Every window saves its tabs under its own key per vault. The host window's key has no label so the tabs
 * of a single-window setup keep one stable place.
 */
function storageKey(vaultId: string): string | null {
  if (!connection) return null;
  const windows = connection.windows;
  return windows && !windows.isHost
    ? `${STORAGE_PREFIX}:${vaultId}:${windows.label}`
    : `${STORAGE_PREFIX}:${vaultId}`;
}

function readStoredKey(key: string): WorkspaceTabsState | null {
  try {
    const raw = localStorage.getItem(key);
    return raw ? parseWorkspaceTabsState(JSON.parse(raw) as unknown, availableAppViews()) : null;
  } catch (error) {
    console.warn("Failed to read workspace tabs:", error);
    return null;
  }
}

function readStoredState(vaultId: string | null): WorkspaceTabsState | null {
  const key = vaultId ? storageKey(vaultId) : null;
  return key && typeof localStorage !== "undefined" ? readStoredKey(key) : null;
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

function forgetKey(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch (error) {
    console.warn("Failed to forget workspace tabs:", error);
  }
}

function forgetStoredState(): void {
  const key = storageVaultId ? storageKey(storageVaultId) : null;
  if (key && typeof localStorage !== "undefined") forgetKey(key);
}

function closedStorageKey(vaultId: string): string {
  return `${CLOSED_STORAGE_PREFIX}:${vaultId}`;
}

function readSharedClosed(vaultId: string | null): ClosedWorkspaceTab[] {
  if (!vaultId || typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(closedStorageKey(vaultId));
    return raw ? parseClosedWorkspaceTabs(JSON.parse(raw) as unknown, availableAppViews()) : [];
  } catch (error) {
    console.warn("Failed to read closed workspace tabs:", error);
    return [];
  }
}

function persistClosed(closed: readonly ClosedWorkspaceTab[]): void {
  if (!storageVaultId || typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(closedStorageKey(storageVaultId), serializeClosedWorkspaceTabs(closed));
  } catch (error) {
    console.warn("Failed to save closed workspace tabs:", error);
  }
}

/** Take up the reopen history other windows saved, so the next change to it builds on theirs. */
function syncClosed(): void {
  const closed = readSharedClosed(storageVaultId);
  if (serializeClosedWorkspaceTabs(closed) !== serializeClosedWorkspaceTabs(tabsState.closed)) {
    tabsState = { ...tabsState, closed };
  }
}

function setState(next: WorkspaceTabsState): boolean {
  if (next === tabsState) return false;
  const ids = new Set(next.tabs.map((tab) => tab.id));
  activationOrder = [next.activeTabId, ...activationOrder.filter((id) => id !== next.activeTabId && ids.has(id))];
  if (next.closed !== tabsState.closed) persistClosed(next.closed);
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
    const target = pendingDrop?.tabId === transfer.tab.id ? pendingDrop.target : null;
    pendingDrop = null;
    const inserted = insertWorkspaceTab(tabsState, transfer.tab, "end");
    commitState(target ? moveWorkspaceTab(inserted, transfer.tab.id, target.id, target.placement) : inserted);
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

/**
 * Take over the tabs another window saved, forgetting them there. The host window does this at launch when
 * it saved no tabs of its own, because it was hidden when the app quit, so the app reopens as it was left.
 */
function adoptSavedWindow(vaultId: string | null): WorkspaceTabsState | null {
  if (!vaultId || !connection?.windows?.isHost || typeof localStorage === "undefined") return null;
  const prefix = `${STORAGE_PREFIX}:${vaultId}:`;
  for (const label of savedWindowLabels(storageKeys(), prefix)) {
    const key = `${prefix}${label}`;
    const saved = readStoredKey(key);
    forgetKey(key);
    if (saved) return saved;
  }
  return null;
}

function loadForVault(vaultId: string | null, initialTab: WorkspaceTab | null, adoptSaved = false): void {
  storageVaultId = vaultId;
  const initial = initialTab ? { tabs: [initialTab], activeTabId: initialTab.id, closed: [] } : null;
  const loaded = readStoredState(vaultId)
    ?? (adoptSaved ? adoptSavedWindow(vaultId) : null)
    ?? initial
    ?? defaultWorkspaceTabsState(availableAppViews(), createTabId);
  activationOrder = [];
  setState({ ...loaded, closed: readSharedClosed(vaultId) });
}

/**
 * Start saving a hidden host window's tabs again once it is back on screen. It hid when the user closed it
 * while other windows stayed open, and shows again when the app needs it, for example on a second launch.
 */
function reviveHostWindow(): void {
  if (!windowReleased || !connection?.windows?.isHost) return;
  windowReleased = false;
  persist(tabsState);
}

function connectWindows(host: WorkspaceTabsConnection): () => void {
  const windows = host.windows;
  if (!windows) return () => undefined;
  const onFocus = () => {
    syncClosed();
    reviveHostWindow();
  };
  const onVisibilityChange = () => {
    if (document.visibilityState === "visible") reviveHostWindow();
  };
  const onStorage = (event: StorageEvent) => {
    if (storageVaultId && event.key === closedStorageKey(storageVaultId)) syncClosed();
  };
  window.addEventListener("focus", onFocus);
  window.addEventListener("storage", onStorage);
  document.addEventListener("visibilitychange", onVisibilityChange);
  const unlisten = windows.listen({
    transfer: adoptTransfer,
    release: (release) => releaseTab(release.tabId, release.vaultId),
    request: (request) => {
      void offerRequestedTab(request).catch((error: unknown) => {
        console.error("Failed to move a dragged tab:", error);
      });
    },
  });
  return () => {
    window.removeEventListener("focus", onFocus);
    window.removeEventListener("storage", onStorage);
    document.removeEventListener("visibilitychange", onVisibilityChange);
    unlisten();
  };
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
  loadForVault(activeVaultIdentity(), host.initialTab ?? null, true);
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

/** Close a tab. Closing the last one asks to close this window, which closes the app when it is the last. */
function close(id: string): void {
  const host = connection;
  if (!host) return;
  syncClosed();
  const next = closeWorkspaceTab(tabsState, id);
  if (next === null) host.requestWindowClose();
  else commitState(next);
}

function reopen(closedIndex = 0): void {
  syncClosed();
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

/** Run one move of a tab out of this window, refusing to start a second move of the same tab meanwhile. */
async function depart(id: string, move: (windows: WorkspaceTabWindows) => Promise<void>): Promise<void> {
  const windows = connection?.windows;
  if (!windows || detachBlock(id) || departingTabIds.has(id)) return;
  departingTabIds.add(id);
  try {
    await move(windows);
  } finally {
    departingTabIds.delete(id);
  }
}

/** Move a tab into a new window of its own, opening it at `position` when given. */
function detach(id: string, position: ScreenPoint | null = null): Promise<void> {
  return depart(id, async (windows) => {
    const tab = await prepareDeparture(id);
    if (!tab) return;
    await windows.open(tab, position);
    const next = takeWorkspaceTab(tabsState, id);
    if (next) {
      departedTabIds.add(id);
      commitState(next);
    }
  });
}

/**
 * Move a tab dropped outside every window into a new window there. It waits briefly first, so a drop the
 * engine misreports while another window takes the tab cannot move it twice.
 */
async function tearOff(id: string, position: ScreenPoint | null): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, TEAR_OFF_SETTLE_MS));
  if (departedTabIds.has(id) || requestedTabIds.has(id)) return;
  await detach(id, position);
}

/** What a drag of a tab carries so another window can ask for it, or null when the tab cannot leave. */
function dragPayload(id: string): TabWindowDrag | null {
  const windows = connection?.windows;
  if (!windows || !storageVaultId || detachBlock(id)) return null;
  requestedTabIds.delete(id);
  void connection?.flushPendingEdits?.().catch((error: unknown) => {
    console.error("Failed to save pending edits before a tab drag:", error);
  });
  return { sourceLabel: windows.label, vaultId: storageVaultId, tabId: id };
}

/**
 * Ask the window a tab was dragged from to move it here, next to `target` or at the end. Returns whether
 * the drop was taken: a drag from this window, another vault, or onto a full strip is not.
 */
function receiveDrop(raw: string, target: { id: string; placement: WorkspaceTabPlacement } | null): boolean {
  const windows = connection?.windows;
  const drag = windows?.readDrag(raw) ?? null;
  if (!windows || !drag || drag.vaultId !== storageVaultId || !canAddWorkspaceTab(tabsState)) return false;
  pendingDrop = { tabId: drag.tabId, target };
  void windows.request(drag.sourceLabel, drag.vaultId, drag.tabId).catch((error: unknown) => {
    console.error("Failed to ask for a dragged tab:", error);
  });
  return true;
}

/** Offer a dragged tab to the window it was dropped on, with its live location and saved edits. */
function offerRequestedTab(request: TabWindowRequest): Promise<void> {
  if (request.vaultId !== storageVaultId) return Promise.resolve();
  requestedTabIds.add(request.tabId);
  return depart(request.tabId, async (windows) => {
    const tab = await prepareDeparture(request.tabId);
    if (tab) await windows.offer(request.sourceLabel, request.vaultId, tab);
  });
}

/**
 * Remember the tabs the user closes with this window in the shared reopen history and stop saving its tabs.
 * Call right before the window closes or hides.
 */
function releaseWindow(): void {
  if (!connection?.windows || windowReleased) return;
  syncClosed();
  const tabs = tabsState.tabs
    .filter((tab) => !departedTabIds.has(tab.id))
    .map((tab) => ({ ...tab, location: liveLocation(tab) }));
  setState(rememberClosedWorkspaceTabs(tabsState, tabs));
  windowReleased = true;
  forgetStoredState();
  if (connection.windows.isHost) resetHiddenHostWindow();
}

/**
 * Give the host window fresh tabs as it hides, so it comes back as if newly opened, ready for whatever brings
 * it back, such as a notification that opens an item.
 */
function resetHiddenHostWindow(): void {
  departedTabIds.clear();
  activationOrder = [];
  cancelRestore();
  setState({ ...defaultWorkspaceTabsState(availableAppViews(), createTabId), closed: tabsState.closed });
  void restoreActive();
}

/** Count the other app windows on screen, or zero when this is the only window the tabs know of. */
async function otherOpenWindows(): Promise<number> {
  return (await connection?.windows?.otherOpenWindows()) ?? 0;
}

function storageKeys(): string[] {
  const keys: string[] = [];
  for (let index = 0; index < localStorage.length; index += 1) {
    const key = localStorage.key(index);
    if (key !== null) keys.push(key);
  }
  return keys;
}

/**
 * Reopen the other windows that were open when the app last quit, each with the tabs it saved. Saved
 * tabs that are no longer usable are forgotten instead of opening an empty window. Host window only.
 */
async function restoreWindows(): Promise<void> {
  const windows = connection?.windows;
  if (!windows?.isHost || !storageVaultId || typeof localStorage === "undefined") return;
  const prefix = `${STORAGE_PREFIX}:${storageVaultId}:`;
  for (const label of savedWindowLabels(storageKeys(), prefix)) {
    const key = `${prefix}${label}`;
    if (!readStoredKey(key)) {
      forgetKey(key);
      continue;
    }
    try {
      await windows.reopen(label);
    } catch (error) {
      console.error(`Failed to reopen window ${label}:`, error);
    }
  }
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
    get launcherRequests(): number {
      return launcherRequests;
    },
    connect,
    activate,
    openKind,
    openLocation,
    close,
    closeOthers(id: string): void {
      syncClosed();
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
    tearOff,
    dragPayload,
    receiveDrop,
    releaseWindow,
    otherOpenWindows,
    restoreWindows,
    requestLauncher(): void {
      launcherRequests += 1;
    },
  };
}

export type WorkspaceTabs = ReturnType<typeof getWorkspaceTabs>;
