import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_RECEIPT_ITEMS,
  ABOUT_SEGMENTS,
  aboutConfirmCheck,
  aboutConfirmRing,
  aboutFeed,
  aboutImpact,
  aboutPrinter,
  aboutPrinting,
  aboutScreen,
  aboutStamp,
  aboutTotal,
} from "./welcome-about-scene";

const SEGMENTS = ABOUT_SEGMENTS.map((_, index) => index);

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(ABOUT_BEATS.end / step) + 1 }, (_, index) => index * step);
}

/** The first moment `value` becomes positive. */
function firstPositive(value: (elapsed: number) => number): number {
  return moments(5).find((elapsed) => value(elapsed) > 0) ?? Infinity;
}

/** The first moment `value` reaches 1. */
function firstComplete(value: (elapsed: number) => number): number {
  return moments(5).find((elapsed) => value(elapsed) >= 1) ?? Infinity;
}

describe("welcome about scene", () => {
  it("lays the receipt out as a header, the features between rules, the total, and a barcode, between blank paper", () => {
    expect(ABOUT_SEGMENTS).toEqual([
      "lead",
      "header",
      "rule",
      ...Array.from({ length: ABOUT_RECEIPT_ITEMS }, () => "item"),
      "rule",
      "total",
      "barcode",
      "tail",
    ]);
  });

  it("starts empty: no terminal, screen, paper, or stamp", () => {
    expect(aboutPrinter(0)).toBe(0);
    expect(aboutScreen(0)).toBe(0);
    expect(aboutTotal(0)).toBe(0);
    expect(aboutConfirmRing(0)).toBe(0);
    expect(aboutConfirmCheck(0)).toBe(0);
    for (const segment of SEGMENTS) expect(aboutFeed(0, segment)).toBe(0);
    expect(aboutStamp(0)).toBe(0);
    expect(aboutImpact(0)).toBe(0);
    expect(aboutPrinting(0)).toBe(false);
  });

  it("lights the screen with the total only once the terminal is in place", () => {
    const placed = firstComplete(aboutPrinter);
    expect(firstPositive(aboutScreen)).toBeGreaterThanOrEqual(placed);
    expect(firstPositive(aboutTotal)).toBeGreaterThanOrEqual(placed);
    expect(aboutTotal(firstComplete(aboutScreen))).toBe(1);
  });

  it("confirms after the total has been shown, drawing the ring before the check", () => {
    const totalShown = firstComplete(aboutTotal);
    const ringStart = firstPositive(aboutConfirmRing);
    expect(ringStart).toBeGreaterThan(totalShown);
    expect(firstPositive(aboutConfirmCheck)).toBeGreaterThan(ringStart);
    expect(aboutTotal(ringStart)).toBeLessThan(1);
    expect(firstComplete(aboutConfirmCheck)).toBe(ABOUT_BEATS.confirmed);
  });

  it("prints only once the payment is confirmed", () => {
    expect(firstPositive((elapsed) => aboutFeed(elapsed, ABOUT_SEGMENTS.length - 1))).toBeGreaterThan(ABOUT_BEATS.confirmed);
    expect(aboutPrinting(ABOUT_BEATS.confirmed)).toBe(false);
  });

  it("reveals the segments one at a time, from the bottom up", () => {
    for (const segment of SEGMENTS.slice(0, -1)) {
      expect(firstPositive((elapsed) => aboutFeed(elapsed, segment)))
        .toBeGreaterThan(firstComplete((elapsed) => aboutFeed(elapsed, segment + 1)));
    }
  });

  it("is printing exactly while paper feeds", () => {
    for (const elapsed of moments()) {
      const feeding = SEGMENTS.some((segment) => {
        const fed = aboutFeed(elapsed, segment);
        return fed > 0 && fed < 1;
      });
      if (feeding) expect(aboutPrinting(elapsed)).toBe(true);
    }
    expect(aboutPrinting(ABOUT_BEATS.printEnd)).toBe(false);
  });

  it("stamps the receipt only after the whole receipt is printed", () => {
    const printed = firstComplete((elapsed) => aboutFeed(elapsed, 0));
    expect(firstPositive(aboutStamp)).toBeGreaterThan(printed);
  });

  it("shakes the receipt only once the stamp lands, and then settles", () => {
    const landed = firstComplete(aboutStamp);
    expect(firstPositive(aboutImpact)).toBe(landed);
    expect(aboutImpact(landed)).toBe(1);
    expect(aboutImpact(ABOUT_BEATS.end)).toBe(0);
  });

  it("ends with the payment confirmed and the receipt printed, stamped, and at rest", () => {
    const end = ABOUT_BEATS.end;
    expect(aboutPrinter(end)).toBe(1);
    expect(aboutScreen(end)).toBe(1);
    expect(aboutTotal(end)).toBe(0);
    expect(aboutConfirmRing(end)).toBe(1);
    expect(aboutConfirmCheck(end)).toBe(1);
    for (const segment of SEGMENTS) expect(aboutFeed(end, segment)).toBe(1);
    expect(aboutStamp(end)).toBe(1);
    expect(aboutImpact(end)).toBe(0);
    expect(aboutPrinting(end)).toBe(false);
  });

  it("keeps every value between 0 and 1 throughout", () => {
    for (const elapsed of moments()) {
      const values = [
        aboutPrinter(elapsed),
        aboutScreen(elapsed),
        aboutTotal(elapsed),
        aboutConfirmRing(elapsed),
        aboutConfirmCheck(elapsed),
        aboutStamp(elapsed),
        aboutImpact(elapsed),
        ...SEGMENTS.map((segment) => aboutFeed(elapsed, segment)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });
});
