import { describe, expect, it } from "vitest";
import { chatParticipantDisplayName } from "./participant-display";

describe("Chat participant display", () => {
  it("resolves every local-user snapshot through the current profile", () => {
    expect(chatParticipantDisplayName(
      { id: "participant:local-owner", kind: "local_user", displayName: "You" },
      "Alice",
      "You",
    )).toBe("Alice");
  });

  it("uses the localized fallback for an unnamed local profile", () => {
    expect(chatParticipantDisplayName(
      { id: "participant:local-owner", kind: "local_user", displayName: "Stale name" },
      "",
      "You",
    )).toBe("You");
  });

  it("does not override teammates or future collaborators", () => {
    expect(chatParticipantDisplayName(
      { id: "participant:atlas", kind: "ai_teammate", displayName: "Atlas" },
      "Alice",
      "You",
    )).toBe("Atlas");
    expect(chatParticipantDisplayName(
      { id: "participant:alex", kind: "human", displayName: "Alex" },
      "Alice",
      "You",
    )).toBe("Alex");
  });

  it("does not treat an invalid second local participant as the owner", () => {
    expect(chatParticipantDisplayName(
      { id: "participant:other", kind: "local_user", displayName: "Stored name" },
      "Alice",
      "You",
    )).toBe("Stored name");
  });
});
