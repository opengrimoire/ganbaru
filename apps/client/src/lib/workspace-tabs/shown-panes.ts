import type { View } from "$lib/navigation";
import type { WorkspacePaneScope } from "./model";

/** A tab shown in the content area and the view that shows it. */
export interface WorkspaceShownPane {
  tabId: string;
  view: View;
  /** The set of view stores the view shows through, which tells apart two panes of one view. */
  scope: WorkspacePaneScope;
  /** Whether a Projects tab hosts the view, so it shows beside that project's sidebar. */
  hosted: boolean;
}

/** A tab on screen, by id and kind. */
export interface WorkspaceShownTab {
  id: string;
  kind: View;
}

/** Where the content area finds what each shown tab shows through. */
export interface WorkspaceShownPaneSources {
  /** The pane scope a shown tab reads its stores from. */
  scopeOf: (tabId: string) => WorkspacePaneScope;
  /** The workspace the Projects store of a scope hosts in place of a task view, or null for a task view. */
  hostedViewIn: (scope: WorkspacePaneScope) => View | null;
}

/** How a tab shows: a Projects tab shows the workspace its own pane's Projects store hosts. */
function shownPane(tab: WorkspaceShownTab, sources: WorkspaceShownPaneSources): WorkspaceShownPane {
  const scope = sources.scopeOf(tab.id);
  const hosted = tab.kind === "projects" ? sources.hostedViewIn(scope) : null;
  return { tabId: tab.id, view: hosted ?? tab.kind, scope, hosted: hosted !== null };
}

/**
 * The tabs the content area shows in split order, each with the view and stores that show it. One view shows
 * twice only through two sets of stores.
 */
export function workspaceShownPanes(
  active: WorkspaceShownTab,
  partner: WorkspaceShownTab | null,
  partnerFirst: boolean,
  sources: WorkspaceShownPaneSources,
): WorkspaceShownPane[] {
  const shown = shownPane(active, sources);
  if (!partner) return [shown];
  const beside = shownPane(partner, sources);
  if (beside.view === shown.view && beside.scope === shown.scope) return [shown];
  return partnerFirst ? [beside, shown] : [shown, beside];
}
