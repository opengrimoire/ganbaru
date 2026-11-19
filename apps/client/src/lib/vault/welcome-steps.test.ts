import { describe, expect, it } from "vitest";
import {
  WELCOME_DATA_STEP,
  WELCOME_SETUP_STEPS,
  WELCOME_TOUR_STEPS,
  welcomeProgress,
  welcomeStepAt,
} from "./welcome-steps";

describe("welcome steps", () => {
  it("chooses the language and then the vault before any feature screen", () => {
    expect(WELCOME_SETUP_STEPS).toEqual(["language", "data"]);
    expect(welcomeStepAt("setup", WELCOME_DATA_STEP)).toBe("data");
  });

  it("tours the features from the Calendar and ends on the app's credentials", () => {
    expect(WELCOME_TOUR_STEPS.slice(0, 9)).toEqual(["calendar", "blocking", "limits", "music", "tasks", "projects", "notes", "quickNotes", "chat"]);
    expect(WELCOME_TOUR_STEPS.at(-1)).toBe("about");
  });

  it("clamps out of range and fractional positions to a real screen of the stage", () => {
    expect(welcomeStepAt("setup", -3)).toBe("language");
    expect(welcomeStepAt("setup", 99)).toBe("data");
    expect(welcomeStepAt("tour", 99)).toBe("about");
    expect(welcomeStepAt("tour", 0.7)).toBe("calendar");
    expect(welcomeStepAt("tour", Number.NaN)).toBe("calendar");
  });

  it("counts progress across both stages, with the tour after the setup screens", () => {
    const count = WELCOME_SETUP_STEPS.length + WELCOME_TOUR_STEPS.length;
    expect(welcomeProgress("setup", 0)).toEqual({ index: 0, count });
    expect(welcomeProgress("setup", WELCOME_DATA_STEP)).toEqual({ index: WELCOME_DATA_STEP, count });
    expect(welcomeProgress("tour", 0)).toEqual({ index: WELCOME_SETUP_STEPS.length, count });
    expect(welcomeProgress("tour", 99)).toEqual({ index: count - 1, count });
  });
});
