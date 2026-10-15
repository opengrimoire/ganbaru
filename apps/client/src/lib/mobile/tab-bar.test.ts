import { describe, expect, it } from "vitest";
import { mobileTabBarSlots, mobileTabCountLabel } from "./tab-bar";

const tabs = ["a", "b", "c", "d", "e", "f"].map((id) => ({ id }));
const ids = (slots: readonly { id: string }[]) => slots.map((slot) => slot.id);

describe("mobileTabBarSlots", () => {
  it("shows the first tabs in order while the active one is among them", () => {
    expect(ids(mobileTabBarSlots(tabs, "c"))).toEqual(["a", "b", "c", "d"]);
    expect(ids(mobileTabBarSlots(tabs, "d"))).toEqual(["a", "b", "c", "d"]);
  });

  it("puts an active tab past the bar in the last slot", () => {
    expect(ids(mobileTabBarSlots(tabs, "f"))).toEqual(["a", "b", "c", "f"]);
    expect(ids(mobileTabBarSlots(tabs, "e"))).toEqual(["a", "b", "c", "e"]);
  });

  it("shows every tab when there are fewer than the bar holds", () => {
    expect(ids(mobileTabBarSlots(tabs.slice(0, 2), "b"))).toEqual(["a", "b"]);
  });

  it("keeps the leading tabs when the active id is unknown", () => {
    expect(ids(mobileTabBarSlots(tabs, "missing"))).toEqual(["a", "b", "c", "d"]);
  });

  it("follows a custom capacity and shows nothing without room", () => {
    expect(ids(mobileTabBarSlots(tabs, "e", 1))).toEqual(["e"]);
    expect(mobileTabBarSlots(tabs, "a", 0)).toEqual([]);
  });
});

describe("mobileTabCountLabel", () => {
  const format = (value: number) => new Intl.NumberFormat("en").format(value);

  it("prints counts up to 99 and caps larger ones", () => {
    expect(mobileTabCountLabel(1, format)).toBe("1");
    expect(mobileTabCountLabel(99, format)).toBe("99");
    expect(mobileTabCountLabel(100, format)).toBe("99+");
  });
});
