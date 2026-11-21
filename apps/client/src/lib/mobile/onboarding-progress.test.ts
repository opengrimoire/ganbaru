import { describe, expect, it } from "vitest";
import { WELCOME_DATA_STEP, WELCOME_SETUP_STEPS, WELCOME_TOUR_STEPS } from "$lib/vault/welcome-steps";
import { FOCUS_ACCESS_STEP_ORDER } from "./focus-access-steps";
import { MOBILE_ONBOARDING_SCREEN_COUNT, onboardingScreenPosition, onboardingSegmentFills } from "./onboarding-progress";

const setupCount = WELCOME_SETUP_STEPS.length;
const tourCount = WELCOME_TOUR_STEPS.length;
const permissionCount = FOCUS_ACCESS_STEP_ORDER.length;
/** Where the tour starts: after the setup screens and the linking screen. */
const tourStart = setupCount + 1;
const permissionStart = tourStart + tourCount;

describe("mobile onboarding progress", () => {
  it("has one segment for the language and vault screens, linking, every tour screen, and every possible permission", () => {
    expect(MOBILE_ONBOARDING_SCREEN_COUNT).toBe(setupCount + 1 + tourCount + permissionCount);
    expect(onboardingSegmentFills("setup")).toHaveLength(MOBILE_ONBOARDING_SCREEN_COUNT);
  });

  it("places linking right after the vault choice, then the tour, then the permissions", () => {
    expect(onboardingScreenPosition("setup")).toBe(0);
    expect(onboardingScreenPosition("setup", WELCOME_DATA_STEP)).toBe(WELCOME_DATA_STEP);
    expect(onboardingScreenPosition("linking")).toBe(setupCount);
    expect(onboardingScreenPosition("tour")).toBe(tourStart);
    expect(onboardingScreenPosition("tour", tourCount - 1)).toBe(permissionStart - 1);
    expect(onboardingScreenPosition("permissions")).toBe(permissionStart);
    expect(onboardingScreenPosition("permissions", permissionCount - 1)).toBe(MOBILE_ONBOARDING_SCREEN_COUNT - 1);
  });

  it("fills every screen up to and including the current one", () => {
    const fills = onboardingSegmentFills("tour", 1);
    expect(fills.slice(0, tourStart + 2).every(Boolean)).toBe(true);
    expect(fills.slice(tourStart + 2).every((fill) => !fill)).toBe(true);
    const linking = onboardingSegmentFills("linking");
    expect(linking.filter(Boolean)).toHaveLength(setupCount + 1);
  });

  it("passes the permission screens a phone skips as done", () => {
    const appBlocking = FOCUS_ACCESS_STEP_ORDER.indexOf("app-blocking");
    const fills = onboardingSegmentFills("permissions", appBlocking);
    expect(fills.filter(Boolean)).toHaveLength(permissionStart + appBlocking + 1);
    expect(fills.at(-1)).toBe(appBlocking === permissionCount - 1);
  });

  it("keeps the screen within its stage when it is out of range or not a number", () => {
    expect(onboardingScreenPosition("setup", 99)).toBe(setupCount - 1);
    expect(onboardingScreenPosition("linking", 5)).toBe(setupCount);
    expect(onboardingScreenPosition("tour", 99)).toBe(permissionStart - 1);
    expect(onboardingScreenPosition("permissions", -1)).toBe(permissionStart);
    expect(onboardingScreenPosition("permissions", Number.NaN)).toBe(permissionStart);
  });
});
