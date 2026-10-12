import { describe, expect, it } from "vitest";
import { soundSelectionAction } from "./selection";

describe("background sound selection", () => {
  it("replaces sounds in the default one-at-a-time mode", () => {
    expect(soundSelectionAction(["brown"], "pink", false, "playing")).toEqual({ kind: "start", ids: ["pink"] });
    expect(soundSelectionAction(["brown"], "brown", false, "playing")).toEqual({ kind: "pause" });
    expect(soundSelectionAction(["brown"], "brown", false, "paused")).toEqual({ kind: "resume" });
  });

  it("adds and removes individual layers without clearing the others", () => {
    expect(soundSelectionAction(["brown"], "pink", true, "playing")).toEqual({ kind: "start", ids: ["brown", "pink"] });
    expect(soundSelectionAction(["brown", "pink"], "brown", true, "playing")).toEqual({ kind: "start", ids: ["pink"] });
    expect(soundSelectionAction(["brown"], "brown", true, "playing")).toEqual({ kind: "stop" });
  });
});
