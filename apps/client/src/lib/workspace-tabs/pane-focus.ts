import { getContext, setContext } from "svelte";
import { isAppKeyboardIsolated } from "$lib/modal-focus";

const WORKSPACE_PANE_FOCUS_KEY = Symbol("workspace-pane-focus");

/** Whether the pane a view renders in is the focused one, the pane window-wide shortcuts act on. */
export type WorkspacePaneFocus = () => boolean;

/** Tells the view rendered below whether its pane of a split view is the focused one. */
export function provideWorkspacePaneFocus(focused: WorkspacePaneFocus): void {
  setContext(WORKSPACE_PANE_FOCUS_KEY, focused);
}

/**
 * Returns whether the calling view's pane is focused. Outside a split view, as on phones, the view is always
 * focused. No pane is focused while a modal panel such as Quick notes owns the keyboard, so keys typed there never
 * reach the view behind it. Call during component initialization.
 */
export function getWorkspacePaneFocus(): WorkspacePaneFocus {
  const paneFocused = getContext<WorkspacePaneFocus | undefined>(WORKSPACE_PANE_FOCUS_KEY) ?? (() => true);
  return () => paneFocused() && !isAppKeyboardIsolated();
}
