import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { publishMobileDistractionsConfig } from "./mobile-distractions";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/i18n/translator.svelte", () => ({ translate: (key: string) => key }));

describe("Android native policy publication boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "android");
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("sends only localized copy, never a frontend rule snapshot", async () => {
    await publishMobileDistractionsConfig();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_mobile_update_copy", {
      copy: {
        channelName: "settings.distractions.mobile.channelName",
        channelDescription: "settings.distractions.mobile.channelDescription",
        blockedMessage: "settings.distractions.mobile.blockedMessage",
        limitMessage: "settings.distractions.mobile.limitMessage",
      },
    });
  });

  it("propagates a failed copy update", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("copy update failed"));
    await expect(publishMobileDistractionsConfig()).rejects.toThrow("copy update failed");
  });

  it("does not invoke the Android policy owner on another platform", async () => {
    vi.stubGlobal("__GANBARU_BUILD_PLATFORM__", "linux");
    await publishMobileDistractionsConfig();
    expect(invoke).not.toHaveBeenCalled();
  });
});
