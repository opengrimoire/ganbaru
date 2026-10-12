<script lang="ts">
  import { getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import type { Theme } from "$lib/themes";

  let {
    color,
    theme,
    size = 14,
    class: className = "",
  }: {
    color: EventColor;
    theme: Theme;
    size?: number;
    class?: string;
  } = $props();

  const fill = $derived(getEventColor(color, theme).bg);
</script>

<svg
  class={className}
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  aria-hidden="true"
>
  <rect
    x="2.55"
    y="0.75"
    width="1.9"
    height="14.5"
    rx="0.95"
    fill={fill}
  />
  <rect
    x="3.7"
    y="0.75"
    width="9.95"
    height="9.6"
    fill={fill}
  />
</svg>
