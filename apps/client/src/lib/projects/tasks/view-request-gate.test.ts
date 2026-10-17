import { describe, expect, it } from "vitest";
import { ProjectTaskViewRequestGate } from "./view-request-gate";

describe("ProjectTaskViewRequestGate", () => {
  it("rejects an older response after a newer request begins", () => {
    const gate = new ProjectTaskViewRequestGate();
    const oldRequest = gate.begin();
    const currentRequest = gate.begin();
    expect(gate.isCurrent(oldRequest)).toBe(false);
    expect(gate.isCurrent(currentRequest)).toBe(true);
  });

  it("invalidates the active response when cancelled", () => {
    const gate = new ProjectTaskViewRequestGate();
    const request = gate.begin();
    gate.cancel();
    expect(gate.isCurrent(request)).toBe(false);
  });
});
