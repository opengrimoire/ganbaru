import { describe, expect, it } from "vitest";
import { WELCOME_STEPS } from "$lib/vault/welcome-steps";
import { FOCUS_ACCESS_STEP_ORDER } from "./focus-access-steps";
import { MOBILE_ONBOARDING_SCREEN_COUNT, onboardingScreenPosition, onboardingSegmentFills } from "./onboarding-progress";

const welcomeCount = WELCOME_STEPS.length;
const permissionCount = FOCUS_ACCESS_STEP_ORDER.length;

describe("mobile onboarding progress", () => {
  it("has one segment for every welcome screen, the vault, every possible permission, and linking", () => {
    expect(MOBILE_ONBOARDING_SCREEN_COUNT).toBe(welcomeCount + 1 + permissionCount + 1);
    expect(onboardingSegmentFills("welcome")).toHaveLength(MOBILE_ONBOARDING_SCREEN_COUNT);
  });

  it("places each stage's screens after every screen of the stages before it", () => {
    expect(onboardingScreenPosition("welcome")).toBe(0);
    expect(onboardingScreenPosition("welcome", welcomeCount - 1)).toBe(welcomeCount - 1);
    expect(onboardingScreenPosition("vault")).toBe(welcomeCount);
    expect(onboardingScreenPosition("permissions")).toBe(welcomeCount + 1);
    expect(onboardingScreenPosition("permissions", permissionCount - 1)).toBe(welcomeCount + permissionCount);
    expect(onboardingScreenPosition("linking")).toBe(MOBILE_ONBOARDING_SCREEN_COUNT - 1);
  });

  it("fills every screen up to and including the current one", () => {
    const fills = onboardingSegmentFills("welcome", 2);
    expect(fills.slice(0, 3)).toEqual([true, true, true]);
    expect(fills.slice(3).every((fill) => !fill)).toBe(true);
    expect(onboardingSegmentFills("linking").every(Boolean)).toBe(true);
  });

  it("passes the permission screens a phone skips as done", () => {
    const appBlocking = FOCUS_ACCESS_STEP_ORDER.indexOf("app-blocking");
    const fills = onboardingSegmentFills("permissions", appBlocking);
    expect(fills.filter(Boolean)).toHaveLength(welcomeCount + 1 + appBlocking + 1);
    expect(fills.at(-1)).toBe(false);
  });

  it("keeps the screen within its stage when it is out of range or not a number", () => {
    expect(onboardingScreenPosition("welcome", 99)).toBe(welcomeCount - 1);
    expect(onboardingScreenPosition("vault", 3)).toBe(welcomeCount);
    expect(onboardingScreenPosition("permissions", -1)).toBe(welcomeCount + 1);
    expect(onboardingScreenPosition("permissions", Number.NaN)).toBe(welcomeCount + 1);
  });
});
