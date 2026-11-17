import { describe, expect, it } from "vitest";
import { SETTINGS_SECTIONS, settingsSectionsForShell } from "./section-catalog";

describe("settingsSectionsForShell", () => {
  it("keeps the complete settings order on desktop", () => {
    expect(settingsSectionsForShell("desktop")).toBe(SETTINGS_SECTIONS);
  });

  it("removes only keyboard shortcuts and agents from mobile settings", () => {
    const desktopIds = settingsSectionsForShell("desktop").map((section) => section.id);
    const mobileIds = settingsSectionsForShell("mobile").map((section) => section.id);

    expect(mobileIds).not.toContain("shortcuts");
    expect(mobileIds).not.toContain("agents");
    expect(mobileIds).toContain("chat");
    expect(mobileIds).toEqual(desktopIds.filter((id) => id !== "shortcuts" && id !== "agents"));
  });

  it("places agents right after chat", () => {
    const desktopIds = SETTINGS_SECTIONS.map((section) => section.id);
    expect(desktopIds.indexOf("agents")).toBe(desktopIds.indexOf("chat") + 1);
  });
});
