import { untrack } from "svelte";
import { SvelteSet } from "svelte/reactivity";
import { availableAppViews, type View } from "$lib/navigation";
import { registerNavigationRouter, type NavigationRouterRegistration } from "$lib/stores/navigation.svelte";
import { activeVaultIdentity, onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import type {
  ScreenPoint,
  TabDragGrab,
  TabDragOutcome,
  TabWindowClaim,
  TabWindowDrag,
  TabWindowDragEnded,
  TabWindowTransfer,
  WorkspaceTabWindows,
} from "$lib/windows/tab-windows";
import {
  activateWorkspaceTab,
  activeWorkspaceTab,
  canAddWorkspaceTab,
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
  resizeWorkspaceSplit,
  savedWindowLabels,
  sameTabLocation,
  serializeClosedWorkspaceTabs,
  serializeWorkspaceTabsState,
  setWorkspaceSplitOrientation,
  showWorkspaceTabNeighbor,
  splitWorkspaceTabs,
  swapWorkspaceSplit,
  tabMatchesTarget,
  takeWorkspaceTab,
  unsplitWorkspaceTab,
  updateWorkspaceTab,
  workspaceSplitBlock,
  workspaceSplitPartnerId,
  workspaceTabIdAtPosition,
  workspaceTabLockLevel,
  workspaceTabSplit,
  type ClosedWorkspaceTab,
  type WorkspaceSplitBlock,
  type WorkspaceSplitOrientation,
  type WorkspaceTab,
  type WorkspaceTabLevel,
  type WorkspaceTabLocation,
  type WorkspaceTabPlacement,
  type WorkspaceTabSplit,
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
  /** The view that shows a location, such as Notes for a project view that hosts Notes. */
  paneView(location: WorkspaceTabLocation): View;
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

/**
 * A tab drag under way in this vault, started in this window or another. Every window draws the tab while the
 * drag passes over it, and whichever window holds the tab moves it along its strip.
 */
export interface LiveTabDrag extends TabDragGrab {
  /** Window the drag started in, or null when this window cannot share tabs with others. */
  sourceLabel: string | null;
  tab: WorkspaceTab;
}

/** How a tab drag ended, as each window hears of it once. */
export interface TabDragEnd extends TabDragOutcome {
  tabId: string;
}

const STORAGE_PREFIX = "ganbaru.workspace-tabs.v1";
/** Reopen history of a vault, shared by every window so a tab closed in one can be reopened in any. */
const CLOSED_STORAGE_PREFIX = "ganbaru.workspace-tabs-closed.v1";
/**
 * How long a tab dropped outside its window waits before it opens a window of its own. A drop on another
 * window asks for the tab within this time, and that move wins.
 */
const TEAR_OFF_SETTLE_MS = 250;
/**
 * How long a tab dropped on another window keeps its slot there while it has not arrived, and stays hidden in
 * the window it left while it has not gone.
 */
export const TAB_ARRIVAL_TIMEOUT_MS = 3000;
/** How often a window asks again for a tab dragged over its strip, until the window holding it lets it go. */
const CLAIM_INTERVAL_MS = 150;

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
/**
 * Tabs that moved to another window, which a closing window must not remember as closed. A window's last tab
 * stays in its state when it leaves, so this also tells that the window no longer holds it.
 */
const departedTabIds = new SvelteSet<string>();
/** Tabs on their way to another window, so a second move of the same tab cannot start meanwhile. */
const departingTabIds = new Set<string>();
/** Tabs another window asked for, so a tear-off of the same drag leaves them to that window. */
const requestedTabIds = new Set<string>();
/**
 * Where a tab this window asked for goes once it arrives, or null to keep it at the end, and whether it was
 * dropped here, so it is still expected after its drag ends.
 */
let pendingDrop: {
  tabId: string;
  target: { id: string; placement: WorkspaceTabPlacement } | null;
  dropped: boolean;
} | null = null;
/** The tab drag under way in this vault, from this window or another, until it ends. */
let liveDrag = $state.raw<LiveTabDrag | null>(null);
/** Tab whose drag started in this window, whose drag source must stay on screen until the drag ends. */
let nativeDragTabId: string | null = null;
/** This window's last tab left during a drag it started, so it closes once that drag ends. */
let closeAfterDrag = false;
/** When this window last asked for a dragged tab, so it asks again only now and then. */
let lastClaim: { tabId: string; at: number } | null = null;
let arrivalTimer: ReturnType<typeof setTimeout> | null = null;
/** What this window does when a drag ends elsewhere. */
let dragEndListener: ((end: TabDragEnd) => void) | null = null;

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

/** The tab shown beside the active one in its split, or null. */
function partnerTab(): WorkspaceTab | null {
  const id = workspaceSplitPartnerId(tabsState, tabsState.activeTabId);
  return id ? tabsState.tabs.find((tab) => tab.id === id) ?? null : null;
}

/** Whether a tab is on screen: the active tab or the tab beside it. */
function isShown(id: string): boolean {
  return id === tabsState.activeTabId || id === workspaceSplitPartnerId(tabsState, tabsState.activeTabId);
}

/**
 * Whether restoring a split partner can show where it was: Calendar always can, and any other view only on the
 * project the active tab shows, because the views share the selected project.
 */
function partnerFollowsActive(active: WorkspaceTab, partner: WorkspaceTab): boolean {
  const { kind, projectId } = partner.location;
  return kind === "calendar" || active.location.kind === "calendar" || projectId === active.location.projectId;
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

/**
 * Show the active tab and the tab beside it. The partner goes first, so the active tab's own location wins
 * wherever the two touch the same stores. A partner on another project than the active tab is not restored
 * and shows the active tab's project instead.
 */
async function restoreActive(): Promise<void> {
  const host = connection;
  if (!host) return;
  const tab = currentTab();
  const partner = partnerTab();
  const restorePartner = partner !== null && partnerFollowsActive(tab, partner);
  const sequence = ++restoreSequence;
  restoring = true;
  navigation?.commit(tab.location.kind);
  const isCurrent = () => sequence === restoreSequence && tabsState.activeTabId === tab.id;
  const partnerIsCurrent = () => isCurrent() && partnerTab()?.id === partner?.id;
  try {
    if (partner && restorePartner) await host.adapter.apply(partner.location, partnerIsCurrent);
    if (isCurrent()) await host.adapter.apply(tab.location, isCurrent);
  } catch (error) {
    console.error("Failed to restore workspace tab:", error);
  } finally {
    if (sequence === restoreSequence) {
      const shown = host.adapter.read(tab.location.kind);
      if (shown) unlockIfUnreachable(tab.id, shown);
      const partnerShown = partner && restorePartner && partnerIsCurrent()
        ? host.adapter.read(partner.location.kind)
        : null;
      if (partner && partnerShown) unlockIfUnreachable(partner.id, partnerShown);
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

/**
 * Adopt what the stores show into the tab beside the active one. The split ends when that tab can no longer
 * show beside it: its lock no longer holds what it shows, or it now shows the same view as the active tab.
 */
function recordPartnerLocation(partnerId: string, location: WorkspaceTabLocation): void {
  const host = connection;
  const partner = partnerTab();
  if (!host || partner?.id !== partnerId) return;
  if (!isWithinTabScope(partner, location)) {
    if (meaningfulLocation(location)) setState(unsplitWorkspaceTab(tabsState, partnerId));
    return;
  }
  if (host.adapter.paneView(location) === host.adapter.paneView(currentTab().location)) {
    setState(unsplitWorkspaceTab(tabsState, partnerId));
    return;
  }
  setState(updateWorkspaceTab(tabsState, partnerId, { location }));
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
  const partner = partnerTab();
  if (partner?.location.kind === kind) {
    focusPane(partner.id);
    return true;
  }
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

/** Where a tab is now: the live stores for a tab on screen, else its recorded location. */
function liveLocation(tab: WorkspaceTab): WorkspaceTabLocation {
  if (!isShown(tab.id) || restoring) return tab.location;
  return connection?.adapter.read(tab.location.kind) ?? tab.location;
}

/** Whether this window holds a tab: it is in its strip and has not moved to another window. */
function holds(tabId: string): boolean {
  return tabsState.tabs.some((tab) => tab.id === tabId) && !departedTabIds.has(tabId);
}

/**
 * Adopt a tab another window offers, shown at once, then tell that window to drop its copy. A full strip
 * leaves the tab where it is. A tab this window already holds is only confirmed, so a repeated offer cannot
 * duplicate it.
 */
function adoptTransfer(transfer: TabWindowTransfer): void {
  const windows = connection?.windows;
  const id = transfer.tab.id;
  if (!windows || transfer.vaultId !== storageVaultId) return;
  const existing = tabsState.tabs.find((tab) => tab.id === id);
  const target = pendingDrop?.tabId === id ? pendingDrop.target : null;
  if (existing) {
    departedTabIds.delete(id);
    commitState(activateWorkspaceTab(tabsState, existing.id));
  } else if (canAddWorkspaceTab(tabsState)) {
    departedTabIds.delete(id);
    const inserted = insertWorkspaceTab(tabsState, transfer.tab, "end");
    commitState(target ? moveWorkspaceTab(inserted, id, target.id, target.placement) : inserted);
  } else {
    return;
  }
  requestedTabIds.delete(id);
  if (pendingDrop?.tabId === id) pendingDrop = null;
  if (lastClaim?.tabId === id) lastClaim = null;
  stopAwaitingArrival();
  void windows.release(transfer.sourceLabel, transfer.vaultId, id).catch((error: unknown) => {
    console.error("Failed to confirm a moved tab:", error);
  });
}

/**
 * Drop a tab that now lives in another window, closing this window when it was the last one. A window whose
 * last tab leaves during a drag it started only leaves the screen until the drag ends, because closing it
 * would end the drag.
 */
function releaseTab(tabId: string, vaultId: string): void {
  const host = connection;
  if (!host || vaultId !== storageVaultId || !holds(tabId)) return;
  departedTabIds.add(tabId);
  const next = takeWorkspaceTab(tabsState, tabId);
  if (next) {
    commitState(next);
  } else if (nativeDragTabId !== null && host.windows) {
    closeAfterDrag = true;
    void host.windows.minimize().catch((error: unknown) => {
      console.error("Failed to minimize a window whose last tab left:", error);
    });
  } else {
    host.requestWindowClose();
  }
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
  const initial = initialTab ? { tabs: [initialTab], activeTabId: initialTab.id, closed: [], splits: [] } : null;
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
    claim: (claim) => {
      void offerClaimedTab(claim).catch((error: unknown) => {
        console.error("Failed to move a dragged tab:", error);
      });
    },
    dragStarted: (started) => {
      if (started.vaultId !== storageVaultId) return;
      requestedTabIds.delete(started.tab.id);
      liveDrag = started;
    },
    dragEnded: (ended) => {
      if (ended.vaultId === storageVaultId) concludeDrag(ended);
    },
    raised: reviveHostWindow,
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
    resetDrag();
    loadForVault(next, null);
    void restoreActive();
  }));
  void restoreActive();

  $effect(() => {
    if (restoring) return;
    const tab = currentTab();
    const partner = partnerTab();
    const location = host.adapter.read(tab.location.kind);
    const partnerLocation = partner ? host.adapter.read(partner.location.kind) : null;
    untrack(() => {
      if (location) recordLocation(tab, location);
      if (partner && partnerLocation) recordPartnerLocation(partner.id, partnerLocation);
    });
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
    resetDrag();
    navigation?.unregister();
    navigation = null;
    if (connection === host) connection = null;
  });
}

function activate(id: string): void {
  if (id === tabsState.activeTabId) return;
  if (id === partnerTab()?.id) focusPane(id);
  else commitState(activateWorkspaceTab(tabsState, id));
}

/**
 * Make the tab beside the active one active. Both already show, so nothing is restored and whatever the user
 * started in that pane carries on.
 */
function focusPane(id: string): void {
  const tab = partnerTab();
  if (tab?.id !== id) return;
  cancelRestore();
  setState(activateWorkspaceTab(tabsState, id));
  navigation?.commit(tab.location.kind);
}

/** Why a tab cannot show beside the active tab, or null when it can. */
function splitBlock(id: string): WorkspaceSplitBlock | "unavailable" | null {
  const host = connection;
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!host || !tab) return "unavailable";
  return workspaceSplitBlock(currentTab(), tab, (location) => host.adapter.paneView(location));
}

/** Record where the shown tabs are, so a change of split keeps what they show. */
function recordShownTabs(): WorkspaceTabsState {
  let next = tabsState;
  for (const tab of [currentTab(), partnerTab()]) {
    if (tab) next = updateWorkspaceTab(next, tab.id, { location: liveLocation(tab) });
  }
  return next;
}

/** Show a tab beside the active one, replacing the tab shown there, if any. */
function openInSplit(id: string): void {
  if (splitBlock(id)) return;
  const recorded = recordShownTabs();
  cancelRestore();
  setState(splitWorkspaceTabs(recorded, recorded.activeTabId, id));
  void restoreActive();
}

/** A new tab of a kind, at what its stores show now, or null when no tab can open. */
function newKindTab(kind: View): WorkspaceTab | null {
  const host = connection;
  if (!host || !canAddWorkspaceTab(tabsState)) return null;
  const tab = createWorkspaceTab(createTabId(), host.adapter.read(kind) ?? kindLocation(kind));
  return { ...tab, ...tabNames(host.adapter, tab) };
}

/** Kinds a new tab beside the active one can show. */
function splitKinds(): View[] {
  const host = connection;
  if (!host || !canAddWorkspaceTab(tabsState)) return [];
  const current = currentTab();
  const paneView = (location: WorkspaceTabLocation) => host.adapter.paneView(location);
  return availableAppViews().filter((kind) => {
    const location = host.adapter.read(kind) ?? kindLocation(kind);
    return workspaceSplitBlock(current, createWorkspaceTab("", location), paneView) === null;
  });
}

/** Open a new tab of a kind beside the active one. */
function openKindInSplit(kind: View): void {
  if (!splitKinds().includes(kind)) return;
  const tab = newKindTab(kind);
  if (!tab) return;
  const recorded = recordShownTabs();
  cancelRestore();
  const inserted = insertWorkspaceTab(recorded, tab, "after-active", false);
  setState(splitWorkspaceTabs(inserted, inserted.activeTabId, tab.id));
  void restoreActive();
}

/** Close one side of the split a tab belongs to, the first (left or top) or the second. */
function closeSplitSide(id: string, side: 0 | 1): void {
  const split = workspaceTabSplit(tabsState, id);
  if (split) close(split.tabIds[side]);
}

/**
 * Show the neighbor of the active tab `id` while it is dragged out of the strip. Its place is recorded first,
 * because only the shown tab reads it from the live stores.
 */
function showNeighborOf(id: string): void {
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!tab || id !== tabsState.activeTabId) return;
  const recorded = updateWorkspaceTab(tabsState, id, { location: liveLocation(tab) });
  commitState(showWorkspaceTabNeighbor(recorded, id));
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

/** Whether a tab may move into a new window of its own, which a window's last tab may not. */
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

/**
 * Run one move of a tab out of this window, refusing to start a second move of the same tab meanwhile. The
 * last tab may move to another window, which then closes this one.
 */
async function depart(id: string, move: (windows: WorkspaceTabWindows) => Promise<void>): Promise<void> {
  const windows = connection?.windows;
  if (!windows || detachBlock(id) === "unsupported" || departingTabIds.has(id)) return;
  departingTabIds.add(id);
  try {
    await move(windows);
  } finally {
    departingTabIds.delete(id);
  }
}

/** Move a tab into a new window of its own, opening it at `position` when given. */
function detach(id: string, position: ScreenPoint | null = null): Promise<void> {
  if (detachBlock(id)) return Promise.resolve();
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

/**
 * Start dragging a tab held at `grab`, telling the other windows so they can draw it as it passes over them.
 * Returns what the drag carries, or null when tabs cannot leave this window.
 */
function beginDrag(id: string, grab: TabDragGrab): TabWindowDrag | null {
  const windows = connection?.windows;
  const found = tabsState.tabs.find((candidate) => candidate.id === id);
  const vaultId = storageVaultId;
  if (!found) return null;
  const tab = { ...found, location: liveLocation(found) };
  requestedTabIds.delete(id);
  nativeDragTabId = id;
  lastClaim = null;
  liveDrag = { sourceLabel: windows?.label ?? null, tab, ...grab };
  if (!windows || !vaultId) return null;
  void connection?.flushPendingEdits?.().catch((error: unknown) => {
    console.error("Failed to save pending edits before a tab drag:", error);
  });
  void windows.announceDrag(vaultId, tab, grab).catch((error: unknown) => {
    console.error("Failed to announce a tab drag:", error);
  });
  return { sourceLabel: windows.label, vaultId, tabId: id };
}

/**
 * End the drag of `tabId` as this window saw it end, on its drag source or by a drop on it, and tell the other
 * windows how it ended. Returns that end, or null when this window already heard of it, so each window acts on
 * a drag's end once.
 */
function finishDrag(tabId: string, outcome: TabDragOutcome): TabDragEnd | null {
  if (nativeDragTabId === tabId) nativeDragTabId = null;
  if (liveDrag?.tab.id !== tabId) {
    closeIfLeftEmpty();
    return null;
  }
  const windows = connection?.windows;
  const vaultId = storageVaultId;
  if (windows && vaultId) {
    void windows.announceDragEnd(vaultId, tabId, outcome).catch((error: unknown) => {
      console.error("Failed to announce the end of a tab drag:", error);
    });
  }
  const end = { tabId, ...outcome };
  settleDrag(end);
  return end;
}

/** Act on the end of a drag another window saw end, unless this window already heard of it. */
function concludeDrag(ended: TabWindowDragEnded): void {
  if (liveDrag?.tab.id !== ended.tabId) return;
  const end = { tabId: ended.tabId, accepted: ended.accepted, screen: ended.screen };
  settleDrag(end);
  dragEndListener?.(end);
}

/** Forget a drag that ended. A tab dropped here that has not arrived keeps its place until it does. */
function settleDrag(end: TabDragEnd): void {
  liveDrag = null;
  lastClaim = null;
  if (nativeDragTabId === end.tabId) nativeDragTabId = null;
  if (pendingDrop?.tabId === end.tabId && !pendingDrop.dropped) pendingDrop = null;
  closeIfLeftEmpty();
}

/** Close this window once the drag it started ended, when its last tab left during that drag. */
function closeIfLeftEmpty(): void {
  if (!closeAfterDrag || nativeDragTabId !== null) return;
  closeAfterDrag = false;
  connection?.requestWindowClose();
}

/** Forget any drag under way, as when this window changes vault or disconnects. */
function resetDrag(): void {
  liveDrag = null;
  nativeDragTabId = null;
  pendingDrop = null;
  lastClaim = null;
  stopAwaitingArrival();
}

function stopAwaitingArrival(): void {
  if (arrivalTimer !== null) clearTimeout(arrivalTimer);
  arrivalTimer = null;
}

/** Give up on a tab dropped here that has not arrived in time. */
function awaitArrival(tabId: string): void {
  stopAwaitingArrival();
  arrivalTimer = setTimeout(() => {
    arrivalTimer = null;
    if (pendingDrop?.tabId === tabId) pendingDrop = null;
  }, TAB_ARRIVAL_TIMEOUT_MS);
}

/**
 * Ask for the dragged tab `tabId`, to show it next to `target` or at the end as soon as it arrives. While the
 * drag goes on the request repeats only now and then, until the window holding the tab lets it go; a drop
 * always asks. Returns whether this window can take the tab: it cannot without other windows or with a full
 * strip.
 */
function claim(
  tabId: string,
  target: { id: string; placement: WorkspaceTabPlacement } | null,
  { dropped = false }: { dropped?: boolean } = {},
): boolean {
  const windows = connection?.windows;
  const vaultId = storageVaultId;
  if (!windows || !vaultId) return false;
  if (holds(tabId)) return true;
  if (!canAddWorkspaceTab(tabsState)) return false;
  pendingDrop = { tabId, target, dropped };
  if (dropped) awaitArrival(tabId);
  const now = performance.now();
  if (!dropped && lastClaim?.tabId === tabId && now - lastClaim.at < CLAIM_INTERVAL_MS) return true;
  lastClaim = { tabId, at: now };
  void windows.claim(vaultId, tabId).catch((error: unknown) => {
    console.error("Failed to ask for a dragged tab:", error);
  });
  return true;
}

/** Offer a dragged tab this window holds to the window that claimed it, with its live location and saved edits. */
function offerClaimedTab(request: TabWindowClaim): Promise<void> {
  if (request.vaultId !== storageVaultId || !holds(request.tabId)) return Promise.resolve();
  return depart(request.tabId, async (windows) => {
    const tab = await prepareDeparture(request.tabId);
    if (!tab || !holds(tab.id)) return;
    requestedTabIds.add(tab.id);
    await windows.offer(request.sourceLabel, request.vaultId, tab);
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
    /** The tab drag under way in this vault, from this window or another, for this strip to draw as it passes over. */
    get liveDrag(): LiveTabDrag | null {
      return liveDrag;
    },
    /** Whether this window's strip has room for a tab from another window. */
    get canReceiveTab(): boolean {
      return canAddWorkspaceTab(tabsState);
    },
    /** The split the active tab shows in, or null. */
    get split(): WorkspaceTabSplit | null {
      return workspaceTabSplit(tabsState, tabsState.activeTabId);
    },
    /** The tab shown beside the active one, or null. */
    get partnerTab(): WorkspaceTab | null {
      return partnerTab();
    },
    /** The split a tab belongs to, or null. */
    splitOf(id: string): WorkspaceTabSplit | null {
      return workspaceTabSplit(tabsState, id);
    },
    holds,
    connect,
    activate,
    focusPane,
    splitBlock,
    splitKinds,
    openInSplit,
    openKindInSplit,
    closeSplitSide,
    unsplit(id: string): void {
      setState(unsplitWorkspaceTab(tabsState, id));
    },
    swapSplit(id: string): void {
      setState(swapWorkspaceSplit(tabsState, id));
    },
    setSplitOrientation(id: string, orientation: WorkspaceSplitOrientation): void {
      setState(setWorkspaceSplitOrientation(tabsState, id, orientation));
    },
    resizeSplit(id: string, ratio: number): void {
      setState(resizeWorkspaceSplit(tabsState, id, ratio));
    },
    showNeighborOf,
    openKind,
    openLocation,
    close,
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
    beginDrag,
    finishDrag,
    claim,
    /** Act on the ends of drags this window hears of from another window. Returns a cleanup. */
    onDragEnd(listener: (end: TabDragEnd) => void): () => void {
      dragEndListener = listener;
      return () => {
        if (dragEndListener === listener) dragEndListener = null;
      };
    },
    releaseWindow,
    otherOpenWindows,
    restoreWindows,
    requestLauncher(): void {
      launcherRequests += 1;
    },
  };
}

export type WorkspaceTabs = ReturnType<typeof getWorkspaceTabs>;
