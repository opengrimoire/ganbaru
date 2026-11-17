import { describe, expect, it } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import { suggestAgentMention } from "./mention-suggestion";

const ada = { id: "participant:ada", displayName: "Ada" };
const grace = { id: "participant:grace", displayName: "Grace Hopper" };

function mention(participantId: string, startOffset: number, label: string): ChatMessageReference {
  return {
    kind: "participant",
    participantId,
    participantKind: "ai_agent",
    metadata: {
      referenceId: `reference:${participantId}`,
      labelSnapshot: label,
      startOffset,
      endOffset: startOffset + label.length + 1,
      plainTextProjection: `@${label}`,
    },
  };
}

describe("suggestAgentMention", () => {
  it("suggests an agent named as a whole word in any case with the range of the name", () => {
    expect(suggestAgentMention("could ada check this?", [], [ada])).toEqual({
      participantId: "participant:ada",
      displayName: "Ada",
      start: 6,
      end: 9,
    });
  });

  it("does not suggest an agent who is already mentioned", () => {
    const text = "@Ada and Ada again";
    expect(suggestAgentMention(text, [mention("participant:ada", 0, "Ada")], [ada])).toBeNull();
  });

  it("ignores a name inside another reference or typed right after @", () => {
    const text = "@Ada Lovelace notes";
    const other = mention("participant:lovelace", 0, "Ada Lovelace");
    expect(suggestAgentMention(text, [other], [ada])).toBeNull();
    expect(suggestAgentMention("@Ada", [], [ada])).toBeNull();
  });

  it("does not match part of a word or an unfinished name", () => {
    expect(suggestAgentMention("Adam is here", [], [ada])).toBeNull();
    expect(suggestAgentMention("ask Ad", [], [ada])).toBeNull();
    expect(suggestAgentMention("Canada_Ada", [], [ada])).toBeNull();
  });

  it("matches a name with spaces and accented neighbors only on word boundaries", () => {
    expect(suggestAgentMention("thanks grace hopper.", [], [grace])?.start).toBe(7);
    expect(suggestAgentMention("\u{E9}Ada", [], [ada])).toBeNull();
  });

  it("picks the agent named first when the draft names two", () => {
    const text = "Grace Hopper should review what Ada wrote";
    expect(suggestAgentMention(text, [], [ada, grace])?.participantId).toBe("participant:grace");
    expect(suggestAgentMention("Ada then Grace Hopper", [], [grace, ada])?.participantId).toBe("participant:ada");
  });

  it("suggests the second agent once the first is mentioned", () => {
    const text = "@Ada and Grace Hopper";
    expect(suggestAgentMention(text, [mention("participant:ada", 0, "Ada")], [ada, grace])?.participantId)
      .toBe("participant:grace");
  });
});
