import { describe, expect, it } from "vitest";
import { pickMusicFrequencyTooltipPosition } from "./frequency-tooltip-position";

const base = {
  anchorTop: 280,
  anchorBottom: 304,
  anchorLeft: 120,
  tooltipWidth: 256,
  tooltipHeight: 160,
  viewportWidth: 800,
  viewportHeight: 600,
};

describe("Mix frequency tooltip position", () => {
  it("prefers above when the whole hint fits", () => {
    expect(pickMusicFrequencyTooltipPosition(base)).toEqual({ top: 112, left: 120, placement: "above" });
  });

  it("falls below when there is not enough room above", () => {
    expect(pickMusicFrequencyTooltipPosition({ ...base, anchorTop: 100, anchorBottom: 124 }))
      .toEqual({ top: 132, left: 120, placement: "below" });
  });

  it("keeps the hint inside the viewport near an edge", () => {
    expect(pickMusicFrequencyTooltipPosition({ ...base, anchorLeft: 760, viewportWidth: 800 }).left).toBe(536);
    expect(pickMusicFrequencyTooltipPosition({ ...base, anchorTop: 20, anchorBottom: 44, viewportHeight: 170 }).top).toBe(8);
  });
});
