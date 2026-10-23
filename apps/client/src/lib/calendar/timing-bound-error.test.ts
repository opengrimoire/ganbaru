import { describe, expect, it } from "vitest";
import {
  TIMING_TOO_LARGE_CODE,
  isTimingTooLargeMessage,
  timingTooLargeImportLabel,
} from "./timing-bound-error";

describe("timing bound error", () => {
  it("recognizes the backend refusal by its stable prefix", () => {
    expect(isTimingTooLargeMessage(`${TIMING_TOO_LARGE_CODE}: too many`)).toBe(true);
    expect(isTimingTooLargeMessage("Save failed")).toBe(false);
    expect(isTimingTooLargeMessage(` ${TIMING_TOO_LARGE_CODE}: x`)).toBe(false);
  });

  it("extracts the event label from an import warning", () => {
    expect(timingTooLargeImportLabel(`${TIMING_TOO_LARGE_CODE}: Weekly sync`)).toBe("Weekly sync");
    expect(timingTooLargeImportLabel("Skipped event")).toBeNull();
  });

  it("keeps a label that contains a colon", () => {
    expect(timingTooLargeImportLabel(`${TIMING_TOO_LARGE_CODE}: Team: standup`)).toBe("Team: standup");
  });
});
