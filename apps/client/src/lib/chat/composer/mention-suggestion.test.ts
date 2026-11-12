import { describe, expect, it } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import { suggestTeammateMention } from "./mention-suggestion";

const ada = { id: "participant:ada", displayName: "Ada" };
const grace = { id: "participant:grace", displayName: "Grace Hopper" };

function mention(participantId: string, startOffset: number, label: string): ChatMessageReference {
  return {
    kind: "participant",
    participantId,
    participantKind: "ai_teammate",
    metadata: {
      referenceId: `reference:${participantId}`,
      labelSnapshot: label,
      startOffset,
      endOffset: startOffset + label.length + 1,
      plainTextProjection: `@${label}`,
    },
  };
}

describe("suggestTeammateMention", () => {
  it("suggests a teammate named as a whole word in any case with the range of the name", () => {
    expect(suggestTeammateMention("could ada check this?", [], [ada])).toEqual({
      participantId: "participant:ada",
      displayName: "Ada",
      start: 6,
      end: 9,
    });
  });

  it("does not suggest a teammate who is already mentioned", () => {
    const text = "@Ada and Ada again";
    expect(suggestTeammateMention(text, [mention("participant:ada", 0, "Ada")], [ada])).toBeNull();
  });

  it("ignores a name inside another reference or typed right after @", () => {
    const text = "@Ada Lovelace notes";
    const other = mention("participant:lovelace", 0, "Ada Lovelace");
    expect(suggestTeammateMention(text, [other], [ada])).toBeNull();
    expect(suggestTeammateMention("@Ada", [], [ada])).toBeNull();
  });

  it("does not match part of a word or an unfinished name", () => {
    expect(suggestTeammateMention("Adam is here", [], [ada])).toBeNull();
    expect(suggestTeammateMention("ask Ad", [], [ada])).toBeNull();
    expect(suggestTeammateMention("Canada_Ada", [], [ada])).toBeNull();
  });

  it("matches a name with spaces and accented neighbors only on word boundaries", () => {
    expect(suggestTeammateMention("thanks grace hopper.", [], [grace])?.start).toBe(7);
    expect(suggestTeammateMention("\u{E9}Ada", [], [ada])).toBeNull();
  });

  it("picks the teammate named first when the draft names two", () => {
    const text = "Grace Hopper should review what Ada wrote";
    expect(suggestTeammateMention(text, [], [ada, grace])?.participantId).toBe("participant:grace");
    expect(suggestTeammateMention("Ada then Grace Hopper", [], [grace, ada])?.participantId).toBe("participant:ada");
  });

  it("suggests the second teammate once the first is mentioned", () => {
    const text = "@Ada and Grace Hopper";
    expect(suggestTeammateMention(text, [mention("participant:ada", 0, "Ada")], [ada, grace])?.participantId)
      .toBe("participant:grace");
  });
});
