import { describe, expect, it } from "vitest";

import {
  checkerboardBackground,
  checkerboardBackgroundForCells,
} from "./display";

describe("checkerboardBackground", () => {
  it("starts with the muted checker color instead of the light cell", () => {
    expect(checkerboardBackground(11)).toContain(
      "var(--editor-chrome-checker-a) 25%",
    );
  });

  it("uses the requested tile size for square swatch grids", () => {
    expect(checkerboardBackground(11)).toContain("/11px 11px");
  });

  it("sizes square swatches to a requested checker count", () => {
    expect(checkerboardBackgroundForCells(24, 3)).toContain("/16px 16px");
  });
});
