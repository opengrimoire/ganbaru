import type { MobileThemeEditorComponent } from "./mobile-editor-loader-contracts";

/** Reject on desktop, where Settings use the title-bar-owned floating theme editor host. */
export function loadMobileThemeEditor(): Promise<MobileThemeEditorComponent> {
  return Promise.reject(new Error("The mobile theme editor host is unavailable on desktop."));
}

/** Desktop Settings have no mobile editor load to retry. */
export function retryMobileThemeEditor(): Promise<MobileThemeEditorComponent> {
  return loadMobileThemeEditor();
}
