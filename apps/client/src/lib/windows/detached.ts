import { emit } from "@tauri-apps/api/event";
import { WebviewWindow, getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import type { DetachableTabView } from "$lib/navigation";
import { isDetachableTabView, viewLabel } from "$lib/navigation";
import { readNativeBackground } from "$lib/themes/native-background";

export const DETACHED_VIEW_WINDOW_PREFIX = "view";
export const DETACHED_VIEW_WINDOW_CHANGED_EVENT = "detached-view-window-changed";
export const DETACHED_VIEW_REATTACH_REQUESTED_EVENT = "detached-view-reattach-requested";
export const DETACHED_VIEW_DRAG_MIME = "application/x-ganbaru-ai-detached-view";
/** Surface the desktop shell paints first, used as the native background of a new detached window. */
const DETACHED_WINDOW_BACKGROUND_TOKEN = "--sidebar";

export interface DetachedViewWindowChange {
  view: DetachableTabView;
  detached: boolean;
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

export function detachedViewWindowUrl(view: DetachableTabView): string {
  return `/?view=${encodeURIComponent(view)}`;
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
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return isDetachableTabView(record.view);
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

export async function openDetachedViewWindow(view: DetachableTabView): Promise<void> {
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
    url: detachedViewWindowUrl(view),
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
    await notifyDetachedViewWindowChanged({ view, detached: true });
  } catch (error) {
    console.error("Failed to publish detached window open:", error);
  }
}
