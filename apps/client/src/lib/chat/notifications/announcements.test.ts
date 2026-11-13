import { describe, expect, it } from "vitest";
import { createBackgroundAnnouncementTracker, nextBackgroundAnnouncement } from "./announcements";
import type { ChatAttentionSignal } from "./gate";

function signal(assignmentId: string, overrides: Partial<ChatAttentionSignal> = {}): ChatAttentionSignal {
  return {
    assignmentId,
    revision: 1,
    kind: "answer",
    projectId: "project-1",
    channelId: "channel-1",
    replyThreadId: `thread-${assignmentId}`,
    teammateName: "Ada",
    channelName: "General",
    executedHere: true,
    stateReason: null,
    updatedAt: "2026-11-12T10:00:00Z",
    ...overrides,
  };
}

describe("nextBackgroundAnnouncement", () => {
  it("ignores changes of the selected reply thread", () => {
    expect(nextBackgroundAnnouncement([], [signal("a")], "thread-a")).toBeNull();
  });

  it("ignores signals already present in the previous snapshot", () => {
    expect(nextBackgroundAnnouncement([signal("a")], [signal("a")], null)).toBeNull();
    expect(nextBackgroundAnnouncement([], [], null)).toBeNull();
  });

  it("announces a new revision of work already in the queue", () => {
    const next = signal("a", { revision: 2, kind: "approval" });
    expect(nextBackgroundAnnouncement([signal("a")], [next], null)).toEqual(next);
  });

  it("picks the newest of several changes", () => {
    const older = signal("a", { updatedAt: "2026-11-12T10:00:00Z" });
    const newest = signal("b", { kind: "failure", updatedAt: "2026-11-12T10:05:00Z" });
    const middle = signal("c", { kind: "review", updatedAt: "2026-11-12T10:02:00Z" });
    const selected = signal("d", { updatedAt: "2026-11-12T10:09:00Z" });

    expect(nextBackgroundAnnouncement([], [older, newest, middle, selected], "thread-d")).toEqual(newest);
  });
});

describe("createBackgroundAnnouncementTracker", () => {
  const speaking = { quiet: false, enabled: true, selectedReplyThreadId: null };

  it("keeps the first snapshot silent and speaks a later change", () => {
    const tracker = createBackgroundAnnouncementTracker();
    expect(tracker.observe([signal("a")])).toBe(false);
    expect(tracker.decide(speaking)).toBeNull();
    expect(tracker.observe([signal("a"), signal("b")])).toBe(true);
    expect(tracker.decide(speaking)?.assignmentId).toBe("b");
    expect(tracker.decide(speaking)).toBeNull();
  });

  it("starts a new silent baseline after the queue unloads", () => {
    const tracker = createBackgroundAnnouncementTracker();
    tracker.observe([signal("a")]);
    expect(tracker.observe(null)).toBe(false);
    expect(tracker.observe([signal("b")])).toBe(false);
    expect(tracker.decide(speaking)).toBeNull();
  });

  it("holds changes while quiet and speaks the newest once quiet ends", () => {
    const tracker = createBackgroundAnnouncementTracker();
    tracker.observe([]);
    tracker.observe([signal("a", { updatedAt: "2026-11-12T10:00:00Z" })]);
    expect(tracker.decide({ ...speaking, quiet: true })).toBeNull();
    tracker.observe([
      signal("a", { updatedAt: "2026-11-12T10:00:00Z" }),
      signal("b", { updatedAt: "2026-11-12T10:03:00Z" }),
    ]);
    expect(tracker.decide({ ...speaking, quiet: true })).toBeNull();
    expect(tracker.decide(speaking)?.assignmentId).toBe("b");
  });

  it("advances the baseline without speaking when announcements are off", () => {
    const tracker = createBackgroundAnnouncementTracker();
    tracker.observe([]);
    tracker.observe([signal("a")]);
    expect(tracker.decide({ ...speaking, enabled: false })).toBeNull();
    expect(tracker.decide(speaking)).toBeNull();
  });
});
