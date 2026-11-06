import { describe, expect, it } from "vitest";
import {
  ABOUT_BEATS,
  ABOUT_ITEM_COUNT,
  aboutCodeReveal,
  aboutComputerItem,
  aboutEdit,
  aboutLink,
  aboutPackets,
  aboutPhone,
  aboutPhoneItem,
  aboutSweeping,
} from "./welcome-about-scene";

const ITEMS = Array.from({ length: ABOUT_ITEM_COUNT }, (_, index) => index);

/** Every millisecond step of the scene, for properties that must hold throughout. */
function moments(step = 10): number[] {
  return Array.from({ length: Math.floor(ABOUT_BEATS.end / step) + 1 }, (_, index) => index * step);
}

describe("welcome about scene", () => {
  it("starts on an empty computer with no phone, link, changes in flight, or code", () => {
    for (const index of ITEMS) {
      expect(aboutComputerItem(0, index)).toBe(0);
      expect(aboutPhoneItem(0, index)).toBe(0);
    }
    expect(aboutPhone(0)).toBe(0);
    expect(aboutLink(0)).toBe(0);
    expect(aboutPackets(0)).toEqual([]);
    expect(aboutCodeReveal(0)).toBe(0);
  });

  it("fills the computer one item after another before the phone arrives", () => {
    for (const index of ITEMS.slice(1)) {
      const halfway = moments().find((moment) => aboutComputerItem(moment, index) >= 0.5) ?? Infinity;
      expect(aboutComputerItem(halfway, index - 1)).toBeGreaterThan(aboutComputerItem(halfway, index));
    }
    expect(aboutComputerItem(ABOUT_BEATS.phoneIn, ABOUT_ITEM_COUNT - 1)).toBe(1);
    expect(aboutLink(ABOUT_BEATS.linkIn)).toBe(0);
    expect(aboutPhone(ABOUT_BEATS.linkIn)).toBe(1);
  });

  it("shows an item on the phone only after a change has travelled all the way to it", () => {
    for (const index of ITEMS) {
      const shown = moments(1).find((moment) => aboutPhoneItem(moment, index) > 0) ?? Infinity;
      const inFlight = moments(1).filter((moment) => moment < shown && aboutPackets(moment).length > 0);
      expect(inFlight.length).toBeGreaterThan(0);
      expect(aboutLink(shown)).toBe(1);
    }
  });

  it("only lists changes on their way, each between the two devices", () => {
    for (const moment of moments()) {
      for (const position of aboutPackets(moment)) {
        expect(position).toBeGreaterThanOrEqual(0);
        expect(position).toBeLessThanOrEqual(1);
      }
    }
    expect(aboutPackets(ABOUT_BEATS.syncStart)).toEqual([]);
    expect(aboutPackets(ABOUT_BEATS.syncStart + 100)).toHaveLength(1);
  });

  it("sends the edit from the phone back to the computer once every item has synced", () => {
    expect(ITEMS.every((index) => aboutPhoneItem(ABOUT_BEATS.editStart, index) === 1)).toBe(true);
    const departure = moments(1).find((moment) => aboutEdit(moment).phone === 1) ?? Infinity;
    const path = moments(1)
      .filter((moment) => moment > departure && aboutEdit(moment).computer === 0)
      .flatMap((moment) => aboutPackets(moment));
    expect(path[0]).toBeGreaterThan(0.9);
    expect(path.at(-1)).toBeLessThan(0.1);
    expect(path.every((position, index) => index === 0 || position <= path[index - 1])).toBe(true);
  });

  it("highlights the edit while it travels and lets it fade before the sweep", () => {
    const travelling = moments().find((moment) => aboutPackets(moment).length > 0 && moment > ABOUT_BEATS.editStart) ?? 0;
    expect(aboutEdit(travelling).highlight).toBe(1);
    expect(aboutEdit(ABOUT_BEATS.sweepIn)).toEqual({ phone: 1, computer: 1, highlight: 0 });
  });

  it("opens the code across the scene, holds it, and closes it again", () => {
    expect(aboutCodeReveal(ABOUT_BEATS.sweepIn)).toBe(0);
    expect(aboutSweeping(ABOUT_BEATS.sweepIn + 300)).toBe(true);
    expect(aboutCodeReveal(ABOUT_BEATS.sweepOut)).toBe(1);
    expect(aboutSweeping(ABOUT_BEATS.sweepOut)).toBe(false);
    expect(aboutCodeReveal(ABOUT_BEATS.sweepOut + 300)).toBeLessThan(1);
    const reveals = moments().filter((moment) => moment >= ABOUT_BEATS.sweepIn).map(aboutCodeReveal);
    expect(reveals.every((reveal) => reveal >= 0 && reveal <= 1)).toBe(true);
  });

  it("ends on both devices synced and showing the app, which is the reduced-motion still", () => {
    const end = ABOUT_BEATS.end;
    for (const index of ITEMS) {
      expect(aboutComputerItem(end, index)).toBe(1);
      expect(aboutPhoneItem(end, index)).toBe(1);
    }
    expect(aboutPhone(end)).toBe(1);
    expect(aboutLink(end)).toBe(1);
    expect(aboutPackets(end)).toEqual([]);
    expect(aboutEdit(end)).toEqual({ phone: 1, computer: 1, highlight: 0 });
    expect(aboutCodeReveal(end)).toBe(0);
    expect(aboutSweeping(end)).toBe(false);
  });
});
