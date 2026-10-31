import { describe, expect, it } from "vitest";
import { canonicalProjectCustomFieldNumber, compareProjectCustomFieldNumbers, uniqueProjectCustomFieldName } from "./custom-fields";

describe("uniqueProjectCustomFieldName", () => {
  it("keeps an unused name", () => {
    expect(uniqueProjectCustomFieldName("Points", [{ name: "Owner" }], "en")).toBe("Points");
  });

  it("skips names already used in any letter case or with surrounding spaces", () => {
    const fields = [{ name: "number" }, { name: " Number (2) " }, { name: "Number (4)" }];
    expect(uniqueProjectCustomFieldName("Number", fields, "en")).toBe("Number (3)");
  });

  it("trims the base and formats the suffix for the locale", () => {
    const fields = Array.from({ length: 1000 }, (_, index) => ({ name: index === 0 ? "Total" : `Total (${new Intl.NumberFormat("es").format(index + 1)})` }));
    expect(uniqueProjectCustomFieldName(" Total ", fields, "es")).toBe(`Total (${new Intl.NumberFormat("es").format(1001)})`);
  });
});

describe("canonicalProjectCustomFieldNumber", () => {
  it("canonicalizes leading zeros, trailing fractional zeros, and negative zero", () => {
    expect(canonicalProjectCustomFieldNumber("007.500")).toBe("7.5");
    expect(canonicalProjectCustomFieldNumber("-0.0")).toBe("0");
    expect(canonicalProjectCustomFieldNumber("  12  ")).toBe("12");
    expect(canonicalProjectCustomFieldNumber("1.230000000")).toBe("1.23");
  });

  it("accepts the signed 64-bit millionths range and rejects values beyond it", () => {
    expect(canonicalProjectCustomFieldNumber("9223372036854.775807")).toBe("9223372036854.775807");
    expect(canonicalProjectCustomFieldNumber("-9223372036854.775808")).toBe("-9223372036854.775808");
    expect(canonicalProjectCustomFieldNumber("9223372036854.775808")).toBeNull();
    expect(canonicalProjectCustomFieldNumber("-9223372036854.775809")).toBeNull();
  });

  it("rejects more than six significant decimals", () => {
    expect(canonicalProjectCustomFieldNumber("0.123456")).toBe("0.123456");
    expect(canonicalProjectCustomFieldNumber("0.1234567")).toBeNull();
  });

  it("rejects signs, exponents, separators, and partial decimals", () => {
    for (const text of ["+1", "1e3", "1,5", ".5", "1.", "1 000", "", "-", "Infinity", "NaN"]) {
      expect(canonicalProjectCustomFieldNumber(text)).toBeNull();
    }
  });
});

describe("compareProjectCustomFieldNumbers", () => {
  it("compares exactly where floating point would round", () => {
    expect(compareProjectCustomFieldNumbers("9223372036854.775806", "9223372036854.775807")).toBeLessThan(0);
    expect(compareProjectCustomFieldNumbers("-2", "1.5")).toBeLessThan(0);
    expect(compareProjectCustomFieldNumbers("10", "9.999999")).toBeGreaterThan(0);
    expect(compareProjectCustomFieldNumbers("1.5", "1.5")).toBe(0);
  });
});
