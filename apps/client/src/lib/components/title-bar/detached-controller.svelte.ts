import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  canDetachMainView,
  firstMainView,
  type DetachableTabView,
  type View,
} from "$lib/navigation";
import {
  DETACHED_VIEW_DRAG_MIME,
  DETACHED_VIEW_REATTACH_REQUESTED_EVENT,
  focusMainWindow,
  isDetachedViewReattachRequest,
  notifyDetachedViewReattachRequested,
  notifyDetachedViewWindowChanged,
  openDetachedViewWindow,
  parseDetachedViewDragPayload,
} from "$lib/windows/detached";

type CurrentWindow = ReturnType<typeof getCurrentWindow>;

export interface TitleBarDetachedControllerContext {
  window: CurrentWindow;
  isMainWindow: boolean;
  detachedWindowView: DetachableTabView | undefined;
  contextView: () => DetachableTabView | null;
  closeContextMenu: () => void;
  currentView: () => View;
  navigate: (view: View) => void;
  detachedViews: () => ReadonlySet<DetachableTabView>;
  markAttached: (view: DetachableTabView) => void;
  markDetached: (view: DetachableTabView) => void;
  reportError: (message: string, error: unknown) => void;
}

/** Own detached-tab transfers, reattach events, and window switching. */
export function createTitleBarDetachedController(
  context: TitleBarDetachedControllerContext,
) {
  function dragPayload(event: DragEvent) {
    const transfer = event.dataTransfer;
    if (!transfer) return undefined;
    const customPayload = transfer.getData(DETACHED_VIEW_DRAG_MIME);
    if (customPayload) return parseDetachedViewDragPayload(customPayload);
    const plainPayload = transfer.getData("text/plain");
    return plainPayload ? parseDetachedViewDragPayload(plainPayload) : undefined;
  }

  function hasDragPayload(event: DragEvent): boolean {
    return Array.from(event.dataTransfer?.types ?? []).includes(DETACHED_VIEW_DRAG_MIME);
  }

  function handleDragOver(event: DragEvent): void {
    if (!context.isMainWindow || !hasDragPayload(event)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
  }

  function handleDrop(event: DragEvent): void {
    if (!context.isMainWindow) return;
    const payload = dragPayload(event);
    if (!payload) return;
    event.preventDefault();
    event.stopPropagation();
    void requestReattach(payload.view);
  }

  async function moveToNewWindow(): Promise<void> {
    const view = context.contextView();
    if (!view || !canDetachContextView()) return;
    context.closeContextMenu();
    try {
      await openDetachedViewWindow(view);
      context.markDetached(view);
      if (context.currentView() === view) {
        context.navigate(firstMainView(context.detachedViews()));
      }
    } catch (error) {
      context.reportError("Failed to move tab to a new window:", error);
    }
  }

  async function requestReattach(view: DetachableTabView): Promise<void> {
    context.markAttached(view);
    context.navigate(view);
    try {
      await notifyDetachedViewWindowChanged({ view, detached: false });
    } catch (error) {
      context.reportError("Failed to publish detached window restore:", error);
    }
    try {
      await notifyDetachedViewReattachRequested({ view });
    } catch (error) {
      context.reportError("Failed to request detached window restore:", error);
    }
  }

  async function reattachCurrentWindow(): Promise<void> {
    const view = context.detachedWindowView;
    if (!view) return;
    context.markAttached(view);
    try {
      await notifyDetachedViewWindowChanged({ view, detached: false });
    } catch (error) {
      context.reportError("Failed to publish detached window restore:", error);
    }
    try {
      await focusMainWindow();
    } catch (error) {
      context.reportError("Failed to focus main window after tab restore:", error);
    }
    await context.window.destroy();
  }

  function activateContextAction(): void {
    if (context.detachedWindowView) {
      context.closeContextMenu();
      void reattachCurrentWindow();
    } else {
      void moveToNewWindow();
    }
  }

  function canDetachContextView(): boolean {
    return Boolean(
      context.isMainWindow
      && context.contextView()
      && canDetachMainView(context.detachedViews()),
    );
  }

  $effect(() => {
    if (!context.detachedWindowView) return;
    let cleanup: UnlistenFn | undefined;
    void listen<unknown>(DETACHED_VIEW_REATTACH_REQUESTED_EVENT, (event) => {
      if (
        isDetachedViewReattachRequest(event.payload)
        && event.payload.view === context.detachedWindowView
      ) {
        void reattachCurrentWindow();
      }
    }).then((unlisten) => { cleanup = unlisten; }).catch((error) => {
      context.reportError("Failed to listen for detached tab reattach:", error);
    });
    return () => cleanup?.();
  });

  return { activateContextAction, canDetachContextView, handleDragOver, handleDrop };
}
