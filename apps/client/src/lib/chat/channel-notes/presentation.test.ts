import { describe, expect, it } from "vitest";
import { CHAT_MAX_ACTIVE_CHANNEL_NOTES, CHAT_MAX_CHANNEL_NOTE_CHARS } from "$lib/chat/contracts";
import { LOCAL_CHAT_PARTICIPANT_ID } from "$lib/chat/agents/participant-display";
import { channelNoteDraftProblem, channelNoteParticipantName, type ChannelNoteNameSources } from "./presentation";

const sources: ChannelNoteNameSources = {
  participants: [
    { id: LOCAL_CHAT_PARTICIPANT_ID, kind: "local_user", displayName: "Stored name" },
    { id: "participant:atlas", kind: "ai_agent", displayName: "Atlas" },
    { id: "participant:atlas", kind: "ai_agent", displayName: "Atlas copy" },
  ],
  localProfileDisplayName: "  ",
  localFallback: "You",
  unknownName: "Someone",
};

describe("channelNoteParticipantName", () => {
  it("credits nobody for a missing participant id", () => {
    expect(channelNoteParticipantName(null, sources)).toBeNull();
  });

  it("uses the first matching participant and the local profile name", () => {
    expect(channelNoteParticipantName("participant:atlas", sources)).toBe("Atlas");
    expect(channelNoteParticipantName(LOCAL_CHAT_PARTICIPANT_ID, sources)).toBe("You");
    expect(channelNoteParticipantName(LOCAL_CHAT_PARTICIPANT_ID, { ...sources, localProfileDisplayName: "Victor" })).toBe("Victor");
  });

  it("falls back to the unknown label for a participant this device does not list", () => {
    expect(channelNoteParticipantName("participant:gone", sources)).toBe("Someone");
  });
});

describe("channelNoteDraftProblem", () => {
  it("accepts text up to the bound after trimming", () => {
    expect(channelNoteDraftProblem(`  ${"é".repeat(CHAT_MAX_CHANNEL_NOTE_CHARS)}\n`, 0, true)).toBeNull();
  });

  it("refuses blank and oversize text", () => {
    expect(channelNoteDraftProblem(" \n\t", 0, true)).toBe("empty");
    expect(channelNoteDraftProblem("é".repeat(CHAT_MAX_CHANNEL_NOTE_CHARS + 1), 0, false)).toBe("too_long");
  });

  it("refuses a new note at the limit but lets an existing note be edited", () => {
    expect(channelNoteDraftProblem("Use pnpm", CHAT_MAX_ACTIVE_CHANNEL_NOTES, true)).toBe("limit");
    expect(channelNoteDraftProblem("Use pnpm", CHAT_MAX_ACTIVE_CHANNEL_NOTES - 1, true)).toBeNull();
    expect(channelNoteDraftProblem("Use pnpm", CHAT_MAX_ACTIVE_CHANNEL_NOTES + 3, false)).toBeNull();
  });
});
