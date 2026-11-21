import { describe, expect, it } from "vitest";
import { platformProfileFor } from "$lib/platform";
import {
  WELCOME_DATA_STEP,
  WELCOME_SETUP_STEPS,
  welcomeProgress,
  welcomeStepAt,
  welcomeTourSteps,
} from "./welcome-steps";

const desktop = platformProfileFor("linux");
const android = platformProfileFor("android");

describe("welcome steps", () => {
  it("chooses the language and then the vault before any feature screen", () => {
    expect(WELCOME_SETUP_STEPS).toEqual(["language", "data"]);
    expect(welcomeStepAt("setup", WELCOME_DATA_STEP)).toBe("data");
  });

  it("tours the features from the Calendar, checks the coding agents after Chat on desktop, and ends on the app's credentials", () => {
    expect(welcomeTourSteps(desktop)).toEqual([
      "calendar", "blocking", "limits", "music", "tasks", "projects", "notes", "quickNotes", "chat", "agents", "about",
    ]);
    expect(welcomeTourSteps(platformProfileFor("windows"))).toContain("agents");
  });

  it("leaves the coding agents screen out where the device does not run agents", () => {
    expect(welcomeTourSteps(android)).not.toContain("agents");
    expect(welcomeTourSteps(android).slice(-2)).toEqual(["chat", "about"]);
    expect(welcomeStepAt("tour", 9, android)).toBe("about");
  });

  it("clamps out of range and fractional positions to a real screen of the stage", () => {
    expect(welcomeStepAt("setup", -3)).toBe("language");
    expect(welcomeStepAt("setup", 99)).toBe("data");
    expect(welcomeStepAt("tour", 99, desktop)).toBe("about");
    expect(welcomeStepAt("tour", 0.7, desktop)).toBe("calendar");
    expect(welcomeStepAt("tour", Number.NaN, desktop)).toBe("calendar");
  });

  it("counts progress across the setup screens, then the linking screen, then the tour of the platform", () => {
    for (const profile of [desktop, android]) {
      const count = WELCOME_SETUP_STEPS.length + 1 + welcomeTourSteps(profile).length;
      expect(welcomeProgress("setup", 0, profile)).toEqual({ index: 0, count });
      expect(welcomeProgress("setup", 99, profile)).toEqual({ index: WELCOME_DATA_STEP, count });
      expect(welcomeProgress("linking", 0, profile)).toEqual({ index: WELCOME_SETUP_STEPS.length, count });
      expect(welcomeProgress("tour", 0, profile)).toEqual({ index: WELCOME_SETUP_STEPS.length + 1, count });
      expect(welcomeProgress("tour", 99, profile)).toEqual({ index: count - 1, count });
    }
    expect(welcomeProgress("tour", 0, desktop).count).toBe(welcomeProgress("tour", 0, android).count + 1);
  });
});
