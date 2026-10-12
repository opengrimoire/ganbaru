import { describe, expect, it } from "vitest";
import {
  customFieldOptionConnectorPath,
  customFieldOptionConnectorStyle,
  customFieldOptionConnectorViewBox,
} from "./custom-field-option-connector";

describe("customFieldOptionConnectorStyle", () => {
  it("returns the positioned connector box for an option group", () => {
    expect(customFieldOptionConnectorStyle(2)).toBe(
      "left: -1.275rem; top: -0.775rem; width: 1.425rem; height: 4.425rem",
    );
  });
});

describe("customFieldOptionConnectorViewBox", () => {
  it("matches the connector dimensions", () => {
    expect(customFieldOptionConnectorViewBox(2)).toBe("-0.15 -0.775 1.425 4.425");
  });
});

describe("customFieldOptionConnectorPath", () => {
  it("does not draw a connector without options", () => {
    expect(customFieldOptionConnectorPath(0)).toBe("");
  });

  it("draws one branch per option", () => {
    expect(customFieldOptionConnectorPath(2)).toBe(
      "M 0 -0.625 L 0 3.05 M 0 0.55 C 0 0.901 0.144 1 0.45 1 L 1.125 1 M 0 3.05 C 0 3.401 0.144 3.5 0.45 3.5 L 1.125 3.5",
    );
  });
});
