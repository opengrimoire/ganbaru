import { getContext, setContext } from "svelte";
import type { WorkspacePaneScope } from "./model";

const WORKSPACE_PANE_SCOPE_KEY = Symbol("workspace-pane-scope");

/** Tells the view rendered below which set of view stores its pane shows through. */
export function provideWorkspacePaneScope(scope: () => WorkspacePaneScope): void {
  setContext(WORKSPACE_PANE_SCOPE_KEY, scope);
}

/**
 * Returns the set of view stores the calling view's pane shows through. Outside a split view, as on phones and in
 * settings, views use the primary stores. Call during component initialization.
 */
export function getWorkspacePaneScope(): WorkspacePaneScope {
  return getContext<(() => WorkspacePaneScope) | undefined>(WORKSPACE_PANE_SCOPE_KEY)?.() ?? "primary";
}
