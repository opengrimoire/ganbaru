import type { Component } from "svelte";
import type { ChatMessageRead, ChatScheduledMessageRead } from "$lib/chat/contracts";
import type { TimelineMessageRow } from "./model";

const ORGANIZATIONAL_REACTION_PREFIX = "organizational:";
const EXECUTION_REACTION_PREFIX = "timeline:";
const SCHEDULED_REACTION_PREFIX = "scheduled:";

export type ChatMessageActionTargetKind = "organizational" | "execution" | "scheduled";

export interface ChatMessageActionTarget {
  kind: ChatMessageActionTargetKind;
  sourceId: string;
  reactionKey: string;
  copyText: string;
}

/** A toolbar button that a surface supplies beyond the shared reaction, copy, and more actions. */
export interface ChatMessageCustomAction {
  /** Stable identifier, also exposed as `data-message-action` for tests and styling. */
  id: string;
  /** Accessible name and tooltip. */
  label: string;
  icon: Component<{ size?: number }>;
  disabled?: boolean;
  onSelect: () => void;
}

/** One toolbar entry: a shared action by name, or a surface's own button. */
export type ChatMessageToolbarAction = "reaction" | "copy" | "more" | ChatMessageCustomAction;

/** The actions of a posted or provider message, in toolbar order. */
export const DEFAULT_MESSAGE_TOOLBAR_ACTIONS: readonly ChatMessageToolbarAction[] = ["reaction", "copy", "more"];

/** Creates the shared action target for a durable organizational message. */
export function organizationalMessageActionTarget(
  message: Pick<ChatMessageRead, "itemId" | "normalizedMarkdown">,
): ChatMessageActionTarget {
  return {
    kind: "organizational",
    sourceId: message.itemId,
    reactionKey: `${ORGANIZATIONAL_REACTION_PREFIX}${message.itemId}`,
    copyText: message.normalizedMarkdown,
  };
}

/** Creates the shared action target for a provider execution message. */
export function executionMessageActionTarget(
  message: Pick<TimelineMessageRow, "id" | "markdown">,
): ChatMessageActionTarget {
  return {
    kind: "execution",
    sourceId: message.id,
    reactionKey: `${EXECUTION_REACTION_PREFIX}${message.id}`,
    copyText: message.markdown,
  };
}

/** Creates the shared action target for a message waiting to be sent. */
export function scheduledMessageActionTarget(
  message: Pick<ChatScheduledMessageRead, "id" | "normalizedMarkdown">,
): ChatMessageActionTarget {
  return {
    kind: "scheduled",
    sourceId: message.id,
    reactionKey: `${SCHEDULED_REACTION_PREFIX}${message.id}`,
    copyText: message.normalizedMarkdown,
  };
}
