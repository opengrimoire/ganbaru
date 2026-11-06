import { describe, expect, it } from "vitest";
import {
  completeMobileFocusOnboarding,
  markMobileFocusAccessReviewed,
  mobileFocusAccessReviewed,
  mobileFocusOnboardingCompleted,
  parseMobileBackgroundExecutionStatus,
} from "./mobile-background-execution";

describe("mobile background execution status", () => {
  it("accepts a bounded native status", () => {
    expect(parseMobileBackgroundExecutionStatus({
      manufacturer: "Honor",
      appLabel: "Ganbaru Dev",
      autostartSettingsAvailable: true,
      backgroundRestricted: false,
      batteryOptimizationExempt: false,
    })).toEqual({
      manufacturer: "Honor",
      appLabel: "Ganbaru Dev",
      autostartSettingsAvailable: true,
      backgroundRestricted: false,
      batteryOptimizationExempt: false,
    });
  });

  it("rejects an empty app label", () => {
    expect(() => parseMobileBackgroundExecutionStatus({
      manufacturer: "Example",
      appLabel: " ",
      autostartSettingsAvailable: true,
      backgroundRestricted: false,
      batteryOptimizationExempt: true,
    })).toThrow("invalid app label");
  });

  it("rejects a missing autostart availability", () => {
    expect(() => parseMobileBackgroundExecutionStatus({
      manufacturer: "Example",
      appLabel: "Ganbaru",
      backgroundRestricted: false,
      batteryOptimizationExempt: true,
    })).toThrow("invalid autostart availability");
  });

  it("rejects a missing background restriction state", () => {
    expect(() => parseMobileBackgroundExecutionStatus({
      manufacturer: "Example",
      appLabel: "Ganbaru",
      autostartSettingsAvailable: false,
      batteryOptimizationExempt: false,
    })).toThrow("invalid restriction state");
  });
});

describe("mobile Focus onboarding state", () => {
  it("tracks completion and individual reviewed controls", () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };

    expect(mobileFocusOnboardingCompleted(storage)).toBe(false);
    expect(mobileFocusAccessReviewed("autostart", storage)).toBe(false);

    markMobileFocusAccessReviewed("autostart", storage);
    completeMobileFocusOnboarding(storage);

    expect(values.get("ganbaru.mobile-focus-onboarding.v1")).toBe("complete");
    expect(values.get("ganbaru.mobile-focus-review.v1.autostart")).toBe("reviewed");
    expect(mobileFocusAccessReviewed("autostart", storage)).toBe(true);
    expect(mobileFocusAccessReviewed("battery", storage)).toBe(false);
    expect(mobileFocusOnboardingCompleted(storage)).toBe(true);
  });
});
