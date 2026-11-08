import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_CURSOR,
  ABOUT_CURSOR_TARGETS,
  ABOUT_FILE_ROWS,
  ABOUT_REPOSITORY,
  ABOUT_REPOSITORY_URL,
  aboutPage,
  aboutPop,
  aboutPress,
  aboutRow,
  aboutSparks,
  aboutStarred,
} from "./welcome-about-scene";
import { welcomeCursorFrame, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";

const ROWS = Array.from({ length: ABOUT_FILE_ROWS }, (_, index) => index);

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
  it("points at the repository it shows", () => {
    expect(ABOUT_REPOSITORY_URL).toBe(`https://${ABOUT_REPOSITORY.host}/${ABOUT_REPOSITORY.owner}/${ABOUT_REPOSITORY.name}`);
    expect(new URL(ABOUT_REPOSITORY_URL).pathname).toBe(`/${ABOUT_REPOSITORY.owner}/${ABOUT_REPOSITORY.name}`);
  });

  it("starts empty and unstarred", () => {
    expect(aboutPage(0)).toBe(0);
    for (const row of ROWS) expect(aboutRow(0, row)).toBe(0);
    expect(aboutPress(0)).toBe(0);
    expect(aboutStarred(0)).toBe(false);
    expect(aboutPop(0)).toBe(0);
    expect(aboutSparks(0)).toBe(0);
  });

  it("brings the rows in one after another while the page settles", () => {
    expect(firstPositive((elapsed) => aboutRow(elapsed, 0))).toBeGreaterThan(firstPositive(aboutPage));
    for (const row of ROWS.slice(1)) {
      expect(firstPositive((elapsed) => aboutRow(elapsed, row))).toBeGreaterThan(firstPositive((elapsed) => aboutRow(elapsed, row - 1)));
    }
    expect(firstComplete((elapsed) => aboutRow(elapsed, ABOUT_FILE_ROWS - 1))).toBe(ABOUT_BEATS.rowsEnd);
  });

  it("taps Star only after every row is shown", () => {
    expect(firstPositive(aboutPress)).toBeGreaterThanOrEqual(ABOUT_BEATS.starTap);
  });

  it("stars the repository while the button is pressed, and keeps it starred", () => {
    expect(aboutStarred(ABOUT_BEATS.starred - 1)).toBe(false);
    expect(aboutPress(ABOUT_BEATS.starred)).toBe(1);
    for (const elapsed of moments().filter((moment) => moment >= ABOUT_BEATS.starred)) expect(aboutStarred(elapsed)).toBe(true);
  });

  it("pops the star and sends its sparks only once it is starred", () => {
    expect(firstPositive(aboutPop)).toBeGreaterThan(ABOUT_BEATS.starTap);
    expect(firstPositive(aboutSparks)).toBeGreaterThan(ABOUT_BEATS.starTap);
    expect(aboutPop(ABOUT_BEATS.starred)).toBe(0);
  });

  it("ends with the page shown, the repository starred, and the button at rest", () => {
    const end = ABOUT_BEATS.end;
    expect(aboutPage(end)).toBe(1);
    for (const row of ROWS) expect(aboutRow(end, row)).toBe(1);
    expect(aboutPress(end)).toBe(0);
    expect(aboutStarred(end)).toBe(true);
    expect(aboutPop(end)).toBe(1);
    expect(aboutSparks(end)).toBe(1);
  });

  it("keeps every value between 0 and 1 throughout", () => {
    for (const elapsed of moments()) {
      const values = [
        aboutPage(elapsed),
        aboutPress(elapsed),
        aboutPop(elapsed),
        aboutSparks(elapsed),
        ...ROWS.map((row) => aboutRow(elapsed, row)),
      ];
      for (const value of values) {
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(1);
      }
    }
  });

  it("clicks Star with the pointer, and the button sinks under it", () => {
    expect(welcomeCursorScriptOrdered(ABOUT_CURSOR)).toBe(true);
    const clicks = ABOUT_CURSOR.filter((stop) => stop.press);
    expect(clicks.map((stop) => [stop.target, stop.press])).toEqual([
      [ABOUT_CURSOR_TARGETS.star, { start: ABOUT_BEATS.starTap, end: ABOUT_BEATS.starTap + FILM_TAP_MS }],
    ]);
    for (const elapsed of [ABOUT_BEATS.starTap, ABOUT_BEATS.starred, ABOUT_BEATS.starTap + FILM_TAP_MS]) {
      expect(welcomeCursorHovered(ABOUT_CURSOR, elapsed)).toBe(ABOUT_CURSOR_TARGETS.star);
    }
  });

  it("keeps the pointer on the address bar until every row is shown, and on the star until the scene ends", () => {
    expect(welcomeCursorHovered(ABOUT_CURSOR, 0)).toBe(ABOUT_CURSOR_TARGETS.address);
    expect(welcomeCursorHovered(ABOUT_CURSOR, ABOUT_BEATS.rowsEnd)).toBe(ABOUT_CURSOR_TARGETS.address);
    expect(ABOUT_CURSOR[1]?.depart).toBeGreaterThanOrEqual(ABOUT_BEATS.rowsEnd);
    expect(welcomeCursorFrame(ABOUT_CURSOR, ABOUT_BEATS.end)).toMatchObject({ shown: 1, away: false });
  });
});
