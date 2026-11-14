import { describe, expect, it } from "vitest";
import type { ChatTimelineItemRead, ChatTimelinePageRead, ChatTimelineTurnRead, ChatTurnState } from "$lib/chat/contracts";
import {
  CHANGE_REQUEST_TITLE_MAX_CHARS,
  COMMIT_DRAFT_MAX_CHARS,
  draftReplyText,
  draftTurnOutcome,
  extractDraft,
  firstFencedBlock,
} from "./draft-extraction";

describe("draft extraction", () => {
  it("takes the first closed fenced block with any info string and CRLF line endings", () => {
    const reply = "Here it is:\r\n\r\n```text\r\nfeat(chat): add drafts\r\n\r\nBody line\r\n```\r\n\r\n```\r\nsecond\r\n```";
    expect(extractDraft(reply, "commit")).toEqual({ ok: true, kind: "commit", message: "feat(chat): add drafts\n\nBody line" });
    expect(firstFencedBlock("~~~~\nkeeps ``` inside\n~~~~")).toBe("keeps ``` inside");
    expect(firstFencedBlock("````\n```\nnested\n```\n````")).toBe("```\nnested\n```");
  });

  it("uses the whole reply when no fence closes", () => {
    expect(extractDraft("  fix: plain reply  \n", "commit")).toEqual({ ok: true, kind: "commit", message: "fix: plain reply" });
    expect(extractDraft("```\nfix: never closed", "commit")).toEqual({
      ok: true,
      kind: "commit",
      message: "```\nfix: never closed",
    });
  });

  it("refuses empty and oversized drafts instead of cutting them", () => {
    expect(extractDraft("```\n  \n```", "commit")).toEqual({ ok: false, reason: "empty" });
    expect(extractDraft("", "change_request")).toEqual({ ok: false, reason: "empty" });
    expect(extractDraft("x".repeat(COMMIT_DRAFT_MAX_CHARS), "commit").ok).toBe(true);
    expect(extractDraft("x".repeat(COMMIT_DRAFT_MAX_CHARS + 1), "commit")).toEqual({ ok: false, reason: "too_long" });
    expect(extractDraft("é".repeat(2_001), "commit")).toEqual({ ok: false, reason: "too_long" });
    expect(extractDraft(`${"t".repeat(CHANGE_REQUEST_TITLE_MAX_CHARS + 1)}\nbody`, "change_request"))
      .toEqual({ ok: false, reason: "title_too_long" });
  });

  it("splits a change request draft into its first line title and the remaining body", () => {
    expect(extractDraft("```md\nAdd drafts\n\nWhy it matters.\n\n- Detail\n```", "change_request")).toEqual({
      ok: true,
      kind: "change_request",
      title: "Add drafts",
      body: "Why it matters.\n\n- Detail",
    });
    expect(extractDraft("Title only", "change_request")).toEqual({ ok: true, kind: "change_request", title: "Title only", body: "" });
  });
});

describe("draft reply reading", () => {
  it("reads the latest final answer of the turn, else its latest assistant message", () => {
    const page = timelinePage([
      message("a", "turn-1", 1, "assistant", "commentary", "thinking"),
      message("b", "turn-1", 3, "assistant", "final_answer", "final"),
      message("c", "turn-1", 4, "assistant", "commentary", "after"),
      message("d", "turn-1", 2, "user", null, "prompt"),
      message("e", "turn-2", 5, "assistant", "final_answer", "other turn"),
    ], [turn("turn-1", "completed")]);
    expect(draftReplyText(page, "turn-1")).toBe("final");
    const withoutFinal = timelinePage([
      message("a", "turn-1", 1, "assistant", null, "first"),
      message("b", "turn-1", 2, "assistant", "commentary", "second"),
    ], []);
    expect(draftReplyText(withoutFinal, "turn-1")).toBe("second");
    expect(draftReplyText(withoutFinal, "turn-3")).toBeNull();
  });

  it("ignores assistant activity previews, which can be shortened", () => {
    const page = timelinePage([{
      activityId: "activity",
      turnId: "turn-1",
      sequenceAnchor: 1,
      changeSequence: 1,
      kind: "activity",
      data: { schemaVersion: 1, value: { activityKind: "assistant_message", detail: "cut", role: "assistant", markdown: "cut" } },
    }], []);
    expect(draftReplyText(page, "turn-1")).toBeNull();
  });

  it("reports only terminal turn states as an outcome", () => {
    expect(draftTurnOutcome(timelinePage([], [turn("turn-1", "active")]), "turn-1")).toBeNull();
    expect(draftTurnOutcome(timelinePage([], [turn("turn-1", "interrupted")]), "turn-1")).toBe("interrupted");
    expect(draftTurnOutcome(timelinePage([], [turn("turn-1", "completed")]), "turn-2")).toBeNull();
  });
});

function timelinePage(items: ChatTimelineItemRead[], turns: ChatTimelineTurnRead[]): ChatTimelinePageRead {
  return { threadId: "thread", items, turns, previousCursor: null, nextCursor: null, threadRevision: 1, changeCursor: 1 };
}

function message(
  id: string,
  turnId: string,
  sequence: number,
  role: "user" | "assistant",
  phase: "commentary" | "final_answer" | null,
  markdown: string,
): ChatTimelineItemRead {
  return {
    activityId: id,
    turnId,
    sequenceAnchor: sequence,
    changeSequence: sequence,
    kind: "message",
    data: { schemaVersion: 1, value: { role, markdown, streamingState: "complete", metadata: phase ? { phase } : null } },
  };
}

function turn(turnId: string, state: ChatTurnState): ChatTimelineTurnRead {
  return {
    turnId,
    state,
    startedAt: null,
    completedAt: null,
    stopReason: null,
    errorCode: null,
    modelId: null,
    modelOptions: [],
    modes: { safetyMode: "ask_for_approval", interactionMode: "build" },
    usage: null,
    changedFiles: [],
  };
}
