import { describe, expect, it } from "vitest";
import { DEFAULT_SPLIT_RATIO, MIN_SPLIT_RATIO } from "./model";
import {
  MIN_SPLIT_PANE_HEIGHT,
  MIN_SPLIT_PANE_WIDTH,
  fitSplitRatio,
  minSplitPaneLength,
  workspaceSplitLayout,
} from "./split-layout";

const GAP = 6;
const wide = MIN_SPLIT_PANE_WIDTH * 2 + GAP;
const tall = MIN_SPLIT_PANE_HEIGHT * 2 + GAP;

describe("workspaceSplitLayout", () => {
  it("keeps the chosen orientation while both panes fit that way", () => {
    expect(workspaceSplitLayout("side-by-side", wide, tall, GAP)).toBe("side-by-side");
    expect(workspaceSplitLayout("stacked", wide, tall, GAP)).toBe("stacked");
  });

  it("turns the other way when only that orientation fits", () => {
    expect(workspaceSplitLayout("side-by-side", wide - 1, tall, GAP)).toBe("stacked");
    expect(workspaceSplitLayout("stacked", wide, tall - 1, GAP)).toBe("side-by-side");
  });

  it("shows only the focused tab when neither orientation fits, as in the smallest window", () => {
    expect(workspaceSplitLayout("side-by-side", 280, 180, GAP)).toBe("single");
    expect(workspaceSplitLayout("stacked", wide - 1, tall - 1, GAP)).toBe("single");
  });
});

describe("fitSplitRatio", () => {
  it("keeps a ratio that leaves both panes long enough", () => {
    expect(fitSplitRatio(0.4, 1_200, MIN_SPLIT_PANE_WIDTH)).toBe(0.4);
  });

  it("stops either pane at its minimum length", () => {
    expect(fitSplitRatio(0.1, 1_000, 300)).toBeCloseTo(0.3);
    expect(fitSplitRatio(0.95, 1_000, 300)).toBeCloseTo(0.7);
  });

  it("keeps the stored bounds when the minimum length is small", () => {
    expect(fitSplitRatio(0.01, 10_000, 100)).toBe(MIN_SPLIT_RATIO);
  });

  it("shares the space equally when two minimum panes do not fit", () => {
    expect(fitSplitRatio(0.2, 500, 300)).toBe(DEFAULT_SPLIT_RATIO);
  });

  it("treats a ratio that is not a number as the default", () => {
    expect(fitSplitRatio(Number.NaN, 1_000, 100)).toBe(DEFAULT_SPLIT_RATIO);
  });
});

describe("minSplitPaneLength", () => {
  it("measures along the split's axis", () => {
    expect(minSplitPaneLength("side-by-side")).toBe(MIN_SPLIT_PANE_WIDTH);
    expect(minSplitPaneLength("stacked")).toBe(MIN_SPLIT_PANE_HEIGHT);
  });
});
