import { describe, expect, it } from "vitest";
import { translate } from "$lib/i18n/translator.svelte";
import {
  focusActivityChanged,
  focusActivityLine,
  focusActivityLines,
  parseFocusDeviceActivity,
  type FocusOtherDeviceActivity,
} from "./device-activity";

function wire(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    device: { deviceId: "laptop", deviceLabel: "Laptop", ownDevice: false },
    state: "focus",
    occurrenceId: "occurrence",
    eventTitle: "Write",
    phaseStartedAtMs: 1_000,
    phaseEndsAtMs: 2_000,
    expiresAtMs: 3_000,
    updatedAtMs: 1_500,
    ...overrides,
  };
}

function activity(overrides: Partial<FocusOtherDeviceActivity> = {}): FocusOtherDeviceActivity {
  return { ...parseFocusDeviceActivity([wire()])[0], ...overrides };
}

describe("parseFocusDeviceActivity", () => {
  it("accepts the native activity shape with nullable presentation fields", () => {
    const rows = parseFocusDeviceActivity([
      wire(),
      wire({ device: { deviceId: "phone", deviceLabel: null, ownDevice: false }, state: "paused", occurrenceId: null, eventTitle: null, phaseStartedAtMs: null, phaseEndsAtMs: null }),
    ]);
    expect(rows[0]).toEqual({
      device: { deviceId: "laptop", deviceLabel: "Laptop", ownDevice: false },
      state: "focus",
      occurrenceId: "occurrence",
      eventTitle: "Write",
      phaseStartedAtMs: 1_000,
      phaseEndsAtMs: 2_000,
      expiresAtMs: 3_000,
      updatedAtMs: 1_500,
    });
    expect(rows[1].device.deviceLabel).toBeNull();
    expect(rows[1].eventTitle).toBeNull();
  });

  it("rejects idle rows, unknown states, and malformed fields", () => {
    expect(() => parseFocusDeviceActivity([wire({ state: "idle" })])).toThrow("state");
    expect(() => parseFocusDeviceActivity([wire({ state: "running" })])).toThrow("state");
    expect(() => parseFocusDeviceActivity([wire({ expiresAtMs: 1.5 })])).toThrow("expiresAtMs");
    expect(() => parseFocusDeviceActivity([wire({ device: { deviceId: "laptop" } })])).toThrow("device");
    expect(() => parseFocusDeviceActivity({})).toThrow("array");
  });
});

describe("focusActivityLine", () => {
  it("names the device and its event for each state", () => {
    expect(focusActivityLine(translate, activity())).toBe("Laptop is focusing on Write");
    expect(focusActivityLine(translate, activity({ state: "long_break" }))).toBe("Laptop is on a break from Write");
    expect(focusActivityLine(translate, activity({ state: "paused" }))).toBe("Laptop paused Write");
  });

  it("falls back for unlabeled devices and blank or missing event titles", () => {
    const unlabeled = activity({ device: { deviceId: "phone", deviceLabel: null, ownDevice: false } });
    expect(focusActivityLine(translate, unlabeled)).toBe("Another device is focusing on Write");
    expect(focusActivityLine(translate, activity({ eventTitle: "  " }))).toBe("Laptop is focusing");
    expect(focusActivityLine(translate, activity({ state: "short_break", eventTitle: null }))).toBe("Laptop is on a break");
  });
});

describe("focusActivityLines", () => {
  it("keeps each device's newest row, newest device first", () => {
    const lines = focusActivityLines(translate, [
      activity({ eventTitle: "Old", updatedAtMs: 1_000 }),
      activity({ device: { deviceId: "phone", deviceLabel: "Phone", ownDevice: false }, updatedAtMs: 1_200 }),
      activity({ eventTitle: "New", updatedAtMs: 2_000 }),
    ]);
    expect(lines).toEqual(["Laptop is focusing on New", "Phone is focusing on Write"]);
    expect(focusActivityLines(translate, [])).toEqual([]);
  });
});

describe("focusActivityChanged", () => {
  it("follows only applied notices that carry the Focus topic", () => {
    expect(focusActivityChanged(["calendar_events", "focus"])).toBe(true);
    expect(focusActivityChanged(["pomodoro_device_activity"])).toBe(false);
    expect(focusActivityChanged([])).toBe(false);
  });
});
