import { describe, expect, it } from "vitest";
import {
  TIMING_TOO_LARGE_CODE,
  VALUE_TOO_LARGE_CODE,
  isPreservedDataOmissionMessage,
  isTimingTooLargeMessage,
  isValueTooLargeMessage,
  timingTooLargeImportLabel,
  valueTooLargeImportLabel,
} from "./bound-errors";

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

describe("value bound error", () => {
  const omission = `${VALUE_TOO_LARGE_CODE}: 3 properties left out to keep this value within the sync size limit`;

  it("recognizes the backend refusal by its stable prefix", () => {
    expect(isValueTooLargeMessage(`${VALUE_TOO_LARGE_CODE}: this event's alarms value exceeds 262144 bytes`)).toBe(true);
    expect(isValueTooLargeMessage(`${TIMING_TOO_LARGE_CODE}: x`)).toBe(false);
    expect(isValueTooLargeMessage(` ${VALUE_TOO_LARGE_CODE}: x`)).toBe(false);
  });

  it("extracts the event label from an import refusal", () => {
    expect(valueTooLargeImportLabel(`${VALUE_TOO_LARGE_CODE}: Weekly sync (attendees)`)).toBe("Weekly sync");
    expect(valueTooLargeImportLabel(`${VALUE_TOO_LARGE_CODE}: Plan (v2) (extended properties)`)).toBe("Plan (v2)");
  });

  it("returns no label for the omission count and other warnings", () => {
    expect(valueTooLargeImportLabel(omission)).toBeNull();
    expect(valueTooLargeImportLabel("Skipped event (attendees)")).toBeNull();
    expect(valueTooLargeImportLabel(`${VALUE_TOO_LARGE_CODE}: no group`)).toBeNull();
  });

  it("tells the omission count from an event refusal", () => {
    expect(isPreservedDataOmissionMessage(omission)).toBe(true);
    expect(isPreservedDataOmissionMessage(`${VALUE_TOO_LARGE_CODE}: Weekly sync (alarms)`)).toBe(false);
    expect(isPreservedDataOmissionMessage("3 properties left out to keep this value within the sync size limit")).toBe(false);
  });
});
