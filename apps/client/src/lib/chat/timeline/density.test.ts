import { describe, expect, it } from "vitest";
import { CHAT_TIMELINE_DENSITIES } from "$lib/chat/contracts";
import type { Translate } from "$lib/i18n/translator.svelte";
import { readTimelineDensity, timelineDensityOptions } from "./density";

const t = ((key: string) => key) as Translate;

describe("timeline density", () => {
  it("offers every density once with a localized label", () => {
    const options = timelineDensityOptions(t);
    expect(options.map((option) => option.value)).toEqual([...CHAT_TIMELINE_DENSITIES]);
    expect(options.map((option) => option.label)).toEqual([
      "settings.chat.behavior.densityComfortable",
      "settings.chat.behavior.densityCompact",
    ]);
  });

  it("reads known values and falls back to comfortable for anything else", () => {
    expect(readTimelineDensity("compact")).toBe("compact");
    expect(readTimelineDensity("comfortable")).toBe("comfortable");
    expect(readTimelineDensity("Compact")).toBe("comfortable");
    expect(readTimelineDensity("")).toBe("comfortable");
  });
});
