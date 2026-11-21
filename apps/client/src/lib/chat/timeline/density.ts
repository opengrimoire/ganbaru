import { CHAT_TIMELINE_DENSITIES, type ChatTimelineDensity } from "$lib/chat/contracts";
import type { Translate } from "$lib/i18n/translator.svelte";

/** Settings choices for the Chat timeline density, in display order. */
export function timelineDensityOptions(t: Translate): Array<{ value: ChatTimelineDensity; label: string }> {
  return [
    { value: "comfortable", label: t("settings.chat.behavior.densityComfortable") },
    { value: "compact", label: t("settings.chat.behavior.densityCompact") },
  ];
}

/** Reads a selected density value, falling back to the default for anything unknown. */
export function readTimelineDensity(value: string): ChatTimelineDensity {
  return CHAT_TIMELINE_DENSITIES.find((density) => density === value) ?? "comfortable";
}
