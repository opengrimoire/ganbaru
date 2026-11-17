import { describe, expect, it } from "vitest";
import type { ChatMessageRead, ChatParticipantRead, JsonValue } from "$lib/chat/contracts";
import {
  answerableHostQuestion,
  chatAgentUpdatePresentation,
  chatFailureTechnicalDetail,
  isHostToolUpdate,
} from "./agent-update";

function content(value: JsonValue): ChatMessageRead["richContent"] {
  return { schemaVersion: 1, value };
}

function hostQuestion(choices: JsonValue, assignmentId = "assignment:1"): ChatMessageRead["richContent"] {
  return content({
    type: "agent_update",
    updateKind: "question",
    assignmentId,
    agentRunId: "run:1",
    payload: { question: "Which?", choices, source: "host_tool" },
  });
}

const pushTarget = {
  workingFolderId: "folder:1",
  executionEnvironmentId: null,
  localBranch: "topic",
  remote: "origin",
  destinationBranch: "topic",
  pushUrl: "ssh://git@example.invalid/org/repo.git",
  objectId: "a".repeat(40),
  gitConfigDigest: "b".repeat(64),
};

function pushRequest(push: JsonValue, assignmentId = "assignment:1"): ChatMessageRead["richContent"] {
  return content({
    type: "agent_update",
    updateKind: "question",
    assignmentId,
    agentRunId: "run:1",
    payload: { question: "Push the fix", choices: [], source: "host_tool", push },
  });
}

const person: ChatParticipantRead = {
  id: "participant:local",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};
const agent: ChatParticipantRead = { ...person, id: "participant:atlas", kind: "ai_agent", displayName: "Atlas" };

function reply(
  itemId: string,
  orderKey: string,
  richContent: ChatMessageRead["richContent"],
  author: ChatParticipantRead = agent,
): Pick<ChatMessageRead, "itemId" | "orderKey" | "richContent" | "author"> {
  return { itemId, orderKey, richContent, author };
}

