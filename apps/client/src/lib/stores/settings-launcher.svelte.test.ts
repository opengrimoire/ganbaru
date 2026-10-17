import { describe, expect, it, vi } from "vitest";
import { getSettingsLauncher } from "./settings-launcher.svelte";

describe("settings launcher", () => {
  it("runs the opener's callback once after the modal closes", () => {
    const launcher = getSettingsLauncher();
    const onClosed = vi.fn(() => {
      expect(launcher.isOpen).toBe(false);
    });

    launcher.open("chat", { chatSubsection: "teammates", chatCreateTeammate: true, onClosed });
    expect(launcher.isOpen).toBe(true);
    expect(onClosed).not.toHaveBeenCalled();

    launcher.close();
    expect(onClosed).toHaveBeenCalledTimes(1);
    expect(launcher.targetChatCreateTeammate).toBe(false);

    launcher.open("appearance");
    launcher.close();
    expect(onClosed).toHaveBeenCalledTimes(1);
  });

  it("forgets a callback when a later open replaces it", () => {
    const launcher = getSettingsLauncher();
    const first = vi.fn();
    const second = vi.fn();

    launcher.open("chat", { onClosed: first });
    launcher.open("chat", { onClosed: second });
    launcher.close();

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
