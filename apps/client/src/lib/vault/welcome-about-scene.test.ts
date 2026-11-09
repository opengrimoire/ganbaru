import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_CURSOR,
  ABOUT_CURSOR_TARGETS,
  ABOUT_REPOSITORY,
  ABOUT_REPOSITORY_URL,
  aboutPop,
  aboutPress,
  aboutSparks,
  aboutStarred,
} from "./welcome-about-scene";
import { welcomeCursorFrame, welcomeCursorHovered, welcomeCursorScriptOrdered } from "./welcome-cursor";
import { FILM_TAP_MS } from "./welcome-film";

/** Every step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(ABOUT_BEATS.end / step) + 1 }, (_, index) => index * step);
}

/** The first moment `value` becomes positive. */
function firstPositive(value: (elapsed: number) => number): number {
  return moments(5).find((elapsed) => value(elapsed) > 0) ?? Infinity;
}

describe("welcome about scene", () => {
  it("points at the repository it shows", () => {
    expect(ABOUT_REPOSITORY_URL).toBe(`https://${ABOUT_REPOSITORY.host}/${ABOUT_REPOSITORY.owner}/${ABOUT_REPOSITORY.name}`);
    expect(new URL(ABOUT_REPOSITORY_URL).pathname).toBe(`/${ABOUT_REPOSITORY.owner}/${ABOUT_REPOSITORY.name}`);
  });

  it("starts unstarred, with the button at rest", () => {
    expect(aboutPress(0)).toBe(0);
    expect(aboutStarred(0)).toBe(false);
    expect(aboutPop(0)).toBe(0);
    expect(aboutSparks(0)).toBe(0);
  });

  it("presses Star only once the tap begins", () => {
    expect(firstPositive(aboutPress)).toBeGreaterThanOrEqual(ABOUT_BEATS.starTap);
    expect(ABOUT_BEATS.starTap).toBeGreaterThan(0);
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

  it("ends starred, with the button at rest", () => {
    const end = ABOUT_BEATS.end;
    expect(aboutPress(end)).toBe(0);
    expect(aboutStarred(end)).toBe(true);
    expect(aboutPop(end)).toBe(1);
    expect(aboutSparks(end)).toBe(1);
  });

  it("keeps every value between 0 and 1 throughout", () => {
    for (const elapsed of moments()) {
      for (const value of [aboutPress(elapsed), aboutPop(elapsed), aboutSparks(elapsed)]) {
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
  });

  it("keeps the pointer on Star, shown, from the first frame to the last", () => {
    expect(ABOUT_CURSOR.every((stop) => stop.target === ABOUT_CURSOR_TARGETS.star)).toBe(true);
    for (const elapsed of moments()) {
      expect(welcomeCursorHovered(ABOUT_CURSOR, elapsed)).toBe(ABOUT_CURSOR_TARGETS.star);
      expect(welcomeCursorFrame(ABOUT_CURSOR, elapsed)).toMatchObject({ shown: 1, away: false });
    }
  });
});
