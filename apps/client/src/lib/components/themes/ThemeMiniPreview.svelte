<script lang="ts">
  import {
    resolveAppTokens,
    resolveCalendarTokens,
    type Theme,
  } from "$lib/themes";
  import { cn } from "$lib/utils";

  /** `default` fits compact desktop rows; `touch` matches the touch control height beside it. */
  type ThemeMiniPreviewSize = "default" | "touch";

  let { theme, size = "default" }: { theme: Theme; size?: ThemeMiniPreviewSize } = $props();

  const PREVIEW_INDICES: readonly number[] = [2, 5, 8, 11, 14, 17, 20, 23, 27, 31];

  const appTokens = $derived(resolveAppTokens(theme));
  const calendarTokens = $derived(resolveCalendarTokens(theme));
  const previewGradient = $derived(
    `linear-gradient(to right, ${PREVIEW_INDICES.map(
      (i) => theme.eventPalette[i],
    ).join(", ")})`,
  );
  const touch = $derived(size === "touch");
</script>

<div
  class={cn(
    "flex shrink-0 flex-col rounded-md outline outline-1 -outline-offset-1 outline-black/10 dark:outline-white/10",
    touch ? "w-35 gap-1 p-1.5" : "w-24 gap-0.5 p-1",
  )}
  style:background={appTokens["--background"]}
  aria-hidden="true"
>
  <div
    class={cn("flex items-center gap-1 rounded-sm", touch ? "h-4.5 px-1.5" : "h-3 px-1")}
    style:background={appTokens["--card"]}
  >
    <span
      class={cn("flex-1 truncate font-bold leading-none", touch ? "text-[0.666667rem]" : "text-[0.533333rem]")}
      style:color={appTokens["--foreground"]}
    >
      Aa
    </span>
    <span
      class={cn("shrink-0 rounded-full", touch ? "size-2" : "size-1.5")}
      style:background={appTokens["--primary"]}
    ></span>
  </div>
  <div
    class={cn("w-full rounded-sm", touch ? "h-2.5" : "h-2")}
    style:background-color={calendarTokens["--cal-bg"]}
    style:background-image={previewGradient}
  ></div>
</div>
