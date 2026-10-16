import { emit, listen } from "@tauri-apps/api/event";
import { getAllWebviewWindows, getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { View } from "$lib/navigation";
import { readNativeBackground } from "$lib/themes/native-background";
import { parseWorkspaceTab, type WorkspaceTab } from "$lib/workspace-tabs/model";

/** Label of the window the app starts with. It hosts app-wide work and hides instead of closing while other windows stay open. */
export const MAIN_WINDOW_LABEL = "main";
/** Prefix of the windows opened for workspace tabs besides the first one; the desktop capability grants it. */
const TAB_WINDOW_LABEL_PREFIX = "view-";
const TAB_WINDOW_LABEL_PATTERN = /^view-[A-Za-z0-9-]{1,64}$/;
const TAB_WINDOW_TRANSFER_EVENT = "tab-window-transfer";
const TAB_WINDOW_RELEASE_EVENT = "tab-window-release";
/** Sent to every window by a window that wants a dragged tab; whichever window holds the tab offers it. */
const TAB_WINDOW_CLAIM_EVENT = "tab-window-claim";
/** Sent to every window when a tab drag starts, so the windows it passes over can draw the tab. */
const TAB_WINDOW_DRAG_STARTED_EVENT = "tab-window-drag-started";
/** Sent to every window when that drag ends. */
const TAB_WINDOW_DRAG_ENDED_EVENT = "tab-window-drag-ended";
/** Sent by the native side with the label of a window it brought on screen. */
const WINDOW_RAISED_EVENT = "app-window-raised";
/** Drag data naming a tab another window can ask for. Only ids travel, so the tab leaves with its live location. */
export const TAB_WINDOW_DRAG_MIME = "application/x-ganbaru-ai-window-tab";
/** Where the pointer sits on a torn-off window, so it lands over that window's tab strip. */
const TEAR_OFF_GRAB_OFFSET = { x: 96, y: 16 } as const;
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

/** A tab dragged out of its window, as another window reads it on drop. */
export interface TabWindowDrag {
  sourceLabel: string;
  vaultId: string;
  tabId: string;
}

/**
 * Asks whichever window holds a dragged tab to offer it to `sourceLabel`, the window the tab is dragged over.
 * It is sent to every window because the tab may have moved on from the window the drag started in.
 */
export type TabWindowClaim = TabWindowDrag;

/** Where the pointer holds a dragged tab, from its top-left corner, and the tab's width, in pixels. */
export interface TabDragGrab {
  grabX: number;
  grabY: number;
  width: number;
}

/**
 * A tab another window is dragging, as every window hears of it when the drag starts. It only draws the tab;
 * the move itself still asks the source window for the tab with its live location.
 */
export interface TabWindowDragStarted extends TabDragGrab {
  sourceLabel: string;
  vaultId: string;
  tab: WorkspaceTab;
}

/** A point on the screen in logical pixels. */
export interface ScreenPoint {
  x: number;
  y: number;
}

/** How a tab drag ended: whether a window took the drop, and where on the screen it ended when known. */
export interface TabDragOutcome {
  accepted: boolean;
  screen: ScreenPoint | null;
}

/** A tab drag has ended, as `sourceLabel`, the window that saw it end, tells the others. */
export interface TabWindowDragEnded extends TabWindowDrag, TabDragOutcome {}

/** Create a unique label for a new tab window. */
export function createTabWindowLabel(createId: () => string = () => crypto.randomUUID()): string {
  return `${TAB_WINDOW_LABEL_PREFIX}${createId()}`;
}

/** Return whether a window label names a tab window other than the first one. */
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

function parseTabIdMessage(value: unknown): TabWindowRelease | null {
  const message = parseMessage(value);
  if (!message || !isRecord(value) || typeof value.tabId !== "string" || value.tabId.length === 0) return null;
  return { ...message, tabId: value.tabId };
}

/** Validate a release from another window. */
export function parseTabWindowRelease(value: unknown): TabWindowRelease | null {
  return parseTabIdMessage(value);
}

/** Validate a claim for a dragged tab from another window. Claims from `ownLabel` itself are dropped. */
export function parseTabWindowClaim(value: unknown, ownLabel: string): TabWindowClaim | null {
  return parseDragIdentity(value, ownLabel);
}

/** Serialize a dragged tab for the drag data. */
export function tabWindowDragData(drag: TabWindowDrag): string {
  return JSON.stringify(drag);
}

/** Read which tab of which window a drag carries, dropping drags from `ownLabel` itself. */
function parseDragIdentity(value: unknown, ownLabel: string): TabWindowDrag | null {
  if (!isRecord(value)) return null;
  const { sourceLabel, vaultId, tabId } = value;
  if (!isWindowLabel(sourceLabel) || sourceLabel === ownLabel) return null;
  if (typeof vaultId !== "string" || vaultId.length === 0) return null;
  if (typeof tabId !== "string" || tabId.length === 0) return null;
  return { sourceLabel, vaultId, tabId };
}

function isPixelLength(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

/** Validate a drag another window announced. Announcements from `ownLabel` itself are dropped. */
export function parseTabWindowDragStarted(
  value: unknown,
  availableKinds: readonly View[],
  ownLabel: string,
): TabWindowDragStarted | null {
  if (!isRecord(value)) return null;
  const { sourceLabel, vaultId, grabX, grabY, width } = value;
  if (!isWindowLabel(sourceLabel) || sourceLabel === ownLabel) return null;
  if (typeof vaultId !== "string" || vaultId.length === 0) return null;
  if (!isPixelLength(grabX) || !isPixelLength(grabY) || !isPixelLength(width)) return null;
  const tab = parseWorkspaceTab(value.tab, availableKinds);
  return tab ? { sourceLabel, vaultId, tab, grabX, grabY, width } : null;
}

function parseScreenPoint(value: unknown): ScreenPoint | null | undefined {
  if (value === null) return null;
  if (!isRecord(value)) return undefined;
  const { x, y } = value;
  return typeof x === "number" && Number.isFinite(x) && typeof y === "number" && Number.isFinite(y) ? { x, y } : undefined;
}

/** Validate the end of a drag another window announced. Announcements from `ownLabel` itself are dropped. */
export function parseTabWindowDragEnded(value: unknown, ownLabel: string): TabWindowDragEnded | null {
  const drag = parseDragIdentity(value, ownLabel);
  if (!drag || !isRecord(value) || typeof value.accepted !== "boolean") return null;
  const screen = parseScreenPoint(value.screen);
  return screen === undefined ? null : { ...drag, accepted: value.accepted, screen };
}

/** Where a drag ended, as its `dragend` event reports it. */
export interface DragEndPoint {
  clientX: number;
  clientY: number;
  screenX: number;
  screenY: number;
}

/**
 * Whether a tab drag ended outside its window: either `depth`, the count of document drag entries minus
 * exits during the drag, says it left, or the reported end point lies outside the viewport. Either signal
 * suffices because engines differ in which one they report reliably; some report the end point relative to
 * the window under the pointer, and some report zeros.
 */
export function dragEndedOutsideWindow(end: DragEndPoint, viewport: { width: number; height: number }, depth: number): boolean {
  if (depth <= 0) return true;
  const reported = [end.clientX, end.clientY, end.screenX, end.screenY].some((value) => Number.isFinite(value) && value !== 0);
  return reported
    && (end.clientX < 0 || end.clientY < 0 || end.clientX >= viewport.width || end.clientY >= viewport.height);
}

/**
 * Where a window torn off at a drag's end point opens, so the pointer lands over its tab strip. Engines
 * that report no position give zero for both coordinates; the window then opens centered.
 */
export function tearOffWindowPosition(screenX: number, screenY: number): ScreenPoint | null {
  if (!Number.isFinite(screenX) || !Number.isFinite(screenY) || (screenX === 0 && screenY === 0)) return null;
  return { x: Math.round(screenX - TEAR_OFF_GRAB_OFFSET.x), y: Math.round(screenY - TEAR_OFF_GRAB_OFFSET.y) };
}

/** Send a tab-window message. Every window receives it and only its target acts on it. */
async function sendTabWindowMessage(
  event: typeof TAB_WINDOW_TRANSFER_EVENT | typeof TAB_WINDOW_RELEASE_EVENT,
  message: TabWindowTransfer | TabWindowRelease,
): Promise<void> {
  await emit(event, message);
}

/**
 * Tell every window that a tab drag started, ended, or wants its tab here, so the windows it passes over can
 * draw and take the tab.
 */
async function announceTabDrag(
  event: typeof TAB_WINDOW_DRAG_STARTED_EVENT | typeof TAB_WINDOW_DRAG_ENDED_EVENT | typeof TAB_WINDOW_CLAIM_EVENT,
  message: TabWindowDragStarted | TabWindowDragEnded | TabWindowClaim,
): Promise<void> {
  await emit(event, message);
}

/** Listen for an app event. Returns a cleanup that also works before the listener finished registering. */
function listenEvent(event: string, listener: (payload: unknown) => void): () => void {
  let disposed = false;
  let cleanup: (() => void) | undefined;
  listen<unknown>(event, (received) => {
    listener(received.payload);
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

/**
 * Listen for one kind of tab-window message addressed to a window. Invalid payloads and messages for other
 * windows are dropped.
 */
function listenTabWindowMessages<T extends TabWindowMessage>(
  event: string,
  label: string,
  parse: (value: unknown) => T | null,
  listener: (message: T) => void,
): () => void {
  return listenEvent(event, (payload) => {
    const message = parse(payload);
    if (message && message.targetLabel === label) listener(message);
  });
}

async function focusExistingWindow(window: WebviewWindow): Promise<void> {
  await window.show();
  await window.unminimize();
  await window.setFocus();
}

/**
 * Count the other app windows on screen. Hidden windows do not count, so the window that hosts app-wide work
 * stays out of the count while it is hidden.
 */
async function countOtherOpenWindows(ownLabel: string): Promise<number> {
  const others = (await getAllWebviewWindows()).filter((window) => isWindowLabel(window.label) && window.label !== ownLabel);
  const visible = await Promise.all(others.map((window) => window.isVisible()));
  return visible.filter(Boolean).length;
}

/**
 * Open a tab window, or focus it when it is already open. A new window starts with `tab`, or with the
 * tabs it saved when it has none. A window opened without focus leaves the current one in front. It opens
 * at `position` when given, which window managers may ignore, and centered otherwise.
 */
async function openTabWindow(
  label: string,
  tab: WorkspaceTab | null,
  { focus = true, position = null }: { focus?: boolean; position?: ScreenPoint | null } = {},
): Promise<void> {
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
    ...(position ? { x: position.x, y: position.y } : { center: true }),
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
  /** Another window wants a dragged tab, which this window offers when it holds the tab. */
  claim(message: TabWindowClaim): void;
  /** Another window started dragging a tab. */
  dragStarted(message: TabWindowDragStarted): void;
  /** Another window saw a tab drag end. */
  dragEnded(message: TabWindowDragEnded): void;
  /** The app brought this window on screen, for example on a second launch or to open a notification. */
  raised(): void;
}

/** Desktop window operations that let workspace tabs move between this window and the others. */
export interface WorkspaceTabWindows {
  readonly label: string;
  /**
   * Whether this window hosts app-wide work. It keeps the tabs of a single-window setup, reopens the other
   * saved windows at launch, and hides instead of closing while other windows stay open.
   */
  readonly isHost: boolean;
  /** Open a new window that starts with `tab`, at `position` when given and centered otherwise. */
  open(tab: WorkspaceTab, position?: ScreenPoint | null): Promise<void>;
  /** Reopen a window with the tabs it saved, behind the current one. Labels that name no tab window are ignored. */
  reopen(label: string): Promise<void>;
  /** Offer a tab to another window, which adopts it and then releases it from this one. */
  offer(targetLabel: string, vaultId: string, tab: WorkspaceTab): Promise<void>;
  /** Confirm to the window that offered a tab that it now lives here. */
  release(targetLabel: string, vaultId: string, tabId: string): Promise<void>;
  /** Ask whichever window holds a dragged tab to offer it to this one. */
  claim(vaultId: string, tabId: string): Promise<void>;
  /** Tell every other window that this one started dragging `tab`, held at `grab`. */
  announceDrag(vaultId: string, tab: WorkspaceTab, grab: TabDragGrab): Promise<void>;
  /** Tell every other window that a tab drag ended, and how. */
  announceDragEnd(vaultId: string, tabId: string, outcome: TabDragOutcome): Promise<void>;
  /** Count the other app windows on screen, so closing this one can tell whether it is the last. */
  otherOpenWindows(): Promise<number>;
  /** Take this window off the screen without closing it, so a drag it started keeps running. */
  minimize(): Promise<void>;
  /** Listen for messages addressed to this window. Returns a cleanup. */
  listen(handlers: TabWindowHandlers): () => void;
}

/** Create the window operations of the window labelled `label`, accepting tabs of the available kinds. */
export function createWorkspaceTabWindows(label: string, availableKinds: readonly View[]): WorkspaceTabWindows {
  return {
    label,
    isHost: label === MAIN_WINDOW_LABEL,
    open: (tab, position = null) => openTabWindow(createTabWindowLabel(), tab, { position }),
    reopen: async (savedLabel) => {
      if (isTabWindowLabel(savedLabel)) await openTabWindow(savedLabel, null, { focus: false });
    },
    offer: (targetLabel, vaultId, tab) =>
      sendTabWindowMessage(TAB_WINDOW_TRANSFER_EVENT, { targetLabel, sourceLabel: label, vaultId, tab }),
    release: (targetLabel, vaultId, tabId) =>
      sendTabWindowMessage(TAB_WINDOW_RELEASE_EVENT, { targetLabel, sourceLabel: label, vaultId, tabId }),
    claim: (vaultId, tabId) => announceTabDrag(TAB_WINDOW_CLAIM_EVENT, { sourceLabel: label, vaultId, tabId }),
    announceDrag: (vaultId, tab, grab) =>
      announceTabDrag(TAB_WINDOW_DRAG_STARTED_EVENT, { sourceLabel: label, vaultId, tab, ...grab }),
    announceDragEnd: (vaultId, tabId, outcome) =>
      announceTabDrag(TAB_WINDOW_DRAG_ENDED_EVENT, { sourceLabel: label, vaultId, tabId, ...outcome }),
    otherOpenWindows: () => countOtherOpenWindows(label),
    minimize: () => getCurrentWebviewWindow().minimize(),
    listen(handlers) {
      const cleanups = [
        listenTabWindowMessages(
          TAB_WINDOW_TRANSFER_EVENT,
          label,
          (value) => parseTabWindowTransfer(value, availableKinds),
          handlers.transfer,
        ),
        listenTabWindowMessages(TAB_WINDOW_RELEASE_EVENT, label, parseTabWindowRelease, handlers.release),
        listenEvent(TAB_WINDOW_CLAIM_EVENT, (payload) => {
          const message = parseTabWindowClaim(payload, label);
          if (message) handlers.claim(message);
        }),
        listenEvent(TAB_WINDOW_DRAG_STARTED_EVENT, (payload) => {
          const message = parseTabWindowDragStarted(payload, availableKinds, label);
          if (message) handlers.dragStarted(message);
        }),
        listenEvent(TAB_WINDOW_DRAG_ENDED_EVENT, (payload) => {
          const message = parseTabWindowDragEnded(payload, label);
          if (message) handlers.dragEnded(message);
        }),
        listenEvent(WINDOW_RAISED_EVENT, (raisedLabel) => {
          if (raisedLabel === label) handlers.raised();
        }),
      ];
      return () => {
        for (const cleanup of cleanups) cleanup();
      };
    },
  };
}
