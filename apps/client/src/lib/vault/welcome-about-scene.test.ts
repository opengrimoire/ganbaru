import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_DEVICE_COUNT,
  ABOUT_EDIT_ORIGINS,
  ABOUT_INITIAL_ROWS,
  ABOUT_LINKS,
  ABOUT_ROW_COUNT,
  aboutDevice,
  aboutLink,
  aboutRow,
  aboutRowGlow,
  aboutTravel,
} from "./welcome-about-scene";

const DEVICES = Array.from({ length: ABOUT_DEVICE_COUNT }, (_, index) => index);
const ROWS = Array.from({ length: ABOUT_ROW_COUNT }, (_, index) => index);
const EDITS = ABOUT_EDIT_ORIGINS.map((_, index) => index);

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
  it("starts empty: no devices, links, travelling things, or rows", () => {
    for (const device of DEVICES) {
      expect(aboutDevice(0, device)).toBe(0);
      for (const row of ROWS) expect(aboutRow(0, device, row)).toBe(0);
    }
    for (const link of ABOUT_LINKS.keys()) expect(aboutLink(0, link)).toBe(0);
    for (const edit of EDITS) expect(aboutTravel(0, edit)).toBe(0);
  });

  it("links every pair of devices directly, so none stands between two others", () => {
    const pairs = new Set(ABOUT_LINKS.map(([from, to]) => `${Math.min(from, to)}-${Math.max(from, to)}`));
    for (const from of DEVICES) {
      for (const to of DEVICES.filter((device) => device > from)) expect(pairs.has(`${from}-${to}`)).toBe(true);
    }
  });

  it("lets every device add something", () => {
    expect(new Set(ABOUT_EDIT_ORIGINS)).toEqual(new Set(DEVICES));
  });

  it("draws the links only between devices that are already there", () => {
    const allShown = Math.max(...DEVICES.map((device) => firstComplete((elapsed) => aboutDevice(elapsed, device))));
    for (const link of ABOUT_LINKS.keys()) {
      expect(firstPositive((elapsed) => aboutLink(elapsed, link))).toBeGreaterThanOrEqual(allShown);
    }
  });

  it("shows the same things on every device as it appears", () => {
    for (const device of DEVICES) {
      for (const row of ROWS.slice(0, ABOUT_INITIAL_ROWS)) {
        for (const elapsed of moments(50)) expect(aboutRow(elapsed, device, row)).toBe(aboutDevice(elapsed, device));
      }
    }
  });

  it("sends each new thing only once it is on its origin and every link is drawn", () => {
    const linksDrawn = Math.max(...[...ABOUT_LINKS.keys()].map((link) => firstComplete((elapsed) => aboutLink(elapsed, link))));
    for (const edit of EDITS) {
      const row = ABOUT_INITIAL_ROWS + edit;
      const origin = ABOUT_EDIT_ORIGINS[edit];
      const departure = firstPositive((elapsed) => aboutTravel(elapsed, edit));
      expect(departure).toBeGreaterThanOrEqual(firstComplete((elapsed) => aboutRow(elapsed, origin, row)));
      expect(firstPositive((elapsed) => aboutRow(elapsed, origin, row))).toBeGreaterThanOrEqual(linksDrawn);
    }
  });

  it("lands each new thing on the other devices only when it arrives", () => {
    for (const edit of EDITS) {
      const row = ABOUT_INITIAL_ROWS + edit;
      const arrival = firstComplete((elapsed) => aboutTravel(elapsed, edit));
      for (const device of DEVICES.filter((device) => device !== ABOUT_EDIT_ORIGINS[edit])) {
        expect(firstPositive((elapsed) => aboutRow(elapsed, device, row))).toBeGreaterThanOrEqual(arrival);
      }
    }
  });

  it("adds one thing at a time, each after the previous one has landed everywhere", () => {
    for (const edit of EDITS.slice(1)) {
      const previous = ABOUT_INITIAL_ROWS + edit - 1;
      const landed = Math.max(...DEVICES.map((device) => firstComplete((elapsed) => aboutRow(elapsed, device, previous))));
      const origin = ABOUT_EDIT_ORIGINS[edit];
      expect(firstPositive((elapsed) => aboutRow(elapsed, origin, ABOUT_INITIAL_ROWS + edit))).toBeGreaterThanOrEqual(landed);
    }
  });

  it("marks only new rows, and only for a moment", () => {
    for (const device of DEVICES) {
      for (const row of ROWS.slice(0, ABOUT_INITIAL_ROWS)) {
        for (const elapsed of moments(50)) expect(aboutRowGlow(elapsed, device, row)).toBe(0);
      }
      for (const row of ROWS.slice(ABOUT_INITIAL_ROWS)) {
        expect(Math.max(...moments().map((elapsed) => aboutRowGlow(elapsed, device, row)))).toBeGreaterThan(0.5);
      }
    }
  });

  it("ends with every device linked, holding every thing, and at rest", () => {
    const end = ABOUT_BEATS.end;
    for (const link of ABOUT_LINKS.keys()) expect(aboutLink(end, link)).toBe(1);
    for (const edit of EDITS) expect(aboutTravel(end, edit)).toBe(1);
    for (const device of DEVICES) {
      expect(aboutDevice(end, device)).toBe(1);
      for (const row of ROWS) {
        expect(aboutRow(end, device, row)).toBe(1);
        expect(aboutRowGlow(end, device, row)).toBe(0);
      }
    }
  });

  it("keeps every value between 0 and 1 throughout", () => {
    for (const elapsed of moments()) {
      const values = [
        ...[...ABOUT_LINKS.keys()].map((link) => aboutLink(elapsed, link)),
        ...EDITS.map((edit) => aboutTravel(elapsed, edit)),
        ...DEVICES.flatMap((device) => [
          aboutDevice(elapsed, device),
          ...ROWS.flatMap((row) => [aboutRow(elapsed, device, row), aboutRowGlow(elapsed, device, row)]),
        ]),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });
});
