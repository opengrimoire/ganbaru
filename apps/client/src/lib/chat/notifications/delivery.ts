import type { ChatChannelId, ChatReplyThreadId, ChatWorkAssignmentId } from "$lib/chat/contracts";
import { readEnum, readIdentifier, readRecord } from "$lib/chat/validation/readers";
import type { ChatAttentionKind, ChatAttentionSignal, ChatNotificationDelivery } from "./gate";

const CHAT_NOTIFICATION_TARGET_KINDS = ["assignment", "reviewQueue"] as const;
const SUMMARY_KIND_ORDER: readonly ChatAttentionKind[] = ["answer", "approval", "review", "failure"];

/** Where an opened Chat notification leads, as `show_chat_notification` takes it and `chat-notification-open` returns it. */
export type ChatNotificationTarget =
  | {
    kind: "assignment";
    projectId: string;
    channelId: ChatChannelId;
    replyThreadId: ChatReplyThreadId;
    assignmentId: ChatWorkAssignmentId;
  }
  | { kind: "reviewQueue" };

/** The localized text and target of one system notification. */
export interface ChatNotificationText {
  title: string;
  body: string;
  openLabel: string;
  target: ChatNotificationTarget;
}

/** Catalog text the notification builder needs, injected so the builder stays pure. */
export interface ChatNotificationLabels {
  title(kind: ChatAttentionKind, agent: string): string;
  body(channel: string): string;
  bodyWithReason(channel: string, reason: string): string;
  summaryTitle(count: number): string;
  summaryCount(kind: ChatAttentionKind, count: number): string;
  list(values: string[]): string;
  openThread: string;
  openList: string;
}

function singleBody(signal: ChatAttentionSignal, includeContent: boolean, labels: ChatNotificationLabels): string {
  const reason = signal.stateReason?.trim();
  return includeContent && signal.kind === "failure" && reason
    ? labels.bodyWithReason(signal.channelName, reason)
    : labels.body(signal.channelName);
}

/**
 * Builds the text of a gate delivery. A single notification names the agent, kind, and channel and opens its
 * reply thread; a summary counts each kind and opens the review queue. Content beyond names appears only when the
 * device allows it, and then only as the failure reason.
 *
 * @param delivery What the gate released.
 * @param includeContent The device preference that allows failure reasons in notification text.
 * @param labels Localized catalog text.
 */
export function chatNotificationText(
  delivery: ChatNotificationDelivery,
  includeContent: boolean,
  labels: ChatNotificationLabels,
): ChatNotificationText {
  if (delivery.kind === "single") {
    const { signal } = delivery;
    return {
      title: labels.title(signal.kind, signal.agentName),
      body: singleBody(signal, includeContent, labels),
      openLabel: labels.openThread,
      target: {
        kind: "assignment",
        projectId: signal.projectId,
        channelId: signal.channelId,
        replyThreadId: signal.replyThreadId,
        assignmentId: signal.assignmentId,
      },
    };
  }
  const parts = SUMMARY_KIND_ORDER
    .filter((kind) => delivery.counts[kind] > 0)
    .map((kind) => labels.summaryCount(kind, delivery.counts[kind]));
  return {
    title: labels.summaryTitle(delivery.signals.length),
    body: labels.list(parts),
    openLabel: labels.openList,
    target: { kind: "reviewQueue" },
  };
}

/**
 * Validates the payload of `chat-notification-open`.
 *
 * @param value The untrusted event payload.
 */
export function parseChatNotificationTarget(value: unknown, label = "Chat notification target"): ChatNotificationTarget {
  const record = readRecord(value, label);
  const kind = readEnum(record.kind, CHAT_NOTIFICATION_TARGET_KINDS, `${label}.kind`);
  if (kind === "reviewQueue") return { kind };
  return {
    kind,
    projectId: readIdentifier(record.projectId, `${label}.projectId`),
    channelId: readIdentifier(record.channelId, `${label}.channelId`),
    replyThreadId: readIdentifier(record.replyThreadId, `${label}.replyThreadId`),
    assignmentId: readIdentifier(record.assignmentId, `${label}.assignmentId`),
  };
}
