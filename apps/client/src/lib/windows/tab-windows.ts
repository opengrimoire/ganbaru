import { emit, listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { View } from "$lib/navigation";
import { readNativeBackground } from "$lib/themes/native-background";
import { parseWorkspaceTab, type WorkspaceTab } from "$lib/workspace-tabs/model";

/** Label of the window the app starts with. It owns app-wide work and outlives the other windows. */
export const MAIN_WINDOW_LABEL = "main";
/** Prefix of the windows that hold workspace tabs besides the main one; the desktop capability grants it. */
const TAB_WINDOW_LABEL_PREFIX = "view-";
const TAB_WINDOW_LABEL_PATTERN = /^view-[A-Za-z0-9-]{1,64}$/;
const TAB_WINDOW_TRANSFER_EVENT = "tab-window-transfer";
const TAB_WINDOW_RELEASE_EVENT = "tab-window-release";
const TAB_WINDOW_CLOSED_EVENT = "tab-window-closed";
/** Surface the desktop shell paints first, used as the native background of a new window. */
const TAB_WINDOW_BACKGROUND_TOKEN = "--sidebar";

interface TabWindowMessage {
  /** The only window that acts on the message. */
  targetLabel: string;
  sourceLabel: string;
  /** Vault both windows show; a window on another vault ignores the message. */
  vaultId: string;
}

/** A tab offered to another window, which adopts it and then releases it from its source. */
export interface TabWindowTransfer extends TabWindowMessage {
  tab: WorkspaceTab;
}

/** Tells the source window that a tab now lives elsewhere, so it drops its copy. */
export interface TabWindowRelease extends TabWindowMessage {
  tabId: string;
}

/** The tabs of a window the user closed, so the main window can offer them for reopening. */
export interface TabWindowClosed extends TabWindowMessage {
  tabs: WorkspaceTab[];
}

/** Create a unique label for a new tab window. */
export function createTabWindowLabel(createId: () => string = () => crypto.randomUUID()): string {
  return `${TAB_WINDOW_LABEL_PREFIX}${createId()}`;
}

/** Return whether a window label names a tab window other than the main one. */
export function isTabWindowLabel(label: unknown): label is string {
  return typeof label === "string" && TAB_WINDOW_LABEL_PATTERN.test(label);
}

function isWindowLabel(label: unknown): label is string {
  return label === MAIN_WINDOW_LABEL || isTabWindowLabel(label);
}

/** Address of a new tab window, carrying the tab it opens with and that tab's kind for the first paint. */
export function tabWindowUrl(tab: WorkspaceTab | null): string {
  if (!tab) return "/";
  const params = new URLSearchParams({ view: tab.location.kind, tab: JSON.stringify(tab) });
  return `/?${params.toString()}`;
}

/** Read the tab a window was opened with, when it is valid and its kind is available. */
export function parseTabWindowSearch(search: string, availableKinds: readonly View[]): WorkspaceTab | null {
  const raw = new URLSearchParams(search).get("tab");
  return raw ? parseWorkspaceTab(parseJson(raw), availableKinds) : null;
}

function parseJson(value: string): unknown {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return null;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseMessage(value: unknown): TabWindowMessage | null {
  if (!isRecord(value)) return null;
  const { targetLabel, sourceLabel, vaultId } = value;
  if (!isWindowLabel(targetLabel) || !isWindowLabel(sourceLabel) || targetLabel === sourceLabel) return null;
  if (typeof vaultId !== "string" || vaultId.length === 0) return null;
  return { targetLabel, sourceLabel, vaultId };
}

/** Validate a transfer from another window. */
export function parseTabWindowTransfer(value: unknown, availableKinds: readonly View[]): TabWindowTransfer | null {
  const message = parseMessage(value);
  const tab = message && isRecord(value) ? parseWorkspaceTab(value.tab, availableKinds) : null;
  return message && tab ? { ...message, tab } : null;
}

/** Validate a release from another window. */
export function parseTabWindowRelease(value: unknown): TabWindowRelease | null {
  const message = parseMessage(value);
  if (!message || !isRecord(value) || typeof value.tabId !== "string" || value.tabId.length === 0) return null;
  return { ...message, tabId: value.tabId };
}

/** Validate a closed-window report, keeping only valid tabs. */
export function parseTabWindowClosed(value: unknown, availableKinds: readonly View[]): TabWindowClosed | null {
  const message = parseMessage(value);
  if (!message || !isRecord(value) || !Array.isArray(value.tabs)) return null;
  const tabs = value.tabs.flatMap((entry) => {
    const tab = parseWorkspaceTab(entry, availableKinds);
    return tab ? [tab] : [];
  });
  return { ...message, tabs };
}

/** Send a tab-window message. Every window receives it and only its target acts on it. */
async function sendTabWindowMessage(
  event: typeof TAB_WINDOW_TRANSFER_EVENT | typeof TAB_WINDOW_RELEASE_EVENT | typeof TAB_WINDOW_CLOSED_EVENT,
  message: TabWindowTransfer | TabWindowRelease | TabWindowClosed,
): Promise<void> {
  await emit(event, message);
}

/**
 * Listen for one kind of tab-window message addressed to a window. Invalid payloads and messages for other
 * windows are dropped. Returns a cleanup that also works before the listener finished registering.
 */
function listenTabWindowMessages<T extends TabWindowMessage>(
  event: string,
  label: string,
  parse: (value: unknown) => T | null,
  listener: (message: T) => void,
): () => void {
  let disposed = false;
  let cleanup: (() => void) | undefined;
  listen<unknown>(event, (received) => {
    const message = parse(received.payload);
    if (message && message.targetLabel === label) listener(message);
  }).then((unlisten) => {
    if (disposed) unlisten();
    else cleanup = unlisten;
  }).catch((error: unknown) => {
    console.error(`Failed to listen for ${event}:`, error);
  });
  return () => {
    disposed = true;
    cleanup?.();
  };
}

async function focusExistingWindow(window: WebviewWindow): Promise<void> {
  await window.show();
  await window.unminimize();
  await window.setFocus();
}

/** Bring the main window forward. */
async function focusMainWindow(): Promise<void> {
  const mainWindow = await WebviewWindow.getByLabel(MAIN_WINDOW_LABEL);
  if (mainWindow) await focusExistingWindow(mainWindow);
}

/**
 * Open a tab window, or focus it when it is already open. A new window starts with `tab`, or with the
 * tabs it saved when it has none. A window opened without focus leaves the current one in front.
 */
async function openTabWindow(label: string, tab: WorkspaceTab | null, focus = true): Promise<void> {
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    if (focus) await focusExistingWindow(existing);
    return;
  }

  const background = readNativeBackground(document.documentElement, TAB_WINDOW_BACKGROUND_TOKEN);
  const created = new WebviewWindow(label, {
    ...(background ? { backgroundColor: [background.r, background.g, background.b] } : {}),
    url: tabWindowUrl(tab),
    title: "Ganbaru AI",
    width: 960,
    height: 720,
    minWidth: 280,
    minHeight: 180,
    center: true,
    decorations: false,
    visible: true,
    focus,
  });

  await new Promise<void>((resolve, reject) => {
    void created.once<void>("tauri://created", () => resolve());
    void created.once<unknown>("tauri://error", (event) => {
      reject(event.payload);
    });
  });
}

/** What a window does with the tab-window messages addressed to it. */
export interface TabWindowHandlers {
  transfer(message: TabWindowTransfer): void;
  release(message: TabWindowRelease): void;
  /** Only the main window receives closed-window reports. */
  closed(message: TabWindowClosed): void;
}

/** Desktop window operations that let workspace tabs move between this window and the others. */
export interface WorkspaceTabWindows {
  readonly label: string;
  readonly mainLabel: string;
  /** Whether this is the main window, which keeps the tabs of a single-window setup and the reopen history. */
  readonly isMain: boolean;
  /** Open a new window that starts with `tab`. */
  open(tab: WorkspaceTab): Promise<void>;
  /** Reopen a window with the tabs it saved, behind the current one. Labels that name no tab window are ignored. */
  reopen(label: string): Promise<void>;
  /** Offer a tab to another window, which adopts it and then releases it from this one. */
  offer(targetLabel: string, vaultId: string, tab: WorkspaceTab): Promise<void>;
  /** Confirm to the window that offered a tab that it now lives here. */
  release(targetLabel: string, vaultId: string, tabId: string): Promise<void>;
  /** Hand the tabs of this closing window to the main window's reopen history. */
  reportClosed(vaultId: string, tabs: WorkspaceTab[]): Promise<void>;
  focusMain(): Promise<void>;
  /** Listen for messages addressed to this window. Returns a cleanup. */
  listen(handlers: TabWindowHandlers): () => void;
}

/** Create the window operations of the window labelled `label`, accepting tabs of the available kinds. */
export function createWorkspaceTabWindows(label: string, availableKinds: readonly View[]): WorkspaceTabWindows {
  const isMain = label === MAIN_WINDOW_LABEL;
  return {
    label,
    isMain,
    mainLabel: MAIN_WINDOW_LABEL,
    open: (tab) => openTabWindow(createTabWindowLabel(), tab),
    reopen: async (savedLabel) => {
      if (isTabWindowLabel(savedLabel)) await openTabWindow(savedLabel, null, false);
    },
    offer: (targetLabel, vaultId, tab) =>
      sendTabWindowMessage(TAB_WINDOW_TRANSFER_EVENT, { targetLabel, sourceLabel: label, vaultId, tab }),
    release: (targetLabel, vaultId, tabId) =>
      sendTabWindowMessage(TAB_WINDOW_RELEASE_EVENT, { targetLabel, sourceLabel: label, vaultId, tabId }),
    reportClosed: (vaultId, tabs) =>
      sendTabWindowMessage(TAB_WINDOW_CLOSED_EVENT, { targetLabel: MAIN_WINDOW_LABEL, sourceLabel: label, vaultId, tabs }),
    focusMain: focusMainWindow,
    listen(handlers) {
      const cleanups = [
        listenTabWindowMessages(
          TAB_WINDOW_TRANSFER_EVENT,
          label,
          (value) => parseTabWindowTransfer(value, availableKinds),
          handlers.transfer,
        ),
        listenTabWindowMessages(TAB_WINDOW_RELEASE_EVENT, label, parseTabWindowRelease, handlers.release),
      ];
      if (isMain) {
        cleanups.push(listenTabWindowMessages(
          TAB_WINDOW_CLOSED_EVENT,
          label,
          (value) => parseTabWindowClosed(value, availableKinds),
          handlers.closed,
        ));
      }
      return () => {
        for (const cleanup of cleanups) cleanup();
      };
    },
  };
}
