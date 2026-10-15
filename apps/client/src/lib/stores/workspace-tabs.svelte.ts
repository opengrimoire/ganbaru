import { untrack } from "svelte";
import { availableAppViews, type View } from "$lib/navigation";
import { registerNavigationRouter, type NavigationRouterRegistration } from "$lib/stores/navigation.svelte";
import { activeVaultIdentity, onActiveVaultIdentityChange } from "$lib/vault/active-vault";
import type {
  DetachedViewWindowChange,
  DetachedViewWindowTab,
} from "$lib/windows/detached";
import {
  activateWorkspaceTab,
  activeWorkspaceTab,
  attachWorkspaceTabs,
  closeOtherWorkspaceTabs,
  closeWorkspaceTab,
  createWorkspaceTab,
  defaultWorkspaceTabsState,
  detachWorkspaceTab,
  insertWorkspaceTab,
  isWithinTabScope,
  kindHasLocations,
  kindLocation,
  moveWorkspaceTab,
  parseWorkspaceTabsState,
  relativeWorkspaceTabId,
  removeWorkspaceTab,
  reopenWorkspaceTab,
  sameTabLocation,
  serializeWorkspaceTabsState,
  tabMatchesTarget,
  updateWorkspaceTab,
  visibleWorkspaceTabs,
  workspaceTabIdAtPosition,
  type ClosedWorkspaceTab,
  type WorkspaceTab,
  type WorkspaceTabLevel,
  type WorkspaceTabLocation,
  type WorkspaceTabPlacement,
  type WorkspaceTabsState,
} from "$lib/workspace-tabs/model";

/** Reads and restores the singleton view stores that a tab location describes. */
export interface WorkspaceLocationAdapter {
  read(kind: View): WorkspaceTabLocation;
  /** Name of what a tab at this level points at, or null when it has none or it is not loaded. */
  title(level: WorkspaceTabLevel, location: WorkspaceTabLocation): string | null;
  /** Restore a location, stopping between steps once `isCurrent` turns false. */
  apply(location: WorkspaceTabLocation, isCurrent: () => boolean): Promise<void>;
}

/** Desktop window operations behind detached tabs. */
export interface WorkspaceTabWindowHost {
  canDetach: boolean;
  detachedKinds(): ReadonlySet<View>;
  /** Open the kind's window with the tab and record the kind as detached. */
  open(kind: View, tab: DetachedViewWindowTab): Promise<void>;
  /** Show a location in the window that holds its kind. */
  showLocation(kind: View, location: WorkspaceTabLocation | null): void;
  onChange(listener: (change: DetachedViewWindowChange) => void): () => void;
  onOpenLocation(listener: (location: WorkspaceTabLocation) => void): () => void;
}

export type WorkspaceTabsWindowMode =
  | { type: "main" }
  | { type: "detached"; view: View; tab: DetachedViewWindowTab | null };

export interface WorkspaceTabsConnection {
  adapter: WorkspaceLocationAdapter;
  mode: WorkspaceTabsWindowMode;
  /** Ask to close the app, or this window when it is detached. */
  requestWindowClose(): void;
  windows?: WorkspaceTabWindowHost;
}

export type WorkspaceTabDetachBlock = "unsupported" | "last-tab" | "kind-has-other-tabs";

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

function isMainMode(): boolean {
  return connection?.mode.type === "main";
}

function storageKey(vaultId: string): string {
  return `${STORAGE_PREFIX}:${vaultId}`;
}

function readStoredState(vaultId: string | null): WorkspaceTabsState | null {
  if (!vaultId || typeof localStorage === "undefined") return null;
  try {
    const raw = localStorage.getItem(storageKey(vaultId));
    return raw ? parseWorkspaceTabsState(JSON.parse(raw) as unknown, availableAppViews()) : null;
  } catch (error) {
    console.warn("Failed to read workspace tabs:", error);
    return null;
  }
}

