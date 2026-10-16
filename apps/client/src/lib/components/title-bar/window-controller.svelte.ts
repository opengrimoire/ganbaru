import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

type CurrentWindow = ReturnType<typeof getCurrentWindow>;

export interface TitleBarWindowControllerContext {
  window: CurrentWindow;
  /** Count the other app windows on screen, so closing the last one asks to quit the app instead. */
  otherOpenWindows: () => Promise<number>;
  /** Remember this window's tabs in the shared reopen history and stop saving them, right before it goes. */
  releaseWindow: () => void;
  /** Take this window off the screen: the host window hides so its app-wide work keeps running, others close. */
  dismissWindow: () => Promise<void>;
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

  async function otherWindowsOpen(): Promise<boolean> {
    try {
      return (await context.otherOpenWindows()) > 0;
    } catch (error) {
      context.reportError("Failed to list open windows:", error);
      return false;
    }
  }

  /** Close this window while others stay open, saving its pending edits first. */
  async function closeWindow(): Promise<void> {
    try {
      await context.beforeClose();
    } catch (error) {
      context.reportError("Failed to save pending edits before closing:", error);
      return;
    }
    context.releaseWindow();
    await context.dismissWindow();
  }

  /** Close this window, or ask to quit the app when it is the last one open. */
  async function requestClose(): Promise<void> {
    if (await otherWindowsOpen()) {
      await closeWindow();
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
      context.reportError("Failed to save pending edits before closing:", error);
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
