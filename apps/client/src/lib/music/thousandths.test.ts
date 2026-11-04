import { describe, expect, it } from "vitest";
import { fromThousandths, toThousandths } from "./thousandths";

describe("music thousandths", () => {
  it("converts stored thousandths to multipliers", () => {
    expect(fromThousandths(0)).toBe(0);
    expect(fromThousandths(250)).toBe(0.25);
    expect(fromThousandths(2000)).toBe(2);
  });

  it("rounds multipliers to the nearest stored thousandth", () => {
    expect(toThousandths(0.3)).toBe(300);
    expect(toThousandths(1 + 0.07)).toBe(1070);
    expect(toThousandths(0.12345)).toBe(123);
    expect(toThousandths(0.1236)).toBe(124);
  });

  it("round trips every slider percent exactly", () => {
    for (let percent = 0; percent <= 200; percent += 1) {
      const level = percent / 100;
      expect(fromThousandths(toThousandths(level))).toBe(level);
    }
  });
});
