import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DetachableTabView } from "$lib/navigation";
import {
  DETACHED_VIEW_WINDOW_CHANGED_EVENT,
  listDetachedViews,
  parseDetachedViewWindowChange,
  type DetachedViewWindowChange,
} from "$lib/windows/detached";

export type DetachedViewWindowChangeListener = (change: DetachedViewWindowChange) => void;

let initialized = false;
let eventUnlisten: UnlistenFn | undefined;
let detachedViews = $state<ReadonlySet<DetachableTabView>>(new Set());
const changeListeners = new Set<DetachedViewWindowChangeListener>();

function setDetachedViews(nextViews: ReadonlySet<DetachableTabView>): void {
  detachedViews = new Set(nextViews);
}

async function refresh(): Promise<void> {
  try {
    setDetachedViews(await listDetachedViews());
  } catch (error) {
    console.error("Failed to refresh detached windows:", error);
  }
}

/** Update the detached kinds, then tell listeners in the same task so they never observe a stale set. */
function applyChange(change: DetachedViewWindowChange): void {
  const nextViews = new Set(detachedViews);
  if (change.detached) {
    nextViews.add(change.view);
  } else {
    nextViews.delete(change.view);
  }
  detachedViews = nextViews;
  for (const listener of changeListeners) listener(change);
}

function init(): void {
  if (initialized) return;
  initialized = true;
  void refresh();
  listen<unknown>(DETACHED_VIEW_WINDOW_CHANGED_EVENT, (event) => {
    const change = parseDetachedViewWindowChange(event.payload);
    if (change) {
      applyChange(change);
    } else {
      void refresh();
    }
  }).then((unlisten) => {
    eventUnlisten = unlisten;
  }).catch((error) => {
    console.error("Failed to listen for detached window changes:", error);
  });

  if (typeof window !== "undefined") {
    window.addEventListener("focus", () => {
      void refresh();
    });
  }
}

export function getDetachedWindows() {
  init();
  return {
    get views(): ReadonlySet<DetachableTabView> {
      return detachedViews;
    },
    markDetached(view: DetachableTabView, tabId?: string): void {
      applyChange({ view, detached: true, ...(tabId ? { tabId } : {}) });
    },
    markAttached(view: DetachableTabView): void {
      applyChange({ view, detached: false });
    },
    /** Observe every window change, local or from another window. */
    onChange(listener: DetachedViewWindowChangeListener): () => void {
      changeListeners.add(listener);
      return () => changeListeners.delete(listener);
    },
    refresh,
    destroy(): void {
      eventUnlisten?.();
      eventUnlisten = undefined;
      initialized = false;
    },
  };
}
