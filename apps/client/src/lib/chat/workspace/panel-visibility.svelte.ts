import { SvelteMap } from "svelte/reactivity";
import type { ChatChannelId } from "$lib/chat/contracts";

/** The workspace panels a channel can show beside and below its conversation. */
export type ChatWorkspacePanelPlacement = "inspector" | "bottom";

/** Which of a channel's workspace panels are open. */
export interface ChatWorkspacePanelVisibility {
  inspector: boolean;
  bottom: boolean;
}

const CLOSED: ChatWorkspacePanelVisibility = { inspector: false, bottom: false };

/** Channels kept with a panel open, matching the bound on the panels' own per-channel state. */
const MAX_CHAT_PANEL_VISIBILITY_CHANNELS = 64;

/**
 * Keeps whether each channel's secondary panel and bottom panel are open, in memory for this app session.
 *
 * A channel never seen, or one evicted from the bound, has both panels closed. Only channels with an open panel are
 * stored, so closing both panels forgets the channel.
 */
export class ChatPanelVisibilityState {
  private readonly channels = new SvelteMap<string, ChatWorkspacePanelVisibility>();

  /**
   * Reads which panels a channel shows.
   *
   * @param channelId The channel, or `null` when no channel is selected.
   * @returns The channel's panel visibility, closed for a channel without stored state.
   */
  read(channelId: ChatChannelId | null): ChatWorkspacePanelVisibility {
    return this.channels.get(channelKey(channelId)) ?? CLOSED;
  }

  /**
   * Reads whether one of a channel's panels is open.
   *
   * @param channelId The channel, or `null` when no channel is selected.
   * @param placement The panel to read.
   */
  isOpen(channelId: ChatChannelId | null, placement: ChatWorkspacePanelPlacement): boolean {
    return this.read(channelId)[placement];
  }

  /**
   * Opens or closes one of a channel's panels without touching the other.
   *
   * @param channelId The channel, or `null` when no channel is selected.
   * @param placement The panel to change.
   * @param open Whether the panel is open afterwards.
   */
  setOpen(channelId: ChatChannelId | null, placement: ChatWorkspacePanelPlacement, open: boolean): void {
    const key = channelKey(channelId);
    const current = this.channels.get(key) ?? CLOSED;
    if (current[placement] === open) return;
    const next = { ...current, [placement]: open };
    this.channels.delete(key);
    if (!next.inspector && !next.bottom) return;
    this.channels.set(key, next);
    while (this.channels.size > MAX_CHAT_PANEL_VISIBILITY_CHANNELS) {
      const oldest = this.channels.keys().next().value;
      if (oldest === undefined) break;
      this.channels.delete(oldest);
    }
  }

  /**
   * Flips one of a channel's panels.
   *
   * @param channelId The channel, or `null` when no channel is selected.
   * @param placement The panel to flip.
   */
  toggle(channelId: ChatChannelId | null, placement: ChatWorkspacePanelPlacement): void {
    this.setOpen(channelId, placement, !this.isOpen(channelId, placement));
  }

  /** Closes every channel's panels. */
  clear(): void {
    this.channels.clear();
  }
}

/** The state without a selected channel shares one slot, so the panels still open and close there. */
function channelKey(channelId: ChatChannelId | null): string {
  return channelId ?? "";
}

/** Panel visibility shared by every Chat pane, like the panels' per-channel tabs. */
export const chatPanelVisibility = new ChatPanelVisibilityState();
