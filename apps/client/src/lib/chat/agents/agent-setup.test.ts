import { describe, expect, it } from "vitest";
import { agentNeedsSetupHere } from "./agent-setup";

describe("agentNeedsSetupHere", () => {
  it("asks for setup only where the device runs the work and the provider is unavailable", () => {
    expect(agentNeedsSetupHere("provider_unavailable", true)).toBe(true);
    expect(agentNeedsSetupHere("healthy", true)).toBe(false);
  });

  it("never asks for setup on a device that leaves the work to a linked desktop", () => {
    expect(agentNeedsSetupHere("provider_unavailable", false)).toBe(false);
    expect(agentNeedsSetupHere("healthy", false)).toBe(false);
  });
});
