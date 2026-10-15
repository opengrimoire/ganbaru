import { emit } from "@tauri-apps/api/event";
import { WebviewWindow, getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import type { DetachableTabView } from "$lib/navigation";
import { isDetachableTabView, viewLabel } from "$lib/navigation";
import { readNativeBackground } from "$lib/themes/native-background";
import { parseWorkspaceTabLocation, type WorkspaceTabLocation } from "$lib/workspace-tabs/model";

export const DETACHED_VIEW_WINDOW_PREFIX = "view";
export const DETACHED_VIEW_WINDOW_CHANGED_EVENT = "detached-view-window-changed";
export const DETACHED_VIEW_REATTACH_REQUESTED_EVENT = "detached-view-reattach-requested";
export const DETACHED_VIEW_OPEN_LOCATION_EVENT = "detached-view-open-location";
export const DETACHED_VIEW_DRAG_MIME = "application/x-ganbaru-ai-detached-view";
/** Surface the desktop shell paints first, used as the native background of a new detached window. */
const DETACHED_WINDOW_BACKGROUND_TOKEN = "--sidebar";

/** The workspace tab a detached window shows, so the main window can take it back where it was. */
export interface DetachedViewWindowTab {
  tabId: string;
  location: WorkspaceTabLocation;
}

export interface DetachedViewWindowChange {
  view: DetachableTabView;
  detached: boolean;
  tabId?: string;
  location?: WorkspaceTabLocation;
  /** The window closed its tab instead of moving it back. */
  closed?: boolean;
}

export interface DetachedViewOpenLocationRequest {
  view: DetachableTabView;
  location: WorkspaceTabLocation;
}

export interface DetachedViewReattachRequest {
  view: DetachableTabView;
}

export interface DetachedViewDragPayload {
  view: DetachableTabView;
  sourceLabel: string;
}

export function detachedViewWindowLabel(view: DetachableTabView): string {
  return `${DETACHED_VIEW_WINDOW_PREFIX}-${view}`;
}

export function detachableTabViewFromWindowLabel(label: string): DetachableTabView | undefined {
  const prefix = `${DETACHED_VIEW_WINDOW_PREFIX}-`;
  if (!label.startsWith(prefix)) return undefined;
  const view = label.slice(prefix.length);
  return isDetachableTabView(view) ? view : undefined;
}

export function detachedViewWindowTitle(view: DetachableTabView): string {
  return `Ganbaru AI: ${viewLabel(view)}`;
}

export function detachedViewWindowUrl(view: DetachableTabView, tab?: DetachedViewWindowTab): string {
  const params = new URLSearchParams({ view });
  if (tab) {
    params.set("tab", tab.tabId);
    params.set("location", JSON.stringify(tab.location));
  }
  return `/?${params.toString()}`;
}

/** Read the tab a detached window was opened with, when it matches the window's kind. */
export function parseDetachedViewWindowTab(
  search: string,
  view: DetachableTabView,
): DetachedViewWindowTab | null {
  const params = new URLSearchParams(search);
  const tabId = params.get("tab");
  const rawLocation = params.get("location");
  if (!tabId || !rawLocation) return null;
  const location = parseWorkspaceTabLocation(parseJson(rawLocation));
  return location && location.kind === view ? { tabId, location } : null;
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

/** Validate a window change event, dropping tab details that do not belong to its kind. */
export function parseDetachedViewWindowChange(value: unknown): DetachedViewWindowChange | null {
  if (!isRecord(value) || !isDetachableTabView(value.view) || typeof value.detached !== "boolean") return null;
  const change: DetachedViewWindowChange = { view: value.view, detached: value.detached };
  if (typeof value.tabId === "string" && value.tabId.length > 0) change.tabId = value.tabId;
  const location = value.location === undefined ? null : parseWorkspaceTabLocation(value.location);
  if (location && location.kind === value.view) change.location = location;
  if (value.closed === true) change.closed = true;
  return change;
}

export function parseDetachedViewOpenLocationRequest(value: unknown): DetachedViewOpenLocationRequest | null {
  if (!isRecord(value) || !isDetachableTabView(value.view)) return null;
  const location = parseWorkspaceTabLocation(value.location);
  return location && location.kind === value.view ? { view: value.view, location } : null;
}

export function serializeDetachedViewDragPayload(
  view: DetachableTabView,
  sourceLabel: string,
): string {
  return JSON.stringify({ view, sourceLabel });
}

export function parseDetachedViewDragPayload(
  value: string,
): DetachedViewDragPayload | undefined {
  try {
    const parsed = JSON.parse(value) as unknown;
    if (typeof parsed !== "object" || parsed === null) return undefined;
    const record = parsed as Record<string, unknown>;
    if (!isDetachableTabView(record.view)) return undefined;
    if (typeof record.sourceLabel !== "string") return undefined;
    if (detachableTabViewFromWindowLabel(record.sourceLabel) !== record.view) return undefined;
    return {
      view: record.view,
      sourceLabel: record.sourceLabel,
    };
  } catch {
    return undefined;
  }
}

export function isDetachedViewReattachRequest(
  value: unknown,
): value is DetachedViewReattachRequest {
  return isRecord(value) && isDetachableTabView(value.view);
}

async function focusExistingWindow(window: WebviewWindow): Promise<void> {
  await window.show();
  await window.unminimize();
  await window.setFocus();
}

export async function focusMainWindow(): Promise<void> {
  const mainWindow = await WebviewWindow.getByLabel("main");
  if (!mainWindow) return;
  await focusExistingWindow(mainWindow);
}

export async function listDetachedViews(): Promise<Set<DetachableTabView>> {
  const windows = await getAllWebviewWindows();
  const views = new Set<DetachableTabView>();
  for (const window of windows) {
    const view = detachableTabViewFromWindowLabel(window.label);
    if (view) views.add(view);
  }
  return views;
}

export async function notifyDetachedViewWindowChanged(
  change: DetachedViewWindowChange,
): Promise<void> {
  await emit(DETACHED_VIEW_WINDOW_CHANGED_EVENT, change);
}

export async function notifyDetachedViewReattachRequested(
  request: DetachedViewReattachRequest,
): Promise<void> {
  await emit(DETACHED_VIEW_REATTACH_REQUESTED_EVENT, request);
}

/** Show a location in the window that holds its kind, and bring that window forward. */
export async function openLocationInDetachedViewWindow(
  view: DetachableTabView,
  location: WorkspaceTabLocation | null,
): Promise<void> {
  const existing = await WebviewWindow.getByLabel(detachedViewWindowLabel(view));
  if (!existing) return;
  if (location) {
    const request: DetachedViewOpenLocationRequest = { view, location };
    await emit(DETACHED_VIEW_OPEN_LOCATION_EVENT, request);
  }
  await focusExistingWindow(existing);
}

/** Open a kind in its own window, carrying the moved tab's identity and location when there is one. */
export async function openDetachedViewWindow(
  view: DetachableTabView,
  tab?: DetachedViewWindowTab,
): Promise<void> {
  const label = detachedViewWindowLabel(view);
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await focusExistingWindow(existing);
    return;
  }

  const background = readNativeBackground(document.documentElement, DETACHED_WINDOW_BACKGROUND_TOKEN);
  const backgroundColor: [number, number, number] | undefined = background
    ? [background.r, background.g, background.b]
    : undefined;
  const created = new WebviewWindow(label, {
    ...(backgroundColor ? { backgroundColor } : {}),
    url: detachedViewWindowUrl(view, tab),
    title: detachedViewWindowTitle(view),
    width: 960,
    height: 720,
    minWidth: 280,
    minHeight: 180,
    center: true,
    decorations: false,
    visible: true,
    focus: true,
  });

  await new Promise<void>((resolve, reject) => {
    void created.once<void>("tauri://created", () => resolve());
    void created.once<unknown>("tauri://error", (event) => {
      reject(event.payload);
    });
  });

  try {
    await notifyDetachedViewWindowChanged({ view, detached: true, ...(tab ? { tabId: tab.tabId } : {}) });
  } catch (error) {
    console.error("Failed to publish detached window open:", error);
  }
}
