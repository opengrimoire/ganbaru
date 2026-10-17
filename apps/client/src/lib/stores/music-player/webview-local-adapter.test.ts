import { describe, expect, it } from "vitest";
import {
  canNudgePlayableStart,
  nextPlayableStartCandidateMs,
} from "./webview-local-adapter";

describe("Music WebView local adapter", () => {
  it("prefers the seekable start and then a bounded forward nudge", () => {
    expect(nextPlayableStartCandidateMs(0, 2_500, 0)).toBe(2_500);
    expect(nextPlayableStartCandidateMs(2_500, 2_500, 0)).toBe(3_500);
  });

  it("stops nudging after decoded data or the retry bound", () => {
    expect(canNudgePlayableStart(0, 11)).toBe(true);
    expect(canNudgePlayableStart(0, 12)).toBe(false);
    expect(canNudgePlayableStart(2, 0)).toBe(false);
  });
});
