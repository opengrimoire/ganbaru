import { describe, expect, it } from "vitest";
import { mapChatMessageProvenance } from "./provenance";

describe("mapChatMessageProvenance", () => {
  it("reads each sealing device", () => {
    const read = mapChatMessageProvenance({
      devices: [
        { deviceId: "a", deviceLabel: "Desk", ownDevice: false },
        { deviceId: "b", deviceLabel: null, ownDevice: true },
      ],
    });

    expect(read.devices.map((device) => device.deviceLabel)).toEqual(["Desk", null]);
    expect(read.devices[1]?.ownDevice).toBe(true);
  });

  it("keeps a message without a sync record empty", () => {
    expect(mapChatMessageProvenance({ devices: [] }).devices).toEqual([]);
  });

  it("rejects a device without an id", () => {
    expect(() => mapChatMessageProvenance({ devices: [{ deviceLabel: "Desk", ownDevice: false }] }))
      .toThrow("chat message provenance.devices[0]");
  });
});
