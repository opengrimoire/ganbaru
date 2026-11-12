import { describe, expect, it } from "vitest";
import { chatQuietState } from "./quiet";

describe("chatQuietState", () => {
  it("maps a quiet read with an end to its millisecond end", () => {
    expect(chatQuietState({ quiet: true, reason: "window", endsAt: "2026-11-12T22:30:00Z" })).toEqual({
      quiet: true,
      endsAtMs: Date.UTC(2026, 10, 12, 22, 30),
    });
  });

  it("maps a quiet read without a known end to a null end", () => {
    expect(chatQuietState({ quiet: true, reason: "focus", endsAt: null })).toEqual({ quiet: true, endsAtMs: null });
  });

  it("maps an open read to not quiet", () => {
    expect(chatQuietState({ quiet: false, reason: null, endsAt: null })).toEqual({ quiet: false, endsAtMs: null });
  });
});
