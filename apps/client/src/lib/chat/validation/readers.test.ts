import { describe, expect, it } from "vitest";
import { readJsonValue } from "./readers";
import { parseChatChangeNotification, parseChatLiveFollow, parseChatRemoteApprovalOutcome, parseChatTimelineDelta } from "./reads";

describe("readJsonValue", () => {
  it("keeps untrusted __proto__ keys as data without changing object prototypes", () => {
    const parsed: unknown = JSON.parse('{"nested": {"__proto__": {"isAdmin": true}}, "__proto__": []}');

    const value = readJsonValue(parsed, "payload");

    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("expected an object");
    expect(Object.getPrototypeOf(value)).toBe(Object.prototype);
    expect(Object.getOwnPropertyDescriptor(value, "__proto__")?.value).toEqual([]);
    const nested = value.nested;
    if (nested === null || typeof nested !== "object" || Array.isArray(nested)) throw new Error("expected an object");
    expect(Object.getPrototypeOf(nested)).toBe(Object.prototype);
    expect("isAdmin" in nested).toBe(false);
    expect(Object.getOwnPropertyDescriptor(nested, "__proto__")?.value).toEqual({ isAdmin: true });
  });

  it("rejects values that are not JSON", () => {
    expect(() => readJsonValue({ value: Number.NaN }, "payload")).toThrow("payload.value is not valid JSON");
    expect(() => readJsonValue({ value: () => undefined }, "payload")).toThrow();
  });
});

describe("parseChatChangeNotification", () => {
  const notification = {
    threadId: "thread-1",
    sequence: 7,
    sinceSequence: 5,
    revision: 3,
    changedProjectionKeys: ["messages", "pending_requests"],
  };

  it("reads the covered sequence range and the projection keys", () => {
    expect(parseChatChangeNotification(notification)).toEqual(notification);
  });

  it("reads a projection key this build does not know as unknown", () => {
    const parsed = parseChatChangeNotification({ ...notification, changedProjectionKeys: ["messages", "reactions"] });

    expect(parsed.changedProjectionKeys).toEqual(["messages", "unknown"]);
  });

  it("rejects a first sequence after the last one", () => {
    expect(() => parseChatChangeNotification({ ...notification, sinceSequence: 8 }))
      .toThrow("sinceSequence is after");
  });
});

describe("parseChatTimelineDelta", () => {
  it("reads removed ids and the paging flags", () => {
    const delta = parseChatTimelineDelta({
      threadId: "thread-1",
      items: [],
      turns: [],
      removedItemIds: ["activity-1"],
      changeCursor: 12,
      hasMore: true,
      resetRequired: false,
      threadRevision: 4,
    });

    expect(delta.removedItemIds).toEqual(["activity-1"]);
    expect(delta.hasMore).toBe(true);
  });

  it("rejects an empty removed id", () => {
    expect(() => parseChatTimelineDelta({
      threadId: "thread-1",
      items: [],
      turns: [],
      removedItemIds: [""],
      changeCursor: 12,
      hasMore: false,
      resetRequired: false,
      threadRevision: 4,
    })).toThrow("removedItemIds[0] is required");
  });
});

describe("parseChatLiveFollow", () => {
  const read = {
    assignmentId: "assignment-1",
    runId: null,
    threadId: null,
    turnId: null,
    providerFamilyId: null,
    modelId: null,
    items: [],
    turns: [],
    removedItemIds: [],
    changeCursor: 0,
    hasMore: false,
    resetRequired: false,
    settled: true,
  };

  it("reads a page and the answers that carry no page", () => {
    expect(parseChatLiveFollow({ kind: "page", read })).toEqual({ kind: "page", read });
    expect(parseChatLiveFollow({ kind: "unreachable" })).toEqual({ kind: "unreachable" });
    expect(parseChatLiveFollow({ kind: "notServed" })).toEqual({ kind: "notServed" });
  });

  it("rejects an unknown answer or a page without its flags", () => {
    expect(() => parseChatLiveFollow({ kind: "stream" })).toThrow("Chat live follow.kind has an unsupported value");
    const { settled: _settled, ...withoutSettled } = read;
    expect(() => parseChatLiveFollow({ kind: "page", read: withoutSettled })).toThrow("Chat live follow.read.settled");
    expect(() => parseChatLiveFollow({ kind: "page", read: { ...read, removedItemIds: [""] } })).toThrow();
  });
});

describe("parseChatRemoteApprovalOutcome", () => {
  it("reads each outcome and rejects an unknown one", () => {
    for (const kind of ["resolved", "stale", "notServed", "unreachable"] as const) {
      expect(parseChatRemoteApprovalOutcome({ kind })).toEqual({ kind });
    }
    expect(() => parseChatRemoteApprovalOutcome({ kind: "allowed" })).toThrow("Chat remote approval outcome.kind");
    expect(() => parseChatRemoteApprovalOutcome(null)).toThrow();
  });
});
