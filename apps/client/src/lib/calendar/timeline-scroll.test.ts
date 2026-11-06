import { describe, it, expect } from "vitest";
import {
  calendarScrollbarThumb,
  calendarTimelineScrollSpeedForDistance,
  calendarTimelineWheelMultiplierForDeltaPixels,
  CALENDAR_TIMELINE_LOW_DELTA_WHEEL_MULTIPLIER,
  CALENDAR_TIMELINE_WHEEL_MULTIPLIER,
  wheelDeltaToScrollPixels,
} from "./timeline-scroll";

describe("wheelDeltaToScrollPixels", () => {
  it("keeps pixel-mode wheel deltas unchanged", () => {
    expect(wheelDeltaToScrollPixels(100, 0, 500)).toBe(100);
    expect(wheelDeltaToScrollPixels(-100, 0, 500)).toBe(-100);
  });

  it("normalizes line-mode wheel deltas to direct scroll pixels", () => {
    expect(wheelDeltaToScrollPixels(3, 1, 500)).toBe(120);
  });

  it("normalizes page-mode wheel deltas to viewport pages", () => {
    expect(wheelDeltaToScrollPixels(1, 2, 500)).toBe(500);
  });

  it("keeps the calendar timeline wheel multiplier modest", () => {
    expect(CALENDAR_TIMELINE_WHEEL_MULTIPLIER).toBeCloseTo(1.24);
  });

  it("keeps normal wheel ticks at the base calendar timeline multiplier", () => {
    expect(calendarTimelineWheelMultiplierForDeltaPixels(102)).toBeCloseTo(CALENDAR_TIMELINE_WHEEL_MULTIPLIER);
    expect(calendarTimelineWheelMultiplierForDeltaPixels(-102)).toBeCloseTo(CALENDAR_TIMELINE_WHEEL_MULTIPLIER);
  });

  it("makes lighter calendar timeline wheel deltas more sensitive", () => {
    const multiplier = calendarTimelineWheelMultiplierForDeltaPixels(51);

    expect(multiplier).toBeGreaterThan(CALENDAR_TIMELINE_WHEEL_MULTIPLIER);
    expect(multiplier).toBeLessThan(CALENDAR_TIMELINE_LOW_DELTA_WHEEL_MULTIPLIER);
  });

  it("keeps clipped and normal calendar timeline scroll distances at the base speed", () => {
    const reference = 102 * CALENDAR_TIMELINE_WHEEL_MULTIPLIER;

    expect(calendarTimelineScrollSpeedForDistance(reference / 4)).toBeCloseTo(
      calendarTimelineScrollSpeedForDistance(reference),
    );
    expect(calendarTimelineScrollSpeedForDistance(1)).toBeCloseTo(
      calendarTimelineScrollSpeedForDistance(reference),
    );
  });

  it("increases continuous scroll speed for accumulated long distances", () => {
    const reference = 102 * CALENDAR_TIMELINE_WHEEL_MULTIPLIER;
    const baseSpeed = calendarTimelineScrollSpeedForDistance(reference);

    expect(calendarTimelineScrollSpeedForDistance(reference * 2)).toBeGreaterThan(baseSpeed);
    expect(calendarTimelineScrollSpeedForDistance(reference * 10)).toBeCloseTo(baseSpeed * 2);
  });
});

describe("calendarScrollbarThumb", () => {
  it("sizes the thumb to the visible share and moves it across the free track", () => {
    const scroll = { scrollHeight: 2400, clientHeight: 600 };
    expect(calendarScrollbarThumb({ ...scroll, scrollTop: 0 }, 600)).toEqual({ top: 0, height: 150 });
    expect(calendarScrollbarThumb({ ...scroll, scrollTop: 900 }, 600)).toEqual({ top: 225, height: 150 });
    expect(calendarScrollbarThumb({ ...scroll, scrollTop: 1800 }, 600)).toEqual({ top: 450, height: 150 });
  });

  it("keeps a grabbable thumb on very long content", () => {
    expect(calendarScrollbarThumb({ scrollTop: 0, scrollHeight: 100_000, clientHeight: 500 }, 500).height).toBe(24);
  });

  it("hides the thumb when there is nothing to scroll", () => {
    expect(calendarScrollbarThumb({ scrollTop: 0, scrollHeight: 601, clientHeight: 600 }, 600)).toEqual({ top: 0, height: 0 });
  });

  it("keeps the thumb inside the track when scrolled past either end", () => {
    const scroll = { scrollHeight: 2400, clientHeight: 600 };
    expect(calendarScrollbarThumb({ ...scroll, scrollTop: -40 }, 600).top).toBe(0);
    expect(calendarScrollbarThumb({ ...scroll, scrollTop: 1900 }, 600).top).toBe(450);
  });
});
