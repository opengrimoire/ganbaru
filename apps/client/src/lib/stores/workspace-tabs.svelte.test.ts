// @vitest-environment jsdom

import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { View } from "$lib/navigation";
import type { TabWindowHandlers, WorkspaceTabWindows } from "$lib/windows/tab-windows";
import type { WorkspaceTab } from "$lib/workspace-tabs/model";
import type { WorkspaceLocationAdapter, WorkspaceTabs } from "./workspace-tabs.svelte";

/** Each test connects to a vault of its own, so no tabs carry over from the one before. */
let VAULT_ID = "vault-0";
let vaultCount = 0;
const GRAB = { grabX: 4, grabY: 6, width: 120 };
const NO_SCREEN = { accepted: false, screen: null };
const ACCEPTED = { accepted: true, screen: null };

interface FakeWindows extends WorkspaceTabWindows {
  handlers: TabWindowHandlers;
}

function fakeWindows(label = "tab-window-a"): FakeWindows {
  const windows: FakeWindows = {
    label,
    isHost: false,
    handlers: {
      transfer: () => undefined,
      release: () => undefined,
      claim: () => undefined,
      dragStarted: () => undefined,
      dragEnded: () => undefined,
      raised: () => undefined,
    },
    open: vi.fn(async () => undefined),
    reopen: vi.fn(async () => undefined),
    offer: vi.fn(async () => undefined),
    release: vi.fn(async () => undefined),
    claim: vi.fn(async () => undefined),
    announceDrag: vi.fn(async () => undefined),
    announceDragEnd: vi.fn(async () => undefined),
    otherOpenWindows: vi.fn(async () => 1),
    minimize: vi.fn(async () => undefined),
    listen(handlers) {
      windows.handlers = handlers;
      return () => undefined;
    },
  };
  return windows;
}

const adapter: WorkspaceLocationAdapter = {
  read: () => null,
  itemTitle: () => null,
  projectName: () => null,
  paneView: (location) => location.kind,
  apply: async () => undefined,
};

interface Harness {
  tabs: WorkspaceTabs;
  windows: FakeWindows | undefined;
  requestWindowClose: ReturnType<typeof vi.fn>;
}

let cleanups: (() => void)[] = [];

async function connect(withWindows = true): Promise<Harness> {
  const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
  setActiveVaultIdentity(VAULT_ID);
  const { getWorkspaceTabs } = await import("./workspace-tabs.svelte");
  const tabs = getWorkspaceTabs();
  const windows = withWindows ? fakeWindows() : undefined;
  const requestWindowClose = vi.fn();
  cleanups.push($effect.root(() => {
    tabs.connect({ adapter, windows, requestWindowClose, flushPendingEdits: async () => undefined });
  }));
  flushSync();
  return { tabs, windows, requestWindowClose };
}

/** Two tabs, so either may leave without closing the window. */
function openSecondTab(tabs: WorkspaceTabs): [WorkspaceTab, WorkspaceTab] {
  const kinds = tabs.splitKinds();
  const other = kinds.find((kind) => kind !== tabs.activeTab.location.kind) ?? kinds[0];
  tabs.openKind(other as View);
  const [first, second] = tabs.tabs;
  if (!first || !second) throw new Error("expected two tabs");
  return [first, second];
}

function foreignTab(id: string, base: WorkspaceTab): WorkspaceTab {
  return { ...base, id };
}

