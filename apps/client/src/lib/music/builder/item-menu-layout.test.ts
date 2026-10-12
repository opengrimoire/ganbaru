import { describe, expect, it } from "vitest";
import { projectMusicItemMenuLayout } from "./item-menu-layout";

describe("music item menu layout", () => {
  it("places the menu below its trigger when it fits", () => {
    expect(projectMusicItemMenuLayout({
      anchor: { left: 700, right: 730, top: 100, bottom: 130 },
      viewportWidth: 900,
      viewportHeight: 700,
      panelWidth: 224,
      panelHeight: 260,
    })).toEqual({ panelLeft: 506, panelTop: 134 });
  });

  it("moves above the trigger and clamps at narrow viewport edges", () => {
    expect(projectMusicItemMenuLayout({
      anchor: { left: 250, right: 278, top: 150, bottom: 178 },
      viewportWidth: 280,
      viewportHeight: 180,
      panelWidth: 220,
      panelHeight: 150,
    })).toEqual({ panelLeft: 54, panelTop: 6 });
  });
});
