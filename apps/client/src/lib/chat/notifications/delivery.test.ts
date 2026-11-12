import { describe, expect, it } from "vitest";
import type { ChatChannelId, ChatReplyThreadId, ChatWorkAssignmentId, UtcTimestamp } from "$lib/chat/contracts";
import { chatNotificationText, parseChatNotificationTarget, type ChatNotificationLabels } from "./delivery";
import type { ChatAttentionKind, ChatAttentionSignal } from "./gate";

const labels: ChatNotificationLabels = {
  title: (kind, teammate) => `${kind}:${teammate}`,
  body: (channel) => `in ${channel}`,
  bodyWithReason: (channel, reason) => `in ${channel}: ${reason}`,
  summaryTitle: (count) => `${count} waiting`,
  summaryCount: (kind, count) => `${count} ${kind}`,
  list: (values) => values.join(" + "),
  openThread: "Open thread",
  openList: "Open list",
};

function signal(kind: ChatAttentionKind, overrides: Partial<ChatAttentionSignal> = {}): ChatAttentionSignal {
  return {
    assignmentId: `assignment-${kind}` as ChatWorkAssignmentId,
    revision: 1,
    kind,
    projectId: "project-1",
    channelId: "channel-1" as ChatChannelId,
    replyThreadId: `reply-${kind}` as ChatReplyThreadId,
    teammateName: "Ada",
    channelName: "release",
    executedHere: true,
    stateReason: "Tests failed in parser.rs",
    updatedAt: "2026-11-12T10:00:00Z" as UtcTimestamp,
    ...overrides,
  };
}

describe("chat notification text", () => {
  it("names the teammate and channel and opens the reply thread of a single change", () => {
    const text = chatNotificationText({ kind: "single", signal: signal("approval") }, true, labels);
    expect(text).toEqual({
      title: "approval:Ada",
      body: "in release",
      openLabel: "Open thread",
      target: {
        kind: "assignment",
        projectId: "project-1",
        channelId: "channel-1",
        replyThreadId: "reply-approval",
        assignmentId: "assignment-approval",
      },
    });
  });

  it("shows a failure reason only when the device allows content", () => {
    const failure = { kind: "single", signal: signal("failure") } as const;
    expect(chatNotificationText(failure, false, labels).body).toBe("in release");
    expect(chatNotificationText(failure, true, labels).body).toBe("in release: Tests failed in parser.rs");
    const blank = { kind: "single", signal: signal("failure", { stateReason: "   " }) } as const;
    expect(chatNotificationText(blank, true, labels).body).toBe("in release");
  });

  it("counts each nonzero kind in a summary that opens the review queue", () => {
    const signals = [signal("review"), signal("answer"), signal("review", { assignmentId: "other" as ChatWorkAssignmentId })];
    const text = chatNotificationText({
      kind: "summary",
      counts: { answer: 1, approval: 0, review: 2, failure: 0 },
      signals,
    }, true, labels);
    expect(text).toEqual({
      title: "3 waiting",
      body: "1 answer + 2 review",
      openLabel: "Open list",
      target: { kind: "reviewQueue" },
    });
  });
});

describe("chat notification target parsing", () => {
  it("accepts both target shapes and drops unknown fields", () => {
    expect(parseChatNotificationTarget({ kind: "reviewQueue", extra: 1 })).toEqual({ kind: "reviewQueue" });
    expect(parseChatNotificationTarget({
      kind: "assignment",
      projectId: "p",
      channelId: "c",
      replyThreadId: "r",
      assignmentId: "a",
    })).toEqual({ kind: "assignment", projectId: "p", channelId: "c", replyThreadId: "r", assignmentId: "a" });
  });

  it("refuses unknown kinds and assignments with missing or empty identifiers", () => {
    expect(() => parseChatNotificationTarget({ kind: "calendar" })).toThrow();
    expect(() => parseChatNotificationTarget(null)).toThrow();
    expect(() => parseChatNotificationTarget({ kind: "assignment", projectId: "p", channelId: "c", replyThreadId: "r" }))
      .toThrow();
    expect(() => parseChatNotificationTarget({
      kind: "assignment",
      projectId: "",
      channelId: "c",
      replyThreadId: "r",
      assignmentId: "a",
    })).toThrow();
  });
});