describe("workspace tab drags", () => {
  beforeEach(() => {
    vaultCount += 1;
    VAULT_ID = `vault-${vaultCount}`;
    vi.useFakeTimers();
    localStorage.clear();
  });

  afterEach(() => {
    for (const cleanup of cleanups) cleanup();
    cleanups = [];
    vi.useRealTimers();
  });

  it("starts a drag that every window hears of and ends it once", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);

    const drag = tabs.beginDrag(first.id, GRAB);

    expect(drag).toEqual({ sourceLabel: "tab-window-a", vaultId: VAULT_ID, tabId: first.id });
    expect(tabs.liveDrag?.tab.id).toBe(first.id);
    expect(tabs.liveDrag?.sourceLabel).toBe("tab-window-a");
    expect(windows?.announceDrag).toHaveBeenCalledWith(VAULT_ID, expect.objectContaining({ id: first.id }), GRAB);

    expect(tabs.finishDrag(first.id, NO_SCREEN)).toEqual({ tabId: first.id, ...NO_SCREEN });
    expect(tabs.liveDrag).toBeNull();
    expect(windows?.announceDragEnd).toHaveBeenCalledWith(VAULT_ID, first.id, NO_SCREEN);
    expect(tabs.finishDrag(first.id, NO_SCREEN)).toBeNull();
    expect(windows?.announceDragEnd).toHaveBeenCalledTimes(1);
  });

  it("draws a drag without windows but carries nothing", async () => {
    const { tabs } = await connect(false);

    expect(tabs.beginDrag(tabs.activeTab.id, GRAB)).toBeNull();
    expect(tabs.liveDrag?.sourceLabel).toBeNull();
    expect(tabs.beginDrag("missing", GRAB)).toBeNull();
  });

  it("follows a drag another window started and acts on its end once", async () => {
    const { tabs, windows } = await connect();
    const listener = vi.fn();
    tabs.onDragEnd(listener);
    const dragged = foreignTab("foreign", tabs.activeTab);

    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: "other-vault", tab: dragged, ...GRAB });
    expect(tabs.liveDrag).toBeNull();
    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tab: dragged, ...GRAB });
    expect(tabs.liveDrag?.tab.id).toBe("foreign");

    const ended = { sourceLabel: "tab-window-b", vaultId: VAULT_ID, tabId: "foreign", ...ACCEPTED };
    windows?.handlers.dragEnded(ended);
    windows?.handlers.dragEnded(ended);

    expect(tabs.liveDrag).toBeNull();
    expect(listener).toHaveBeenCalledOnce();
    expect(listener).toHaveBeenCalledWith({ tabId: "foreign", ...ACCEPTED });
  });

  it("asks for a dragged tab now and then while it passes, and always on a drop", async () => {
    const { tabs, windows } = await connect();

    expect(tabs.claim("foreign", null)).toBe(true);
    expect(tabs.claim("foreign", null)).toBe(true);
    expect(windows?.claim).toHaveBeenCalledTimes(1);

    vi.advanceTimersByTime(200);
    expect(tabs.claim("foreign", null)).toBe(true);
    expect(windows?.claim).toHaveBeenCalledTimes(2);

    expect(tabs.claim("foreign", null, { dropped: true })).toBe(true);
    expect(windows?.claim).toHaveBeenCalledTimes(3);
  });

  it("cannot take a dragged tab without windows, and holds its own tab already", async () => {
    const solo = await connect(false);
    expect(solo.tabs.claim("foreign", null)).toBe(false);
    for (const cleanup of cleanups) cleanup();
    cleanups = [];

    const { tabs, windows } = await connect();
    expect(tabs.claim(tabs.activeTab.id, null)).toBe(true);
    expect(windows?.claim).not.toHaveBeenCalled();
  });

  it("places a claimed tab at its target when it arrives and confirms the move", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);
    const arriving = foreignTab("foreign", first);

    tabs.claim("foreign", { id: first.id, placement: "before" });
    windows?.handlers.transfer({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tab: arriving });

    expect(tabs.tabs.map(({ id }) => id).indexOf("foreign")).toBe(0);
    expect(windows?.release).toHaveBeenCalledWith("tab-window-b", VAULT_ID, "foreign");
  });

  it("keeps a dropped tab's target after its drag ends until the tab arrives", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);
    const dragged = foreignTab("foreign", first);
    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tab: dragged, ...GRAB });

    tabs.claim("foreign", { id: first.id, placement: "before" }, { dropped: true });
    windows?.handlers.dragEnded({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tabId: "foreign", ...ACCEPTED });
    windows?.handlers.transfer({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tab: dragged });

    expect(tabs.tabs[0]?.id).toBe("foreign");
  });

  it("forgets a hovered tab's target when its drag ends without a drop here", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);
    const dragged = foreignTab("foreign", first);
    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tab: dragged, ...GRAB });

    tabs.claim("foreign", { id: first.id, placement: "before" });
    windows?.handlers.dragEnded({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tabId: "foreign", ...NO_SCREEN });
    windows?.handlers.transfer({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tab: dragged });

    expect(tabs.tabs.at(-1)?.id).toBe("foreign");
  });

  it("gives up a dropped tab's target once it has not arrived in time", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);
    const dragged = foreignTab("foreign", first);

    tabs.claim("foreign", { id: first.id, placement: "before" }, { dropped: true });
    const { TAB_ARRIVAL_TIMEOUT_MS } = await import("./workspace-tabs.svelte");
    vi.advanceTimersByTime(TAB_ARRIVAL_TIMEOUT_MS);
    windows?.handlers.transfer({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tab: dragged });

    expect(tabs.tabs.at(-1)?.id).toBe("foreign");
  });

  it("hides the window whose last tab left during its own drag and closes it when the drag ends", async () => {
    const { tabs, windows, requestWindowClose } = await connect();
    const only = tabs.activeTab;
    tabs.beginDrag(only.id, GRAB);

    windows?.handlers.release({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tabId: only.id });

    expect(windows?.minimize).toHaveBeenCalledOnce();
    expect(requestWindowClose).not.toHaveBeenCalled();
    expect(tabs.holds(only.id)).toBe(false);

    tabs.finishDrag(only.id, ACCEPTED);
    expect(requestWindowClose).toHaveBeenCalledOnce();
  });

  it("closes the emptied window when its own drag ends even after another drag replaced the drawn one", async () => {
    const { tabs, windows, requestWindowClose } = await connect();
    const only = tabs.activeTab;
    tabs.beginDrag(only.id, GRAB);
    windows?.handlers.release({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tabId: only.id });
    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tab: foreignTab("foreign", only), ...GRAB });

    expect(tabs.finishDrag(only.id, ACCEPTED)).toBeNull();
    expect(requestWindowClose).toHaveBeenCalledOnce();
    expect(tabs.liveDrag?.tab.id).toBe("foreign");
  });

  it("closes the window at once when its last tab leaves outside a drag", async () => {
    const { tabs, windows, requestWindowClose } = await connect();

    windows?.handlers.release({ sourceLabel: "tab-window-b", targetLabel: "tab-window-a", vaultId: VAULT_ID, tabId: tabs.activeTab.id });

    expect(windows?.minimize).not.toHaveBeenCalled();
    expect(requestWindowClose).toHaveBeenCalledOnce();
  });

  it("leaves a torn-off tab to the window that asked for it", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);

    windows?.handlers.claim({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tabId: first.id });
    await vi.waitFor(() => expect(windows?.offer).toHaveBeenCalledOnce());
    const tearing = tabs.tearOff(first.id, null);
    await vi.advanceTimersByTimeAsync(300);
    await tearing;

    expect(windows?.open).not.toHaveBeenCalled();
  });

  it("tears off a dropped tab into a window of its own after a short wait", async () => {
    const { tabs, windows } = await connect();
    const [first] = openSecondTab(tabs);

    const tearing = tabs.tearOff(first.id, { x: 10, y: 20 });
    await vi.advanceTimersByTimeAsync(300);
    await tearing;

    expect(windows?.open).toHaveBeenCalledWith(expect.objectContaining({ id: first.id }), { x: 10, y: 20 });
    expect(tabs.holds(first.id)).toBe(false);
  });

  it("leaves readers of the drawn drag alone while a hovered tab is claimed", async () => {
    const { tabs, windows } = await connect();
    windows?.handlers.dragStarted({ sourceLabel: "tab-window-b", vaultId: VAULT_ID, tab: foreignTab("foreign", tabs.activeTab), ...GRAB });
    let reads = 0;
    cleanups.push($effect.root(() => {
      $effect(() => {
        void tabs.liveDrag;
        reads += 1;
      });
    }));
    flushSync();
    const before = reads;

    tabs.claim("foreign", null);
    vi.advanceTimersByTime(200);
    tabs.claim("foreign", null);
    flushSync();

    expect(reads).toBe(before);
  });

  it("forgets a drag under way when the vault changes", async () => {
    const { tabs } = await connect();
    const [first] = openSecondTab(tabs);
    tabs.beginDrag(first.id, GRAB);

    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    setActiveVaultIdentity("another-vault");

    expect(tabs.liveDrag).toBeNull();
  });
});
