import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";
import { getWorkspacePaneScope } from "$lib/workspace-tabs/pane-scope";

const targets = new Map<WorkspacePaneScope, EventTarget>();

/**
 * The target for Chat interface events, such as opening a file or stopping a turn, within one split pane, so two
 * Chat tabs shown side by side each handle only their own.
 */
export function chatPaneEvents(scope: WorkspacePaneScope): EventTarget {
  let target = targets.get(scope);
  if (!target) {
    target = new EventTarget();
    targets.set(scope, target);
  }
  return target;
}

/** The Chat event target of the calling view's split pane. Call during component initialization. */
export function useChatPaneEvents(): EventTarget {
  return chatPaneEvents(getWorkspacePaneScope());
}
