import { describe, expect, it } from "vitest";
import { buildEventPanelInitKey } from "./init-key";

describe("buildEventPanelInitKey", () => {
  it("keeps create identity tied to the panel session", () => {
    const key = buildEventPanelInitKey({
      parked: false,
      mode: "create",
      panelSessionKey: 7,
    });

    expect(key).toBe(buildEventPanelInitKey({
      parked: false,
      mode: "create",
      panelSessionKey: 7,
    }));
    expect(key).not.toBe(buildEventPanelInitKey({
      parked: false,
      mode: "create",
      panelSessionKey: 8,
    }));
  });

  it("separates active and parked panel identities", () => {
    expect(buildEventPanelInitKey({
      parked: false,
      mode: "create",
      panelSessionKey: 7,
    })).not.toBe(buildEventPanelInitKey({
      parked: true,
      mode: "create",
      panelSessionKey: 7,
    }));
  });

  it("includes the event id for edit identities", () => {
    expect(buildEventPanelInitKey({
      parked: false,
      mode: "edit",
      panelSessionKey: 7,
      eventId: "event-a",
    })).not.toBe(buildEventPanelInitKey({
      parked: false,
      mode: "edit",
      panelSessionKey: 7,
      eventId: "event-b",
    }));
  });
});
