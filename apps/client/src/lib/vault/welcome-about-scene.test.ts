import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_CODE_LINES,
  ABOUT_COPY_ORDER,
  ABOUT_RECIPIENT_COUNT,
  ABOUT_SCREEN_ROWS,
  aboutCard,
  aboutCodeLine,
  aboutCopy,
  aboutRecipient,
  aboutScreenRow,
} from "./welcome-about-scene";

const LINES = Array.from({ length: ABOUT_CODE_LINES }, (_, index) => index);
const DEVICES = Array.from({ length: ABOUT_RECIPIENT_COUNT }, (_, index) => index);
const ROWS = Array.from({ length: ABOUT_SCREEN_ROWS }, (_, index) => index);

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
  it("starts empty: no code, devices, copies, or items", () => {
    expect(aboutCard(0)).toBe(0);
    for (const line of LINES) expect(aboutCodeLine(0, line)).toBe(0);
    for (const device of DEVICES) {
      expect(aboutRecipient(0, device)).toBe(0);
      expect(aboutCopy(0, device)).toBe(0);
      for (const row of ROWS) expect(aboutScreenRow(0, device, row)).toBe(0);
    }
  });

  it("writes the code out line by line before any device appears", () => {
    const lastLine = firstComplete((elapsed) => aboutCodeLine(elapsed, ABOUT_CODE_LINES - 1));
    const firstDevice = Math.min(...DEVICES.map((device) => firstPositive((elapsed) => aboutRecipient(elapsed, device))));
    expect(lastLine).toBeLessThanOrEqual(firstDevice);
    for (const line of LINES.slice(1)) {
      expect(firstPositive((elapsed) => aboutCodeLine(elapsed, line)))
        .toBeGreaterThan(firstPositive((elapsed) => aboutCodeLine(elapsed, line - 1)));
    }
  });

  it("sends each copy only to a device that is already there", () => {
    for (const device of DEVICES) {
      const shown = firstComplete((elapsed) => aboutRecipient(elapsed, device));
      expect(firstPositive((elapsed) => aboutCopy(elapsed, device))).toBeGreaterThanOrEqual(shown);
    }
  });

  it("fills a device with its own items only after its copy arrives", () => {
    for (const device of DEVICES) {
      const arrival = firstComplete((elapsed) => aboutCopy(elapsed, device));
      for (const row of ROWS) {
        expect(firstPositive((elapsed) => aboutScreenRow(elapsed, device, row))).toBeGreaterThanOrEqual(arrival);
      }
    }
  });

  it("sends the copies from the middle outwards", () => {
    const departures = ABOUT_COPY_ORDER.map((device) => firstPositive((elapsed) => aboutCopy(elapsed, device)));
    expect(departures).toEqual([...departures].sort((left, right) => left - right));
    expect(new Set(ABOUT_COPY_ORDER)).toEqual(new Set(DEVICES));
  });

  it("ends with the code written and every device holding its copy and its own items", () => {
    const end = ABOUT_BEATS.end;
    expect(aboutCard(end)).toBe(1);
    for (const line of LINES) expect(aboutCodeLine(end, line)).toBe(1);
    for (const device of DEVICES) {
      expect(aboutRecipient(end, device)).toBe(1);
      expect(aboutCopy(end, device)).toBe(1);
      for (const row of ROWS) expect(aboutScreenRow(end, device, row)).toBe(1);
    }
  });

  it("keeps every value between 0 and 1 throughout", () => {
    for (const elapsed of moments()) {
      const values = [
        aboutCard(elapsed),
        ...LINES.map((line) => aboutCodeLine(elapsed, line)),
        ...DEVICES.flatMap((device) => [
          aboutRecipient(elapsed, device),
          aboutCopy(elapsed, device),
          ...ROWS.map((row) => aboutScreenRow(elapsed, device, row)),
        ]),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });
});
