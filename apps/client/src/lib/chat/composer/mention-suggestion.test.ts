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
  it("suggests an agent named as a whole word with the range of the name", () => {
    expect(suggestAgentMention("could Ada check this?", [], [ada])).toEqual({
      participantId: "participant:ada",
      displayName: "Ada",
      start: 6,
      end: 9,
    });
  });

  it("matches a name only when it keeps its case", () => {
    const sol = { id: "participant:sol", displayName: "Sol" };
    expect(suggestAgentMention("el sol sale", [], [sol])).toBeNull();
    expect(suggestAgentMention("ask Sol about it", [], [sol])).toEqual({
      participantId: "participant:sol",
      displayName: "Sol",
      start: 4,
      end: 7,
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
    expect(suggestAgentMention("thanks Grace Hopper.", [], [grace])?.start).toBe(7);
    expect(suggestAgentMention("\u{E9}Ada", [], [ada])).toBeNull();
  });

  it("picks the agent named first when the draft names two", () => {
    const text = "Grace Hopper should review what Ada wrote";
    expect(suggestAgentMention(text, [], [ada, grace])?.participantId).toBe("participant:grace");
    expect(suggestAgentMention("Ada then Grace Hopper", [], [grace, ada])?.participantId).toBe("participant:ada");
  });

  it("does not read a shorter version inside a longer one and prefers the longest name", () => {
    const opus5 = { id: "participant:opus-5", displayName: "Opus 5" };
    const opus55 = { id: "participant:opus-5-5", displayName: "Opus 5.5" };
    expect(suggestAgentMention("ask Opus 5.5 now", [], [opus5])).toBeNull();
    expect(suggestAgentMention("ask Opus 5.5 now", [], [opus5, opus55])?.participantId).toBe("participant:opus-5-5");
    expect(suggestAgentMention("Opus 5. Then rest", [], [opus5])?.end).toBe(6);
  });

  it("suggests the second agent once the first is mentioned", () => {
    const text = "@Ada and Grace Hopper";
    expect(suggestAgentMention(text, [mention("participant:ada", 0, "Ada")], [ada, grace])?.participantId)
      .toBe("participant:grace");
  });
});
