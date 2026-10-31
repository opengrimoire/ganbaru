interface DesktopReadinessOptions {
  mounted: Promise<unknown>;
  flushUpdates: () => Promise<void>;
  document: Document;
  revealMainWindow: (() => Promise<void>) | null;
  onError: (error: unknown) => void;
}

/**
 * Finishes desktop boot without relying on frames from a hidden WebView.
 * The main window is revealed only after mounted content and fonts are ready.
 * Failed boot leaves native fallback reveal responsible for window recovery.
 */
export async function finishDesktopReadiness(options: DesktopReadinessOptions): Promise<void> {
  try {
    await options.mounted;
    await options.flushUpdates();
    // Resolve styles and required fonts even when native visibility prevents paint.
    options.document.documentElement.getBoundingClientRect();
    await options.document.fonts.ready;
    await options.revealMainWindow?.();
  } catch (error: unknown) {
    options.onError(error);
  }
}
