import { describe, expect, it } from "vitest";
import type { ChatMessageReference } from "$lib/chat/contracts";
import { agentsToAddForMention, suggestAgentMention } from "./mention-suggestion";

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

  it("matches a case-sensitive agent only when the name keeps its case", () => {
    const sol = { id: "participant:sol", displayName: "Sol", matchCase: true };
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

describe("agentsToAddForMention", () => {
  const agent = (id: string, displayName: string, archivedAt: string | null = null) => ({
    participant: { id, displayName, archivedAt },
  });
  const codex = agent("participant:codex", "Codex");
  const claude = agent("participant:claude", "Claude Code");
  const retired = agent("participant:retired", "Codex Old", "2026-10-01T00:00:00.000Z");

  it("offers every known agent outside the channel while the query is empty", () => {
    expect(agentsToAddForMention([codex, claude], new Set(), "")).toEqual([codex, claude]);
  });

  it("leaves out agents already in the channel and archived agents", () => {
    expect(agentsToAddForMention([codex, claude, retired], new Set(["participant:codex"]), ""))
      .toEqual([claude]);
  });

  it("matches the typed query anywhere in the name, ignoring case and surrounding space", () => {
    expect(agentsToAddForMention([codex, claude], new Set(), " CODE ")).toEqual([codex, claude]);
    expect(agentsToAddForMention([codex, claude], new Set(), "claude")).toEqual([claude]);
    expect(agentsToAddForMention([codex, claude], new Set(), "gemini")).toEqual([]);
  });
});
