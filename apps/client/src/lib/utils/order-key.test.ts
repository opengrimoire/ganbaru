import { describe, expect, it } from "vitest";
import { compareCodeUnits, compareOrderKeyThenId } from "./order-key";

describe("order key comparison", () => {
  it("orders by code unit instead of locale collation", () => {
    expect(["a", "B", "Z", "a0", "A"].sort(compareCodeUnits)).toEqual(["A", "B", "Z", "a", "a0"]);
    expect(compareCodeUnits("a0", "a0")).toBe(0);
  });

  it("breaks equal order keys by id", () => {
    const rows = [
      { key: "a1", id: "b" },
      { key: "a0", id: "z" },
      { key: "a1", id: "a" },
    ];
    rows.sort((left, right) => compareOrderKeyThenId(left.key, left.id, right.key, right.id));
    expect(rows.map((row) => `${row.key}:${row.id}`)).toEqual(["a0:z", "a1:a", "a1:b"]);
  });
});