describe("chatAgentUpdatePresentation", () => {
  it("reads a host question with its distinct, trimmed choices", () => {
    expect(chatAgentUpdatePresentation(hostQuestion([" Keep ", "Drop", "Keep", "", 4]))).toEqual({
      kind: "host_question",
      assignmentId: "assignment:1",
      choices: ["Keep", "Drop"],
    });
    expect(chatAgentUpdatePresentation(hostQuestion(null))).toEqual({
      kind: "host_question",
      assignmentId: "assignment:1",
      choices: [],
    });
  });

  it("reads host reports and ignores provider updates of the same kind", () => {
    for (const reportKind of ["plan", "replan", "result"]) {
      expect(chatAgentUpdatePresentation(content({
        type: "agent_update",
        updateKind: reportKind,
        assignmentId: "assignment:1",
        agentRunId: "run:1",
        payload: { kind: reportKind, source: "host_tool" },
      }))).toEqual({ kind: "host_report", assignmentId: "assignment:1", reportKind });
    }
    const providerResult = content({
      type: "agent_update",
      updateKind: "result",
      assignmentId: "assignment:1",
      agentRunId: "run:1",
      payload: { kind: "result" },
    });
    expect(chatAgentUpdatePresentation(providerResult)).toBeNull();
    expect(isHostToolUpdate(providerResult)).toBe(false);
    expect(isHostToolUpdate(hostQuestion([]))).toBe(true);
  });

  it("reads a settled delegation outcome and refuses unknown states or phases", () => {
    const outcome = (payload: JsonValue) => content({
      type: "agent_update",
      updateKind: "delegation",
      assignmentId: "assignment:parent",
      agentRunId: null,
      payload,
    });
    const settled = { phase: "settled", childAssignmentId: "assignment:child", parentAssignmentId: "assignment:parent", state: "failed" };

    expect(chatAgentUpdatePresentation(outcome(settled))).toEqual({
      kind: "delegation_outcome",
      parentAssignmentId: "assignment:parent",
      childAssignmentId: "assignment:child",
      state: "failed",
    });
    expect(chatAgentUpdatePresentation(outcome({ ...settled, state: "exploded" }))).toBeNull();
    expect(chatAgentUpdatePresentation(outcome({ ...settled, phase: "started" }))).toBeNull();
    expect(isHostToolUpdate(outcome(settled))).toBe(false);
  });

  it("reads a host note proposal and refuses provider or mismatched ones", () => {
    const proposal = (payload: JsonValue, assignmentId: JsonValue = "assignment:1") => content({
      type: "agent_update",
      updateKind: "note_proposal",
      assignmentId,
      agentRunId: "run:1",
      payload,
    });
    const valid = { kind: "note_proposal", source: "host_tool" };

    expect(chatAgentUpdatePresentation(proposal(valid))).toEqual({
      kind: "note_proposal",
      assignmentId: "assignment:1",
    });
    expect(isHostToolUpdate(proposal(valid))).toBe(true);
    expect(chatAgentUpdatePresentation(proposal({ kind: "note_proposal" }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({ ...valid, kind: "plan" }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal(valid, " "))).toBeNull();
  });

  it("reads a host schedule proposal and refuses malformed or provider proposals", () => {
    const recurrence = {
      rule: { kind: "fixed_time", localMinute: 540, weekdays: 31 },
      timeZone: "Europe/Madrid",
      quietPolicy: "defer",
    };
    const proposal = (payload: JsonValue) => content({
      type: "agent_update",
      updateKind: "schedule_proposal",
      assignmentId: "assignment:1",
      agentRunId: "run:1",
      payload,
    });
    const valid = {
      message: " Post the standup summary ",
      firstAt: "2026-11-13T08:00:00Z",
      recurrence,
      reason: " Mornings ",
      source: "host_tool",
    };

    expect(chatAgentUpdatePresentation(proposal(valid))).toEqual({
      kind: "schedule_proposal",
      assignmentId: "assignment:1",
      proposal: {
        message: "Post the standup summary",
        firstAt: "2026-11-13T08:00:00Z",
        recurrence,
        reason: "Mornings",
      },
    });
    expect(chatAgentUpdatePresentation(proposal({ ...valid, recurrence: null }))).toMatchObject({
      proposal: { recurrence: null },
    });
    expect(isHostToolUpdate(proposal(valid))).toBe(true);
    expect(chatAgentUpdatePresentation(proposal({ ...valid, source: "provider" }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({ ...valid, message: "  " }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({ ...valid, firstAt: "tomorrow" }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({ ...valid, reason: "x".repeat(501) }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({
      ...valid,
      recurrence: { ...recurrence, rule: { kind: "interval", minutes: 5 } },
    }))).toBeNull();
    expect(chatAgentUpdatePresentation(proposal({
      message: valid.message,
      firstAt: valid.firstAt,
      reason: valid.reason,
      source: valid.source,
    }))).toBeNull();
  });

  it("reads a host push request and refuses a malformed or credential-bearing target", () => {
    expect(chatAgentUpdatePresentation(pushRequest(pushTarget))).toEqual({
      kind: "push_request",
      assignmentId: "assignment:1",
      push: pushTarget,
    });
    expect(chatAgentUpdatePresentation(pushRequest({ ...pushTarget, objectId: "HEAD" }))).toBeNull();
    expect(chatAgentUpdatePresentation(pushRequest({ ...pushTarget, pushUrl: "https://me:secret@example.invalid/r" }))).toBeNull();
    expect(chatAgentUpdatePresentation(pushRequest({ ...pushTarget, gitConfigDigest: "B".repeat(64) }))).toBeNull();
    expect(chatAgentUpdatePresentation(pushRequest(null))).toBeNull();
    const { executionEnvironmentId: _omitted, ...withoutEnvironment } = pushTarget;
    expect(chatAgentUpdatePresentation(pushRequest(withoutEnvironment))).toBeNull();
  });

  it("returns null for plain messages and malformed rich content", () => {
    expect(chatAgentUpdatePresentation(content({}))).toBeNull();
    expect(chatAgentUpdatePresentation(content([]))).toBeNull();
    expect(chatAgentUpdatePresentation(content({ type: "agent_update", updateKind: "question", payload: "text" }))).toBeNull();
    expect(chatAgentUpdatePresentation(content({
      type: "agent_update",
      updateKind: "question",
      assignmentId: "",
      payload: { source: "host_tool", choices: ["Yes"] },
    }))).toBeNull();
  });
});

describe("answerableHostQuestion", () => {
  const waiting = { id: "assignment:1", state: "waiting_for_answer" } as const;

  it("offers the newest question's choices while the work waits for an answer", () => {
    const replies = [
      reply("item:new", "b", hostQuestion(["Yes", "No"])),
      reply("item:old", "a", hostQuestion(["Old"])),
    ];

    expect(answerableHostQuestion(replies, waiting)).toEqual({ messageItemId: "item:new", choices: ["Yes", "No"] });
    expect(answerableHostQuestion(replies, { ...waiting, state: "working" })).toBeNull();
    expect(answerableHostQuestion(replies, null)).toBeNull();
  });

  it("offers nothing once a person replied after the question or when it has no choices", () => {
    expect(answerableHostQuestion([
      reply("item:question", "a", hostQuestion(["Yes"])),
      reply("item:answer", "b", content({}), person),
    ], waiting)).toBeNull();
    expect(answerableHostQuestion([reply("item:question", "a", hostQuestion([]))], waiting)).toBeNull();
    expect(answerableHostQuestion([
      reply("item:question", "a", hostQuestion(["Yes"])),
      reply("item:update", "b", content({}), agent),
    ], waiting)).toEqual({ messageItemId: "item:question", choices: ["Yes"] });
  });

  it("offers nothing after a newer push request for the same work", () => {
    expect(answerableHostQuestion([
      reply("item:question", "a", hostQuestion(["Yes"])),
      reply("item:push", "b", pushRequest(pushTarget)),
    ], waiting)).toBeNull();
    expect(answerableHostQuestion([
      reply("item:question", "a", hostQuestion(["Yes"])),
      reply("item:push", "b", pushRequest(pushTarget, "assignment:2")),
    ], waiting)).toEqual({ messageItemId: "item:question", choices: ["Yes"] });
  });

  it("ignores questions that belong to other work", () => {
    expect(answerableHostQuestion([reply("item:other", "a", hostQuestion(["Yes"], "assignment:2"))], waiting)).toBeNull();
  });
});

describe("failure updates", () => {
  function failure(payload: JsonValue, agentRunId: string | null = "run:1"): ChatMessageRead["richContent"] {
    return content({ type: "agent_update", updateKind: "failure", assignmentId: "assignment:1", agentRunId, payload });
  }

  function cause(value: ChatMessageRead["richContent"]) {
    const presentation = chatAgentUpdatePresentation(value);
    return presentation?.kind === "failure" ? presentation.cause : null;
  }

  it("reads a runtime error with its stored code", () => {
    expect(chatAgentUpdatePresentation(failure({ code: "timeout", safeDetails: null }))).toEqual({
      kind: "failure",
      assignmentId: "assignment:1",
      agentRunId: "run:1",
      cause: { kind: "error", code: "timeout" },
      code: "timeout",
    });
  });

  it("reads launch failures and provider limits", () => {
    expect(cause(failure({ launchFailure: true }, null))).toEqual({ kind: "launch" });
    expect(cause(failure({ code: "provider_usage_limit", resetsAt: null }))).toEqual({ kind: "provider_limit" });
  });

  it("falls back to an unknown cause for codes and payloads this build does not know", () => {
    expect(chatAgentUpdatePresentation(failure({ code: "from_a_newer_build" })))
      .toMatchObject({ kind: "failure", cause: { kind: "unknown" }, code: "from_a_newer_build" });
    expect(chatAgentUpdatePresentation(failure({ recoverable: true })))
      .toMatchObject({ kind: "failure", cause: { kind: "unknown" }, code: null });
  });

  it("needs the work the failure belongs to", () => {
    expect(chatAgentUpdatePresentation(content({
      type: "agent_update",
      updateKind: "failure",
      agentRunId: "run:1",
      payload: { code: "timeout" },
    }))).toBeNull();
  });
});

describe("chatFailureTechnicalDetail", () => {
  it("joins the stored code and the diagnostic text", () => {
    expect(chatFailureTechnicalDetail("timeout", "  Provider turn timed out after 600s\n"))
      .toBe("timeout: Provider turn timed out after 600s");
  });

  it("keeps whichever part exists", () => {
    expect(chatFailureTechnicalDetail("timeout", null)).toBe("timeout");
    expect(chatFailureTechnicalDetail("timeout", "timeout")).toBe("timeout");
    expect(chatFailureTechnicalDetail(null, "Process exited with status 1")).toBe("Process exited with status 1");
    expect(chatFailureTechnicalDetail(null, "   ")).toBeNull();
  });
});
