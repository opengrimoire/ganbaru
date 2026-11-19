import { describe, expect, it } from "vitest";
import { WELCOME_DATA_STEP, WELCOME_SETUP_STEPS, WELCOME_TOUR_STEPS } from "$lib/vault/welcome-steps";
import { FOCUS_ACCESS_STEP_ORDER } from "./focus-access-steps";
import { MOBILE_ONBOARDING_SCREEN_COUNT, onboardingScreenPosition, onboardingSegmentFills } from "./onboarding-progress";

const setupCount = WELCOME_SETUP_STEPS.length;
const tourCount = WELCOME_TOUR_STEPS.length;
const permissionCount = FOCUS_ACCESS_STEP_ORDER.length;

describe("mobile onboarding progress", () => {
  it("has one segment for the language and vault screens, every tour screen, every possible permission, and linking", () => {
    expect(MOBILE_ONBOARDING_SCREEN_COUNT).toBe(setupCount + tourCount + permissionCount + 1);
    expect(onboardingSegmentFills("setup")).toHaveLength(MOBILE_ONBOARDING_SCREEN_COUNT);
  });

  it("places each stage's screens after every screen of the stages before it", () => {
    expect(onboardingScreenPosition("setup")).toBe(0);
    expect(onboardingScreenPosition("setup", WELCOME_DATA_STEP)).toBe(WELCOME_DATA_STEP);
    expect(onboardingScreenPosition("tour")).toBe(setupCount);
    expect(onboardingScreenPosition("tour", tourCount - 1)).toBe(setupCount + tourCount - 1);
    expect(onboardingScreenPosition("permissions")).toBe(setupCount + tourCount);
    expect(onboardingScreenPosition("permissions", permissionCount - 1)).toBe(setupCount + tourCount + permissionCount - 1);
    expect(onboardingScreenPosition("linking")).toBe(MOBILE_ONBOARDING_SCREEN_COUNT - 1);
  });

  it("fills every screen up to and including the current one", () => {
    const fills = onboardingSegmentFills("tour", 1);
    expect(fills.slice(0, setupCount + 2).every(Boolean)).toBe(true);
    expect(fills.slice(setupCount + 2).every((fill) => !fill)).toBe(true);
    expect(onboardingSegmentFills("linking").every(Boolean)).toBe(true);
  });

  it("passes the permission screens a phone skips as done", () => {
    const appBlocking = FOCUS_ACCESS_STEP_ORDER.indexOf("app-blocking");
    const fills = onboardingSegmentFills("permissions", appBlocking);
    expect(fills.filter(Boolean)).toHaveLength(setupCount + tourCount + appBlocking + 1);
    expect(fills.at(-1)).toBe(false);
  });

  it("keeps the screen within its stage when it is out of range or not a number", () => {
    expect(onboardingScreenPosition("setup", 99)).toBe(setupCount - 1);
    expect(onboardingScreenPosition("tour", 99)).toBe(setupCount + tourCount - 1);
    expect(onboardingScreenPosition("permissions", -1)).toBe(setupCount + tourCount);
    expect(onboardingScreenPosition("permissions", Number.NaN)).toBe(setupCount + tourCount);
  });
});
