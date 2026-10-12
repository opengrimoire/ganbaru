import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { flushConfig } from "$lib/vault/config";
import { publishMobileDistractionsConfig } from "./mobile-distractions";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/vault/config", () => ({ flushConfig: vi.fn() }));
vi.mock("$lib/i18n/translator.svelte", () => ({ translate: (key: string) => key }));

describe("Android native policy publication boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("__GANBARU_AI_BUILD_PLATFORM__", "android");
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("sends only localized copy after persisted configuration succeeds", async () => {
    let finish!: () => void;
    vi.mocked(flushConfig).mockReturnValueOnce(new Promise<void>((resolve) => { finish = resolve; }));
    const publication = publishMobileDistractionsConfig();
    expect(invoke).not.toHaveBeenCalled();
    finish();
    await publication;
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_mobile_update_copy", {
      copy: {
        channelName: "settings.distractions.mobile.channelName",
        channelDescription: "settings.distractions.mobile.channelDescription",
        blockedMessage: "settings.distractions.mobile.blockedMessage",
        limitMessage: "settings.distractions.mobile.limitMessage",
      },
    });
  });

  it("propagates persistence failure without publishing a frontend rule snapshot", async () => {
    vi.mocked(flushConfig).mockRejectedValueOnce(new Error("configuration write failed"));
    await expect(publishMobileDistractionsConfig()).rejects.toThrow("configuration write failed");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not invoke the Android policy owner on another platform", async () => {
    vi.stubGlobal("__GANBARU_AI_BUILD_PLATFORM__", "linux");
    await publishMobileDistractionsConfig();
    expect(flushConfig).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });
});
