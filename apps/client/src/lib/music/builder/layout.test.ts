import { describe, expect, it } from "vitest";
import { projectMusicBuilderLayout } from "./layout";

describe("music builder layout projection", () => {
  it("keeps the contextual panel beside the workspace when they fit", () => {
    expect(projectMusicBuilderLayout({ width: 1200, height: 680 })).toMatchObject({
      mode: "wide",
      contextPanelPresentation: "persistent",
      dockPresentation: "sidebar",
      comfortable: true,
    });
  });

  it("keeps the contextual panel until the workspace stops fitting", () => {
    expect(projectMusicBuilderLayout({ width: 700, height: 420 })).toMatchObject({
      mode: "medium",
      contextPanelPresentation: "persistent",
      dockPresentation: "sidebar",
    });
    expect(projectMusicBuilderLayout({ width: 1000, height: 280 }).mode).toBe("narrow");
  });

  it("keeps a recoverable page projection at the app floor", () => {
    expect(projectMusicBuilderLayout({ width: 280, height: 180 })).toEqual({
      mode: "narrow",
      contextPanelPresentation: "sheet",
      dockPresentation: "bottom",
      comfortable: false,
    });
  });
});
