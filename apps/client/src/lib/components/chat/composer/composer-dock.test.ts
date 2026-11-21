import { describe, expect, it } from "vitest";
import { chatComposerDockMetrics } from "./composer-dock";

describe("chatComposerDockMetrics", () => {
  it("reserves the whole dock and starts the backdrop where the composer's top corner curve ends", () => {
    expect(chatComposerDockMetrics({ dockTop: 500, dockBottom: 620, composerTop: 508, cornerRadius: 20.8 })).toEqual({
      height: 120,
      backdropTop: 28.8,
    });
  });

  it("starts the backdrop below a scheduled message summary above the composer", () => {
    const withoutSummary = chatComposerDockMetrics({ dockTop: 500, dockBottom: 620, composerTop: 508, cornerRadius: 20 });
    const withSummary = chatComposerDockMetrics({ dockTop: 460, dockBottom: 620, composerTop: 548, cornerRadius: 20 });
    expect(withSummary.height - withoutSummary.height).toBe(40);
    expect(withSummary.backdropTop).toBe(108);
  });

  it("covers the whole dock when no composer box is rendered", () => {
    expect(chatComposerDockMetrics({ dockTop: 500, dockBottom: 560, composerTop: null, cornerRadius: 0 })).toEqual({
      height: 60,
      backdropTop: 0,
    });
  });

  it("clamps the backdrop and the height to the dock", () => {
    expect(chatComposerDockMetrics({ dockTop: 500, dockBottom: 520, composerTop: 510, cornerRadius: 40 })).toEqual({
      height: 20,
      backdropTop: 20,
    });
    expect(chatComposerDockMetrics({ dockTop: 500, dockBottom: 490, composerTop: 480, cornerRadius: -4 })).toEqual({
      height: 0,
      backdropTop: 0,
    });
  });
});
