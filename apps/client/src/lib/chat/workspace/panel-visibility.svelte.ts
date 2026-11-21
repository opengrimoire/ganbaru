import { SvelteMap } from "svelte/reactivity";
import type { ChatChannelId, ChatReplyThreadId } from "$lib/chat/contracts";

/** The workspace panels a channel can show beside and below its conversation. */
export type ChatWorkspacePanelPlacement = "inspector" | "bottom";

/**
 * The owner of one set of Chat workspace panels: a channel, or a reply thread shown in a workspace tab of its own.
 * Build it with {@link chatPanelScope}.
 */
export type ChatPanelScope = string;

/** Which of a panel scope's workspace panels are open. */
export interface ChatWorkspacePanelVisibility {
  inspector: boolean;
  bottom: boolean;
}

const CLOSED: ChatWorkspacePanelVisibility = { inspector: false, bottom: false };

/** Panel scopes kept with a panel open, matching the bound on the panels' own per-scope state. */
const MAX_CHAT_PANEL_VISIBILITY_SCOPES = 64;

/**
 * Names the panels a Chat view shows: the channel's own, or those of a reply thread shown in its own tab.
 *
 * A reply thread tab keeps panels apart from its channel's while still resolving folders through that channel. The
 * separator is a control character, which identifiers never contain, so a thread scope never equals a channel ID.
 *
 * @param channelId The selected channel, or `null` when none is selected.
 * @param replyThreadTabId The reply thread shown on its own, or `null` when the view shows the channel.
 * @returns The panel scope, or `null` when no channel is selected.
 */
export function chatPanelScope(
  channelId: ChatChannelId | null,
  replyThreadTabId: ChatReplyThreadId | null,
): ChatPanelScope | null {
  if (channelId === null) return null;
  return replyThreadTabId === null ? channelId : `${channelId}\n${replyThreadTabId}`;
}

/**
 * Keeps whether each panel scope's secondary panel and bottom panel are open, in memory for this app session.
 *
 * A scope never seen, or one evicted from the bound, has both panels closed. Only scopes with an open panel are
 * stored, so closing both panels forgets the scope.
 */
export class ChatPanelVisibilityState {
  private readonly scopes = new SvelteMap<string, ChatWorkspacePanelVisibility>();

  /**
   * Reads which panels a scope shows.
   *
   * @param scope The panel scope, or `null` when no channel is selected.
   * @returns The scope's panel visibility, closed for a scope without stored state.
   */
  read(scope: ChatPanelScope | null): ChatWorkspacePanelVisibility {
    return this.scopes.get(scopeKey(scope)) ?? CLOSED;
  }

  /**
   * Reads whether one of a scope's panels is open.
   *
   * @param scope The panel scope, or `null` when no channel is selected.
   * @param placement The panel to read.
   */
  isOpen(scope: ChatPanelScope | null, placement: ChatWorkspacePanelPlacement): boolean {
    return this.read(scope)[placement];
  }

  /**
   * Opens or closes one of a scope's panels without touching the other.
   *
   * @param scope The panel scope, or `null` when no channel is selected.
   * @param placement The panel to change.
   * @param open Whether the panel is open afterwards.
   */
  setOpen(scope: ChatPanelScope | null, placement: ChatWorkspacePanelPlacement, open: boolean): void {
    const key = scopeKey(scope);
    const current = this.scopes.get(key) ?? CLOSED;
    if (current[placement] === open) return;
    const next = { ...current, [placement]: open };
    this.scopes.delete(key);
    if (!next.inspector && !next.bottom) return;
    this.scopes.set(key, next);
    while (this.scopes.size > MAX_CHAT_PANEL_VISIBILITY_SCOPES) {
      const oldest = this.scopes.keys().next().value;
      if (oldest === undefined) break;
      this.scopes.delete(oldest);
    }
  }

  /**
   * Flips one of a scope's panels.
   *
   * @param scope The panel scope, or `null` when no channel is selected.
   * @param placement The panel to flip.
   */
  toggle(scope: ChatPanelScope | null, placement: ChatWorkspacePanelPlacement): void {
    this.setOpen(scope, placement, !this.isOpen(scope, placement));
  }

  /** Closes every scope's panels. */
  clear(): void {
    this.scopes.clear();
  }
}

/** The state without a selected channel shares one slot, so the panels still open and close there. */
function scopeKey(scope: ChatPanelScope | null): string {
  return scope ?? "";
}

/** Panel visibility shared by every Chat pane, like the panels' per-scope tabs. */
export const chatPanelVisibility = new ChatPanelVisibilityState();
