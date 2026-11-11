import { describe, expect, it } from "vitest";
import { parseThreadUsage } from "./events";

const storedUsage = {
  inputTokens: 60,
  outputTokens: 64,
  cachedInputTokens: 3_300,
  contextTokens: 1_302,
  contextLimit: 200_000,
  cost: { amount: 0.02, currency: "USD", providerReported: true },
};

describe("parseThreadUsage", () => {
  it("reads reported cache writes", () => {
    expect(parseThreadUsage({ ...storedUsage, cacheWriteInputTokens: 210 }, "usage")).toEqual({
      ...storedUsage,
      cacheWriteInputTokens: 210,
    });
  });

  it("reads null and absent cache writes as unreported", () => {
    expect(parseThreadUsage({ ...storedUsage, cacheWriteInputTokens: null }, "usage").cacheWriteInputTokens).toBeNull();
    expect(parseThreadUsage(storedUsage, "usage").cacheWriteInputTokens).toBeNull();
  });

  it("rejects negative or fractional cache writes", () => {
    expect(() => parseThreadUsage({ ...storedUsage, cacheWriteInputTokens: -1 }, "usage")).toThrow("usage.cacheWriteInputTokens");
    expect(() => parseThreadUsage({ ...storedUsage, cacheWriteInputTokens: 1.5 }, "usage")).toThrow("usage.cacheWriteInputTokens");
  });
});
