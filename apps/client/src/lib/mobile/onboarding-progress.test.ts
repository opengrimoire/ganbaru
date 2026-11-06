import { describe, expect, it } from "vitest";
import { MOBILE_ONBOARDING_STAGES, onboardingSegmentFills, onboardingStageNumber } from "./onboarding-progress";

describe("mobile onboarding progress", () => {
  it("fills earlier stages, leaves later ones empty, and fills the current stage completely by default", () => {
    expect(onboardingSegmentFills("welcome")).toEqual([1, 0, 0, 0]);
    expect(onboardingSegmentFills("linking")).toEqual([1, 1, 1, 1]);
  });

  it("fills a stage with several screens one screen at a time", () => {
    expect(onboardingSegmentFills("permissions", 2 / 5)).toEqual([1, 1, 0.4, 0]);
    expect(onboardingSegmentFills("permissions", 0)).toEqual([1, 1, 0, 0]);
  });

  it("keeps the current stage within its segment when the fraction is out of range or not a number", () => {
    expect(onboardingSegmentFills("vault", 3)).toEqual([1, 1, 0, 0]);
    expect(onboardingSegmentFills("vault", -1)).toEqual([1, 0, 0, 0]);
    expect(onboardingSegmentFills("vault", Number.NaN)).toEqual([1, 0, 0, 0]);
  });

  it("numbers stages from one in display order", () => {
    expect(MOBILE_ONBOARDING_STAGES.map(onboardingStageNumber)).toEqual([1, 2, 3, 4]);
  });
});
