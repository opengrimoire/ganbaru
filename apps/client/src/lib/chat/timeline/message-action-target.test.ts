import { describe, expect, it } from "vitest";
import {
  executionMessageActionTarget,
  organizationalMessageActionTarget,
} from "./message-action-target";

describe("Chat message action targets", () => {
  it("keeps organizational and execution reactions separate for the same source id", () => {
    const organizational = organizationalMessageActionTarget({
      itemId: "message-1",
      normalizedMarkdown: "Organizational message",
    });
    const execution = executionMessageActionTarget({
      id: "message-1",
      markdown: "Execution answer",
    });

    expect(organizational).toEqual({
      kind: "organizational",
      sourceId: "message-1",
      reactionKey: "organizational:message-1",
      copyText: "Organizational message",
    });
    expect(execution).toEqual({
      kind: "execution",
      sourceId: "message-1",
      reactionKey: "timeline:message-1",
      copyText: "Execution answer",
    });
    expect(organizational.reactionKey).not.toBe(execution.reactionKey);
  });
});
