import { describe, expect, it } from "vitest";
import {
  calendarSwipeAxis,
  calendarSwipeDirection,
  calendarSwipeNavigation,
} from "./mobile-gestures";

describe("mobile calendar gestures", () => {
  it("reserves dominant horizontal movement for view navigation", () => {
    expect(calendarSwipeAxis(30, 8)).toBe("horizontal");
    expect(calendarSwipeAxis(8, 30)).toBe("vertical");
    expect(calendarSwipeAxis(4, 2)).toBeNull();
  });

  it("commits by distance or velocity and maps finger direction", () => {
    expect(calendarSwipeDirection(-12)).toBe("forward");
    expect(calendarSwipeDirection(12)).toBe("back");
    expect(calendarSwipeDirection(0)).toBeNull();
    expect(calendarSwipeNavigation({ deltaX: -80, elapsedMs: 400, viewportWidth: 360 })).toBe("forward");
    expect(calendarSwipeNavigation({ deltaX: 60, elapsedMs: 80, viewportWidth: 360 })).toBe("back");
    expect(calendarSwipeNavigation({ deltaX: -45, elapsedMs: 300, viewportWidth: 360 })).toBe("forward");
    expect(calendarSwipeNavigation({ deltaX: 30, elapsedMs: 400, viewportWidth: 360 })).toBeNull();
  });
});
