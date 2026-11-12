import { describe, expect, it } from "vitest";
import type { ChatMessageRead, ChatParticipantRead, JsonValue } from "$lib/chat/contracts";
import { answerableHostQuestion, chatAgentUpdatePresentation, isHostToolUpdate } from "./agent-update";

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

const person: ChatParticipantRead = {
  id: "participant:local",
  kind: "local_user",
  displayName: "You",
  avatar: { schemaVersion: 1, value: {} },
  revision: 1,
  archivedAt: null,
};
const teammate: ChatParticipantRead = { ...person, id: "participant:atlas", kind: "ai_teammate", displayName: "Atlas" };

function reply(
  itemId: string,
  orderKey: string,
  richContent: ChatMessageRead["richContent"],
  author: ChatParticipantRead = teammate,
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
      reply("item:update", "b", content({}), teammate),
    ], waiting)).toEqual({ messageItemId: "item:question", choices: ["Yes"] });
  });

  it("ignores questions that belong to other work", () => {
    expect(answerableHostQuestion([reply("item:other", "a", hostQuestion(["Yes"], "assignment:2"))], waiting)).toBeNull();
  });
});
