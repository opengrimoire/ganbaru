import { describe, expect, it } from "vitest";
import {
  contrastRatio,
  hexToRgb,
  relativeLuminance,
} from "$lib/color/math";
import { darkTheme, lightTheme } from "$lib/themes";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  projectTagColorDotStyle,
  projectTagColorSwatchClass,
  projectLifecycleBadgeClass,
  projectPriorityBadgeClass,
  projectPriorityLabel,
  projectStatusBadgeClass,
  projectStatusBadgeStyle,
  projectTaskArchivedBadgeClass,
} from "./display";

const t = ((key, ...args) => translateFromPartialCatalog({}, key, ...args)) as Translate;

function styleHex(style: string, property: "background-color" | "color"): string {
  const match = new RegExp(`(?:^|;\\s*)${property}: (#[0-9a-f]{6});`, "i").exec(style);
  if (!match) throw new Error(`Missing ${property} in style: ${style}`);
  return match[1];
}

describe("project display helpers", () => {
  it("returns localized priority labels", () => {
    expect(projectPriorityLabel("urgent", t)).toBe("Urgent");
    expect(projectPriorityLabel("normal", t)).toBe("Normal");
  });

  it("returns lifecycle and task badge classes", () => {
    expect(projectLifecycleBadgeClass("hidden")).toContain("amber");
    expect(projectLifecycleBadgeClass("archived")).toContain("muted");
    expect(projectTaskArchivedBadgeClass({ archivedAt: "2026-06-21T00:00:00.000Z" })).toContain("muted");
    expect(projectTaskArchivedBadgeClass({ archivedAt: undefined })).toContain("emerald");
  });

  it("returns status and priority tone classes", () => {
    expect(projectStatusBadgeClass(undefined)).toContain("muted");
    expect(projectStatusBadgeClass({
      id: "status-a",
      projectId: "project-a",
      name: "Blocked",
      category: "blocked",
      color: 2,
      sortOrder: 1000,
      terminal: false,
      createdAt: "2026-06-21T00:00:00.000Z",
      updatedAt: "2026-06-21T00:00:00.000Z",
    })).toBe("");
    expect(projectPriorityBadgeClass("high")).toContain("amber");
  });

  it("returns readable status badge colors from the event palette", () => {
    const style = projectStatusBadgeStyle({
      id: "status-a",
      projectId: "project-a",
      name: "Blocked",
      category: "blocked",
      color: 2,
      sortOrder: 1000,
      terminal: false,
      createdAt: "2026-06-21T00:00:00.000Z",
      updatedAt: "2026-06-21T00:00:00.000Z",
    }, lightTheme);

    expect(style).toContain("background-color: #");
    expect(style).toContain("color: #");
    expect(style).not.toContain("border-color");
  });

  it("keeps blocked status text light and subtly red-tinted on dark themes", () => {
    const style = projectStatusBadgeStyle({
      id: "status-a",
      projectId: "project-a",
      name: "Blocked",
      category: "blocked",
      color: 2,
      sortOrder: 1000,
      terminal: false,
      createdAt: "2026-06-21T00:00:00.000Z",
      updatedAt: "2026-06-21T00:00:00.000Z",
    }, darkTheme);
    const background = styleHex(style, "background-color");
    const foreground = styleHex(style, "color");
    const foregroundRgb = hexToRgb(foreground);
    if (!foregroundRgb) throw new Error(`Invalid foreground color: ${foreground}`);

    expect(relativeLuminance(foreground)).toBeGreaterThan(relativeLuminance(background));
    expect(contrastRatio(background, foreground)).toBeGreaterThanOrEqual(4.5);
    expect(foregroundRgb.r).toBeGreaterThan(foregroundRgb.g);
    expect(foregroundRgb.r).toBeGreaterThan(foregroundRgb.b);
    expect(foregroundRgb.r - foregroundRgb.g).toBeLessThanOrEqual(40);
  });

  it("keeps blocked status text dark and subtly red-tinted on light themes", () => {
    const style = projectStatusBadgeStyle({
      id: "status-a",
      projectId: "project-a",
      name: "Blocked",
      category: "blocked",
      color: 2,
      sortOrder: 1000,
      terminal: false,
      createdAt: "2026-06-21T00:00:00.000Z",
      updatedAt: "2026-06-21T00:00:00.000Z",
    }, lightTheme);
    const background = styleHex(style, "background-color");
    const foreground = styleHex(style, "color");
    const foregroundRgb = hexToRgb(foreground);
    if (!foregroundRgb) throw new Error(`Invalid foreground color: ${foreground}`);

    expect(relativeLuminance(foreground)).toBeLessThan(relativeLuminance(background));
    expect(contrastRatio(background, foreground)).toBeGreaterThanOrEqual(4.5);
    expect(foregroundRgb.r).toBeGreaterThan(foregroundRgb.g);
    expect(foregroundRgb.r).toBeGreaterThan(foregroundRgb.b);
    expect(foregroundRgb.r - foregroundRgb.g).toBeLessThanOrEqual(60);
  });

  it("returns label swatch styling only when a palette color exists", () => {
    expect(projectTagColorSwatchClass(undefined)).toContain("muted");
    expect(projectTagColorDotStyle(undefined, lightTheme)).toBe("");
    expect(projectTagColorDotStyle(2, lightTheme)).toMatch(/^background-color: #[0-9a-f]{6};$/i);
  });

});
