import { describe, expect, it } from "vitest";
import { WELCOME_STEPS, welcomeStepAt } from "./welcome-steps";

describe("welcome steps", () => {
  it("opens on the language, then the Calendar, and ends on the app's credentials", () => {
    expect(WELCOME_STEPS.slice(0, 8)).toEqual(["language", "calendar", "blocking", "limits", "music", "tasks", "projects", "notes"]);
    expect(WELCOME_STEPS.at(-1)).toBe("about");
  });

  it("clamps out of range and fractional positions to a real step", () => {
    expect(welcomeStepAt(-3)).toBe(WELCOME_STEPS[0]);
    expect(welcomeStepAt(99)).toBe(WELCOME_STEPS.at(-1));
    expect(welcomeStepAt(0.7)).toBe(WELCOME_STEPS[0]);
    expect(welcomeStepAt(Number.NaN)).toBe(WELCOME_STEPS[0]);
  });
});
