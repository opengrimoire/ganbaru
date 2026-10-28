import { describe, expect, it } from "vitest";
import { mapChatConflicts } from "./conflicts";

const device = { deviceId: "device-a", deviceLabel: null, ownDevice: false };

describe("chat conflict mapping", () => {
  it("maps a message conflict with its channel and a channel conflict with its topic", () => {
    const read = mapChatConflicts([
      {
        target: "message",
        id: "item-1",
        context: "design",
        versions: [{ version: "ab", device, editedAtMs: 5, displayed: true, text: "First words", detail: null }],
      },
      {
        target: "channel",
        id: "channel-1",
        context: null,
        versions: [{ version: "cd", device, editedAtMs: 7, displayed: false, text: "research", detail: "" }],
      },
    ]);
    expect(read[0]?.context).toBe("design");
    expect(read[0]?.versions[0]?.detail).toBeNull();
    expect(read[1]?.versions[0]?.detail).toBe("");
    expect(read[1]?.versions[0]?.displayed).toBe(false);
  });

  it("rejects a row kind the native side does not define", () => {
    expect(() => mapChatConflicts([{ target: "draft", id: "d", context: null, versions: [] }]))
      .toThrow("chat conflicts[0].target has an unsupported value");
  });

  it("rejects a version without a device", () => {
    expect(() => mapChatConflicts([{
      target: "teammate",
      id: "t",
      context: null,
      versions: [{ version: "ab", editedAtMs: 1, displayed: true, text: "Reviewer", detail: "Review" }],
    }])).toThrow("chat conflicts[0].versions[0].device");
  });
});
