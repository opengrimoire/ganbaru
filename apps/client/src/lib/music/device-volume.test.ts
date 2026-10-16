import { describe, expect, it } from "vitest";
import { parseDeviceVolumeState } from "./device-volume";

describe("parseDeviceVolumeState", () => {
  it("converts the device step to a level", () => {
    expect(parseDeviceVolumeState("0,15")).toBe(0);
    expect(parseDeviceVolumeState("6,15")).toBeCloseTo(0.4);
    expect(parseDeviceVolumeState("15,15")).toBe(1);
  });

  it("caps an index above the device maximum", () => {
    expect(parseDeviceVolumeState("30,25")).toBe(1);
  });

  it("treats an empty state as a device without app volume control", () => {
    expect(parseDeviceVolumeState("")).toBeNull();
  });

  it("rejects malformed states and zero step counts", () => {
    for (const state of ["7", "7,", ",15", "-1,15", "1.5,15", "a,b", "3,0", "1,2,3"]) {
      expect(parseDeviceVolumeState(state)).toBeNull();
    }
  });
});
