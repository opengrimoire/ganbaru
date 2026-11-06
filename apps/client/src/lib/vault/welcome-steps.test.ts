import { describe, expect, it } from "vitest";
import { WELCOME_STEPS, welcomeProgress, welcomeStepAt } from "./welcome-steps";

describe("welcome steps", () => {
  it("opens on the Calendar and ends on the app's credentials", () => {
    expect(WELCOME_STEPS[0]).toBe("calendar");
    expect(WELCOME_STEPS.at(-1)).toBe("about");
  });

  it("clamps out of range and fractional positions to a real step", () => {
    expect(welcomeStepAt(-3)).toBe(WELCOME_STEPS[0]);
    expect(welcomeStepAt(99)).toBe(WELCOME_STEPS.at(-1));
    expect(welcomeStepAt(0.7)).toBe(WELCOME_STEPS[0]);
    expect(welcomeStepAt(Number.NaN)).toBe(WELCOME_STEPS[0]);
  });

  it("fills its progress one step at a time and is full on the last step", () => {
    expect(welcomeProgress(0)).toBeCloseTo(1 / WELCOME_STEPS.length, 9);
    expect(welcomeProgress(WELCOME_STEPS.length - 1)).toBe(1);
    expect(welcomeProgress(99)).toBe(1);
  });
});
