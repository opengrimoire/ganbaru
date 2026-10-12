import { describe, expect, it } from "vitest";
import { uniqueProjectCustomFieldName } from "./custom-fields";

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
