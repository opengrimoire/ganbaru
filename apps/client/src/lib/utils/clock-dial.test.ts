import { describe, expect, it } from "vitest";
import {
  CLOCK_DIAL_RING_RADIUS,
  clockDialAngle,
  clockDialHand,
  clockDialItems,
  clockTimeFromValue,
  continuousClockRotation,
  displayClockHour,
  formatClockValue,
  parseTypedClockTime,
  pickClockDialTime,
  stepClockTime,
  withClockPeriod,
} from "./clock-dial";

const RADIUS = 100;

/** The pointer offset of a point at the given angle and fraction of the dial radius. */
function pointAt(angle: number, fraction: number): { dx: number; dy: number } {
  const radians = (angle * Math.PI) / 180;
  return { dx: Math.sin(radians) * RADIUS * fraction, dy: -Math.cos(radians) * RADIUS * fraction };
}

describe("clockDialAngle", () => {
  it("measures clockwise from 12 o'clock in screen coordinates", () => {
    expect(clockDialAngle(0, -10)).toBe(0);
    expect(clockDialAngle(10, 0)).toBe(90);
    expect(clockDialAngle(0, 10)).toBe(180);
    expect(clockDialAngle(-10, 0)).toBe(270);
  });
});

describe("clockDialItems", () => {
  it("lays out a 24-hour face with 0 to 11 outside and 12 to 23 inside, both starting at the top", () => {
    const items = clockDialItems("hour", "24h");
    expect(items).toHaveLength(24);
    expect(items[0]).toEqual({ value: 0, label: 0, ring: "outer", angle: 0 });
    expect(items[12]).toEqual({ value: 12, label: 12, ring: "inner", angle: 0 });
    expect(items.find((item) => item.value === 18)).toEqual({ value: 18, label: 18, ring: "inner", angle: 180 });
  });

  it("labels a 12-hour face 12 then 1 to 11 and selects in the given half of the day", () => {
    const morning = clockDialItems("hour", "12h", "am");
    expect(morning.map((item) => item.label)).toEqual([12, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    expect(morning[0].value).toBe(0);
    const evening = clockDialItems("hour", "12h", "pm");
    expect(evening[0].value).toBe(12);
    expect(evening[6].value).toBe(18);
    expect(evening.every((item) => item.ring === "outer")).toBe(true);
  });

  it("labels every fifth minute", () => {
    const items = clockDialItems("minute", "24h");
    expect(items.map((item) => item.label)).toEqual([0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
    expect(items[3].angle).toBe(90);
  });
});

describe("pickClockDialTime", () => {
  const base = { radius: RADIUS, time: { hour: 9, minute: 30 } };

  it("picks the outer ring hour near the edge and the inner ring hour near the center", () => {
    const outer = pickClockDialTime({ ...base, ...pointAt(180, CLOCK_DIAL_RING_RADIUS.outer), mode: "hour", format: "24h" });
    expect(outer).toEqual({ hour: 6, minute: 30 });
    const inner = pickClockDialTime({ ...base, ...pointAt(180, CLOCK_DIAL_RING_RADIUS.inner), mode: "hour", format: "24h" });
    expect(inner).toEqual({ hour: 18, minute: 30 });
  });

  it("treats 12 o'clock as 0 outside and 12 inside", () => {
    expect(pickClockDialTime({ ...base, ...pointAt(355, 0.8), mode: "hour", format: "24h" }).hour).toBe(0);
    expect(pickClockDialTime({ ...base, ...pointAt(5, 0.4), mode: "hour", format: "24h" }).hour).toBe(12);
  });

  it("snaps hours to the nearest mark", () => {
    expect(pickClockDialTime({ ...base, ...pointAt(44, 0.8), mode: "hour", format: "24h" }).hour).toBe(1);
    expect(pickClockDialTime({ ...base, ...pointAt(46, 0.8), mode: "hour", format: "24h" }).hour).toBe(2);
  });

  it("keeps the half of the day on a 12-hour face, whatever the distance", () => {
    const afternoon = { ...base, time: { hour: 15, minute: 0 } };
    expect(pickClockDialTime({ ...afternoon, ...pointAt(0, 0.4), mode: "hour", format: "12h" }).hour).toBe(12);
    expect(pickClockDialTime({ ...afternoon, ...pointAt(240, 0.8), mode: "hour", format: "12h" }).hour).toBe(20);
    expect(pickClockDialTime({ ...base, ...pointAt(0, 0.8), mode: "hour", format: "12h" }).hour).toBe(0);
  });

  it("selects every minute between the labeled marks and wraps 60 to 0", () => {
    expect(pickClockDialTime({ ...base, ...pointAt(42, 0.8), mode: "minute", format: "24h" })).toEqual({ hour: 9, minute: 7 });
    expect(pickClockDialTime({ ...base, ...pointAt(358, 0.5), mode: "minute", format: "24h" }).minute).toBe(0);
  });

  it("snaps minutes to the nearest step, wrapping past the last mark to 0", () => {
    const tap = { ...base, mode: "minute" as const, format: "24h" as const, minuteStep: 5 };
    expect(pickClockDialTime({ ...tap, ...pointAt(42, 0.8) })).toEqual({ hour: 9, minute: 5 });
    expect(pickClockDialTime({ ...tap, ...pointAt(48, 0.8) }).minute).toBe(10);
    expect(pickClockDialTime({ ...tap, ...pointAt(350, 0.8) }).minute).toBe(0);
    expect(pickClockDialTime({ ...tap, ...pointAt(340, 0.8) }).minute).toBe(55);
  });

  it("leaves hours unaffected by the minute step", () => {
    expect(pickClockDialTime({ ...base, ...pointAt(42, 0.8), mode: "hour", format: "24h", minuteStep: 5 })).toEqual({ hour: 1, minute: 30 });
  });
});

describe("clockDialHand", () => {
  it("points inside for afternoon hours on a 24-hour face only", () => {
    expect(clockDialHand({ hour: 18, minute: 0 }, "hour", "24h")).toEqual({ angle: 180, ring: "inner" });
    expect(clockDialHand({ hour: 18, minute: 0 }, "hour", "12h")).toEqual({ angle: 180, ring: "outer" });
    expect(clockDialHand({ hour: 0, minute: 0 }, "hour", "24h")).toEqual({ angle: 0, ring: "outer" });
  });

  it("points at the exact minute", () => {
    expect(clockDialHand({ hour: 3, minute: 37 }, "minute", "24h")).toEqual({ angle: 222, ring: "outer" });
  });
});

describe("stepClockTime", () => {
  it("wraps hours across the day on a 24-hour face", () => {
    expect(stepClockTime({ hour: 23, minute: 5 }, "hour", "24h", 1)).toEqual({ hour: 0, minute: 5 });
    expect(stepClockTime({ hour: 0, minute: 5 }, "hour", "24h", -1)).toEqual({ hour: 23, minute: 5 });
  });

  it("wraps hours inside the half of the day on a 12-hour face", () => {
    expect(stepClockTime({ hour: 23, minute: 0 }, "hour", "12h", 1)).toEqual({ hour: 12, minute: 0 });
    expect(stepClockTime({ hour: 0, minute: 0 }, "hour", "12h", -1)).toEqual({ hour: 11, minute: 0 });
  });

  it("wraps minutes without changing the hour", () => {
    expect(stepClockTime({ hour: 8, minute: 59 }, "minute", "24h", 1)).toEqual({ hour: 8, minute: 0 });
  });
});

describe("continuousClockRotation", () => {
  it("turns the short way across 12 o'clock", () => {
    expect(continuousClockRotation(330, 30)).toBe(390);
    expect(continuousClockRotation(390, 330)).toBe(330);
    expect(continuousClockRotation(0, 270)).toBe(-90);
    expect(continuousClockRotation(720, 90)).toBe(810);
  });
});

describe("12-hour periods", () => {
  it("shows midnight and noon as 12", () => {
    expect(displayClockHour(0, "12h")).toBe(12);
    expect(displayClockHour(12, "12h")).toBe(12);
    expect(displayClockHour(13, "12h")).toBe(1);
    expect(displayClockHour(0, "24h")).toBe(0);
  });

  it("moves a time between halves of the day", () => {
    expect(withClockPeriod({ hour: 9, minute: 15 }, "pm")).toEqual({ hour: 21, minute: 15 });
    expect(withClockPeriod({ hour: 12, minute: 0 }, "am")).toEqual({ hour: 0, minute: 0 });
    expect(withClockPeriod({ hour: 0, minute: 0 }, "pm")).toEqual({ hour: 12, minute: 0 });
  });
});

describe("parseTypedClockTime", () => {
  it("reads 24-hour fields", () => {
    expect(parseTypedClockTime("7", "05", "24h", "am")).toEqual({ hour: 7, minute: 5 });
    expect(parseTypedClockTime("23", "59", "24h", "am")).toEqual({ hour: 23, minute: 59 });
  });

  it("reads 12-hour fields in the chosen half of the day", () => {
    expect(parseTypedClockTime("12", "30", "12h", "am")).toEqual({ hour: 0, minute: 30 });
    expect(parseTypedClockTime("12", "30", "12h", "pm")).toEqual({ hour: 12, minute: 30 });
    expect(parseTypedClockTime("7", "0", "12h", "pm")).toEqual({ hour: 19, minute: 0 });
  });

  it("rejects empty, non-numeric, and out-of-range fields", () => {
    expect(parseTypedClockTime("", "30", "24h", "am")).toBeNull();
    expect(parseTypedClockTime("1a", "30", "24h", "am")).toBeNull();
    expect(parseTypedClockTime("24", "00", "24h", "am")).toBeNull();
    expect(parseTypedClockTime("10", "60", "24h", "am")).toBeNull();
    expect(parseTypedClockTime("0", "10", "12h", "am")).toBeNull();
    expect(parseTypedClockTime("13", "10", "12h", "pm")).toBeNull();
  });
});

describe("clock values", () => {
  it("reads a value, then its fallback, then midnight", () => {
    expect(clockTimeFromValue("08:45", "12:00")).toEqual({ hour: 8, minute: 45 });
    expect(clockTimeFromValue("", "12:00")).toEqual({ hour: 12, minute: 0 });
    expect(clockTimeFromValue("", "")).toEqual({ hour: 0, minute: 0 });
  });

  it("formats the canonical value", () => {
    expect(formatClockValue({ hour: 7, minute: 5 })).toBe("07:05");
  });
});
