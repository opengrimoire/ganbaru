import type { ChatChannelId, ChatThreadId } from "$lib/chat/contracts";
import type { ChatWindowChanges } from "$lib/chat/window-sync";

/** A pane's Chat store as the other panes of its window reach it. */
export interface ChatPaneStore {
  /** Reread Chat records another pane of the same window changed. */
  refreshFromOtherPane(changes: ChatWindowChanges): void;
}

/** Chat threads and channels a change touched, in the shape callers name them. */
export interface ChatChangeIds {
  threadIds?: Iterable<ChatThreadId>;
  channelIds?: Iterable<ChatChannelId>;
}

/** Collect changed ids once, so every receiver reads the same sets. */
export function chatWindowChanges(changes: ChatChangeIds): ChatWindowChanges {
  return { threadIds: new Set(changes.threadIds ?? []), channelIds: new Set(changes.channelIds ?? []) };
}

/**
 * The Chat stores of one window's panes. A change made through one pane reaches the others here, since the
 * change channel between windows does not deliver to the window that published.
 */
export class ChatPaneStores<T extends ChatPaneStore> {
  private readonly stores: T[] = [];

  add(store: T): void {
    if (!this.stores.includes(store)) this.stores.push(store);
  }

  /** Every pane's store, in the order they were created. */
  all(): readonly T[] {
    return this.stores;
  }

  /** Tell every pane except `origin` about the records it changed. Empty changes reach no one. */
  notifyOthers(origin: T, changes: ChatWindowChanges): void {
    if (changes.threadIds.size === 0 && changes.channelIds.size === 0) return;
    for (const store of this.stores) {
      if (store !== origin) store.refreshFromOtherPane(changes);
    }
  }
}
