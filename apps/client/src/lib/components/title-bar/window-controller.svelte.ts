import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { DetachableTabView } from "$lib/navigation";
import { notifyDetachedViewWindowChanged } from "$lib/windows/detached";

type CurrentWindow = ReturnType<typeof getCurrentWindow>;

export interface TitleBarWindowControllerContext {
  window: CurrentWindow;
  detachedWindowView: DetachableTabView | undefined;
  markDetachedViewAttached: (view: DetachableTabView) => void;
  benchmarkLocked: () => boolean;
  resetConfirmationOpen: () => boolean;
  requestCloseConfirmation: () => void;
  closeConfirmation: () => void;
  ensureBenchmarkOverlay: () => Promise<void>;
  beforeClose: () => Promise<void>;
  themeEditOpen: () => boolean;
  cancelThemeEdit: () => Promise<void>;
  reportError: (message: string, error: unknown) => void;
}

/** Own native title-bar window observation and close coordination. */
export function createTitleBarWindowController(
  context: TitleBarWindowControllerContext,
) {
  let isMaximized = $state(false);

  async function benchmarkBlocksClose(): Promise<boolean> {
    if (context.benchmarkLocked()) return true;
    try {
      return (await invoke<string | null>("read_benchmark_state")) !== null;
    } catch {
      return false;
    }
  }

  async function requestClose(): Promise<void> {
    if (context.detachedWindowView) {
      try {
        await context.beforeClose();
      } catch (error) {
        context.reportError("Failed to save Quick notes before closing:", error);
        return;
      }
      context.markDetachedViewAttached(context.detachedWindowView);
      try {
        await notifyDetachedViewWindowChanged({
          view: context.detachedWindowView,
          detached: false,
        });
      } catch (error) {
        context.reportError("Failed to publish detached window close:", error);
      }
      await context.window.destroy();
      return;
    }
    if (context.resetConfirmationOpen()) return;
    if (await benchmarkBlocksClose()) {
      void context.ensureBenchmarkOverlay();
      return;
    }
    if (context.resetConfirmationOpen()) return;
    context.requestCloseConfirmation();
  }

  async function confirmClose(): Promise<void> {
    if (context.benchmarkLocked()) {
      context.closeConfirmation();
      void context.ensureBenchmarkOverlay();
      return;
    }
    try {
      await context.beforeClose();
    } catch (error) {
      context.reportError("Failed to save Quick notes before closing:", error);
      return;
    }
    context.closeConfirmation();
    if (context.themeEditOpen()) void context.cancelThemeEdit();
    void invoke("force_quit");
  }

  $effect(() => {
    void context.window.isMaximized().then((value) => { isMaximized = value; });
    let cleanupResize: (() => void) | undefined;
    let cleanupClose: (() => void) | undefined;
    void context.window.onResized(() => {
      void context.window.isMaximized().then((value) => { isMaximized = value; });
    }).then((unlisten) => { cleanupResize = unlisten; });
    void context.window.onCloseRequested((event) => {
      event.preventDefault();
      void requestClose();
    }).then((unlisten) => { cleanupClose = unlisten; });
    return () => {
      cleanupResize?.();
      cleanupClose?.();
    };
  });

  return {
    get isMaximized(): boolean { return isMaximized; },
    requestClose,
    confirmClose,
  };
}
