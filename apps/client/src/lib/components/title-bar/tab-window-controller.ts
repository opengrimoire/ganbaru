import type { WorkspaceTabs } from "$lib/stores/workspace-tabs.svelte";

export interface TitleBarTabWindowControllerContext {
  contextTabId: () => string | null;
  closeContextMenu: () => void;
  tabs: WorkspaceTabs;
  reportError: (message: string, error: unknown) => void;
}

/** Own the title bar actions that move a tab into another window. */
export function createTitleBarTabWindowController(context: TitleBarTabWindowControllerContext) {
  /** Run a move for the tab whose context menu is open, closing the menu first. */
  async function moveContextTab(move: (tabId: string) => Promise<void>, failure: string): Promise<void> {
    const tabId = context.contextTabId();
    context.closeContextMenu();
    if (!tabId) return;
    try {
      await move(tabId);
    } catch (error) {
      context.reportError(failure, error);
    }
  }

  return {
    moveToNewWindow(): Promise<void> {
      return moveContextTab((tabId) => context.tabs.detach(tabId), "Failed to move tab to a new window:");
    },
  };
}
