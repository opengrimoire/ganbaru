import type { ChatMessageRead } from "$lib/chat/contracts";
import type { TimelineMessageRow } from "./model";

const ORGANIZATIONAL_REACTION_PREFIX = "organizational:";
const EXECUTION_REACTION_PREFIX = "timeline:";

export type ChatMessageActionTargetKind = "organizational" | "execution";

export interface ChatMessageActionTarget {
  kind: ChatMessageActionTargetKind;
  sourceId: string;
  reactionKey: string;
  copyText: string;
}

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