function persist(state: WorkspaceTabsState): void {
  if (!isMainMode() || !storageVaultId || typeof localStorage === "undefined") return;
  try {
    localStorage.setItem(storageKey(storageVaultId), serializeWorkspaceTabsState(state));
  } catch (error) {
    console.warn("Failed to save workspace tabs:", error);
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

function isKindDetached(kind: View): boolean {
  return connection?.windows?.detachedKinds().has(kind) ?? false;
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
    if (sequence === restoreSequence) restoring = false;
  }
}

function commitState(next: WorkspaceTabsState): void {
  const previousActive = tabsState.activeTabId;
  if (setState(next) && next.activeTabId !== previousActive) void restoreActive();
}

function recordLocation(tabId: string, location: WorkspaceTabLocation): void {
  setState(updateWorkspaceTab(tabsState, tabId, { location }));
}

function mostRecentTabOfKind(kind: View): WorkspaceTab | null {
  return mostRecent(visibleWorkspaceTabs(tabsState).filter((tab) => tab.location.kind === kind && !tab.locked));
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
  const candidates = visibleWorkspaceTabs(tabsState).filter((tab) => isWithinTabScope(tab, location));
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
  if (host.mode.type === "detached") return kind === host.mode.view && (navigation?.commit(kind) ?? false);
  if (isKindDetached(kind)) {
    host.windows?.showLocation(kind, meaningfulLocation(host.adapter.read(kind)));
    return false;
  }
  if (currentTab().location.kind === kind) return navigation?.commit(kind) ?? false;
  const location = host.adapter.read(kind);
  const target = tabForLocation(location);
  const next = target
    ? activateWorkspaceTab(tabsState, target.id)
    : insertWorkspaceTab(tabsState, createWorkspaceTab(createTabId(), location), "after-active");
  if (currentTab().location.kind !== kind && next === tabsState) return false;
  cancelRestore();
  setState(next);
  return navigation?.commit(kind) ?? false;
}

function handleDetachedChange(change: DetachedViewWindowChange): void {
  if (change.detached) {
    if (change.tabId) commitState(detachWorkspaceTab(tabsState, change.tabId));
    return;
  }
  const tab = change.tabId ? tabsState.tabs.find((candidate) => candidate.id === change.tabId) : undefined;
  if (change.closed) {
    if (tab) setState(removeWorkspaceTab(tabsState, tab.id, change.location ?? tab.location));
    setState(attachWorkspaceTabs(tabsState, isKindDetached));
    return;
  }
  setState(attachWorkspaceTabs(tabsState, isKindDetached));
  if (!change.tabId) return;
  const location = change.location ?? tab?.location ?? kindLocation(change.view);
  if (tab) {
    setState(updateWorkspaceTab(tabsState, tab.id, { detached: false, location }));
    setState(activateWorkspaceTab(tabsState, tab.id));
  } else {
    setState(insertWorkspaceTab(tabsState, createWorkspaceTab(change.tabId, location), "after-active"));
  }
  void restoreActive();
}

function loadForVault(vaultId: string | null): void {
  storageVaultId = vaultId;
  const loaded = readStoredState(vaultId) ?? defaultWorkspaceTabsState(availableAppViews(), createTabId);
  activationOrder = [];
  setState(loaded);
}

function connectMain(host: WorkspaceTabsConnection): () => void {
  loadForVault(activeVaultIdentity());
  const cleanups: (() => void)[] = [];
  cleanups.push(onActiveVaultIdentityChange((_previous, next) => {
    if (next === storageVaultId) return;
    loadForVault(next);
    void restoreActive();
  }));
  if (host.windows) {
    const windows = host.windows;
    cleanups.push(windows.onChange(handleDetachedChange));
    $effect(() => {
      const detached = windows.detachedKinds();
      untrack(() => setState(attachWorkspaceTabs(tabsState, (kind) => detached.has(kind))));
    });
  }
  return () => cleanups.forEach((cleanup) => cleanup());
}

function connectDetached(host: WorkspaceTabsConnection, view: View, tab: DetachedViewWindowTab | null): () => void {
  storageVaultId = null;
  const initial = createWorkspaceTab(tab?.tabId ?? createTabId(), tab?.location ?? kindLocation(view));
  activationOrder = [];
  setState({ tabs: [initial], activeTabId: initial.id, closed: [] });
  return host.windows?.onOpenLocation((location) => {
    if (location.kind !== view) return;
    setState(updateWorkspaceTab(tabsState, currentTab().id, { location }));
    void restoreActive();
  }) ?? (() => undefined);
}

/**
 * Bind the tabs to this window. Call during the shell component's initialization so the recording
 * effect lives and dies with it.
 */
function connect(host: WorkspaceTabsConnection): void {
  connection = host;
  navigation = registerNavigationRouter(routeNavigation);
  const disconnectMode = host.mode.type === "main"
    ? connectMain(host)
    : connectDetached(host, host.mode.view, host.mode.tab);
  void restoreActive();

  $effect(() => {
    if (restoring) return;
    const tab = currentTab();
    if (tab.detached) return;
    const location = host.adapter.read(tab.location.kind);
    untrack(() => recordLocation(tab.id, location));
  });

  // Keep cached titles current, including renames of what inactive tabs point at.
  $effect(() => {
    const titles = visibleWorkspaceTabs(tabsState)
      .map((tab) => ({ id: tab.id, title: host.adapter.title(tab.level, tab.location) }))
      .filter((entry): entry is { id: string; title: string } => entry.title !== null);
    untrack(() => {
      let next = tabsState;
      for (const entry of titles) next = updateWorkspaceTab(next, entry.id, { title: entry.title });
      setState(next);
    });
  });

  $effect(() => () => {
    disconnectMode();
    navigation?.unregister();
    navigation = null;
    if (connection === host) connection = null;
  });
}

function activate(id: string): void {
  if (id === tabsState.activeTabId) return;
  commitState(activateWorkspaceTab(tabsState, id));
}

/** Open a new tab of a kind from the launcher. A kind without locations focuses its existing tab. */
function openKind(kind: View): void {
  const host = connection;
  if (!host) return;
  if (isKindDetached(kind)) {
    host.windows?.showLocation(kind, null);
    return;
  }
  const existing = kindHasLocations(kind)
    ? null
    : visibleWorkspaceTabs(tabsState).find((tab) => tab.location.kind === kind);
  if (existing) {
    activate(existing.id);
    return;
  }
  commitState(insertWorkspaceTab(tabsState, createWorkspaceTab(createTabId(), host.adapter.read(kind)), "end"));
}

export interface OpenWorkspaceLocationOptions {
  /** Leave the current tab in front. */
  background?: boolean;
}

/**
 * Open a project or item in a tab of its own. A tab that already shows it at that level is focused
 * instead, and a kind shown in another window receives the location there.
 */
function openLocation(
  level: WorkspaceTabLevel,
  location: WorkspaceTabLocation,
  options: OpenWorkspaceLocationOptions = {},
): void {
  const host = connection;
  if (!host || !isMainMode()) return;
  if (isKindDetached(location.kind)) {
    host.windows?.showLocation(location.kind, location);
    return;
  }
  const tab = createWorkspaceTab(createTabId(), location, { level, title: host.adapter.title(level, location) });
  const existing = visibleWorkspaceTabs(tabsState).find((candidate) => tabMatchesTarget(candidate, tab.level, location));
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

function close(id: string): void {
  const host = connection;
  if (!host) return;
  if (host.mode.type === "detached") {
    host.requestWindowClose();
    return;
  }
  const next = closeWorkspaceTab(tabsState, id);
  if (next === null) host.requestWindowClose();
  else commitState(next);
}

function reopen(closedIndex = 0): void {
  const host = connection;
  const entry = tabsState.closed[closedIndex];
  if (!host || !isMainMode() || !entry) return;
  if (isKindDetached(entry.location.kind)) {
    host.windows?.showLocation(entry.location.kind, entry.location);
    setState({ ...tabsState, closed: tabsState.closed.filter((_, index) => index !== closedIndex) });
    return;
  }
  const reopened = reopenWorkspaceTab(tabsState, createTabId, closedIndex);
  if (reopened) commitState(reopened.state);
}

function detachBlock(id: string): WorkspaceTabDetachBlock | null {
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!connection?.windows?.canDetach || !isMainMode() || !tab || tab.detached) return "unsupported";
  if (visibleWorkspaceTabs(tabsState).length <= 1) return "last-tab";
  const sameKind = tabsState.tabs.some((candidate) => (
    candidate.id !== id && candidate.location.kind === tab.location.kind
  ));
  return sameKind || isKindDetached(tab.location.kind) ? "kind-has-other-tabs" : null;
}

/** Move a tab into its own window. The main window keeps its place for when it comes back. */
async function detach(id: string): Promise<void> {
  const host = connection;
  const tab = tabsState.tabs.find((candidate) => candidate.id === id);
  if (!host?.windows || !tab || detachBlock(id)) return;
  const location = id === tabsState.activeTabId ? host.adapter.read(tab.location.kind) : tab.location;
  setState(updateWorkspaceTab(tabsState, id, { location }));
  await host.windows.open(tab.location.kind, { tabId: id, location });
}

/** Return the tab this detached window shows, with its live location. */
function detachedDetails(): DetachedViewWindowTab | null {
  const host = connection;
  if (host?.mode.type !== "detached") return null;
  const tab = currentTab();
  return { tabId: tab.id, location: restoring ? tab.location : host.adapter.read(tab.location.kind) };
}

/** Bring back the most recent tab of a kind after its window returns, restoring where it was. */
function focusKind(kind: View): void {
  setState(attachWorkspaceTabs(tabsState, isKindDetached));
  const tab = mostRecentTabOfKind(kind);
  if (!tab) return;
  setState(activateWorkspaceTab(tabsState, tab.id));
  void restoreActive();
}

export function getWorkspaceTabs() {
  return {
    get tabs(): readonly WorkspaceTab[] {
      return visibleWorkspaceTabs(tabsState);
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
    /** Whether this window can open more tabs; a detached window holds exactly one. */
    get canOpenTabs(): boolean {
      return connection !== null && isMainMode();
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
    detachedDetails,
    focusKind,
    requestLauncher(): void {
      launcherRequests += 1;
    },
  };
}

export type WorkspaceTabs = ReturnType<typeof getWorkspaceTabs>;

