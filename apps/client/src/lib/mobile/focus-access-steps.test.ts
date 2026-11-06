import { describe, expect, it } from "vitest";
import {
  FOCUS_ACCESS_GUIDES,
  focusAccessGranted,
  focusAccessSensitive,
  focusAccessSteps,
  type FocusAccessStatuses,
} from "./focus-access-steps";

function statuses(overrides: {
  permission?: "granted" | "denied" | "prompt";
  exactAlarmRequired?: boolean;
  exactAlarmGranted?: boolean;
  autostart?: boolean;
  exempt?: boolean;
  restricted?: boolean;
  usageAccess?: boolean;
  accessibility?: boolean;
} = {}): FocusAccessStatuses {
  return {
    background: {
      manufacturer: "Xiaomi",
      appLabel: "Ganbaru",
      autostartSettingsAvailable: overrides.autostart ?? true,
      backgroundRestricted: overrides.restricted ?? false,
      batteryOptimizationExempt: overrides.exempt ?? false,
    },
    notifications: {
      permission: overrides.permission ?? "prompt",
      channel: { exists: true, enabled: true, soundConfigured: true },
      exactAlarm: {
        apiLevel: 33,
        required: overrides.exactAlarmRequired ?? true,
        granted: overrides.exactAlarmGranted ?? false,
      },
    },
    distractions: {
      usageAccess: overrides.usageAccess ?? false,
      accessibility: overrides.accessibility ?? false,
    },
  };
}

describe("Focus access steps", () => {
  it("asks for everything a fresh install lacks, delivery first and sensitive access last", () => {
    expect(focusAccessSteps(statuses())).toEqual([
      "notifications",
      "exact-alarm",
      "autostart",
      "battery",
      "usage-access",
      "app-blocking",
    ]);
  });

  it("leaves out access that is already on and controls this device does not have", () => {
    expect(focusAccessSteps(statuses({
      permission: "granted",
      exactAlarmRequired: false,
      autostart: false,
      exempt: true,
      usageAccess: true,
    }))).toEqual(["app-blocking"]);
  });

  it("asks for nothing once every readable access is on and the device has no autostart switch", () => {
    expect(focusAccessSteps(statuses({
      permission: "granted",
      exactAlarmGranted: true,
      autostart: false,
      exempt: true,
      usageAccess: true,
      accessibility: true,
    }))).toEqual([]);
  });

  it("keeps the battery step while Android restricts the app even when it is exempt from optimization", () => {
    expect(focusAccessSteps(statuses({ exempt: true, restricted: true }))).toContain("battery");
  });

  it("keeps steps whose status could not be read, but skips controls whose presence is unknown", () => {
    expect(focusAccessSteps({ background: null, notifications: null, distractions: null })).toEqual([
      "notifications",
      "battery",
      "usage-access",
      "app-blocking",
    ]);
  });

  it("never reports autostart as on, because Android has no public query for it", () => {
    expect(focusAccessGranted("autostart", statuses({ autostart: true }))).toBeNull();
  });

  it("reports detectable access as on or off, and an unexempted battery as unknown", () => {
    const fresh = statuses();
    expect(focusAccessGranted("notifications", fresh)).toBe(false);
    expect(focusAccessGranted("usage-access", statuses({ usageAccess: true }))).toBe(true);
    expect(focusAccessGranted("battery", fresh)).toBeNull();
    expect(focusAccessGranted("battery", statuses({ exempt: true }))).toBe(true);
  });

  it("marks only usage access and app blocking as sensitive", () => {
    expect(focusAccessSensitive("usage-access")).toBe(true);
    expect(focusAccessSensitive("app-blocking")).toBe(true);
    expect(focusAccessSensitive("battery")).toBe(false);
  });

  it("gives every guide phases that hold long enough to read, ending on the longest hold", () => {
    for (const phases of Object.values(FOCUS_ACCESS_GUIDES)) {
      expect(phases.length).toBeGreaterThan(1);
      const durations = phases.map((phase) => phase.durationMs);
      expect(Math.min(...durations)).toBeGreaterThanOrEqual(700);
      expect(durations.at(-1)).toBe(Math.max(...durations.slice(1)));
    }
  });
});
