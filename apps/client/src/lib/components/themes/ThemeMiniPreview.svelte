<script lang="ts">
  import {
    resolveAppTokens,
    resolveCalendarTokens,
    type Theme,
  } from "$lib/themes";

  let { theme }: { theme: Theme } = $props();

  const PREVIEW_INDICES: readonly number[] = [2, 5, 8, 11, 14, 17, 20, 23, 27, 31];

  const appTokens = $derived(resolveAppTokens(theme));
  const calendarTokens = $derived(resolveCalendarTokens(theme));
  const previewGradient = $derived(
    `linear-gradient(to right, ${PREVIEW_INDICES.map(
      (i) => theme.eventPalette[i],
    ).join(", ")})`,
  );
</script>

<div
  class="flex shrink-0 flex-col gap-0.5 rounded-md p-1 outline outline-1 -outline-offset-1 outline-black/10 dark:outline-white/10"
  style="background: {appTokens['--background']}; width: 96px;"
  aria-hidden="true"
>
  <div
    class="flex h-3 items-center gap-1 rounded-sm px-1"
    style="background: {appTokens['--card']};"
  >
    <span
      class="flex-1 truncate text-[0.533333rem] font-bold leading-none"
      style="color: {appTokens['--foreground']};"
    >
      Aa
    </span>
    <span
      class="h-1.5 w-1.5 shrink-0 rounded-full"
      style="background: {appTokens['--primary']};"
    ></span>
  </div>
  <div
    class="h-2 w-full rounded-sm"
    style="background-color: {calendarTokens['--cal-bg']}; background-image: {previewGradient};"
  ></div>
</div>
