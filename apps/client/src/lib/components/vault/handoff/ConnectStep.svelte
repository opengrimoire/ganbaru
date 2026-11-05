<script lang="ts" module>
  /**
   * Classes for a block in the last step that centers across the whole list instead of the step's
   * text column, such as a QR code or a camera view. The negative start margin spans the marker
   * column (`w-6`) and its gap (`gap-3`).
   */
  export const stepCenteredBlockClass = "-ms-9 flex justify-center";

  /** Indent that lines a block after the list up with the steps' text column, past the marker column and its gap. */
  export const stepTextIndentClass = "ms-9";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * One numbered step of a connect flow, for an `<ol>` of steps. The marker column spans the whole
   * row, so a line runs from one circle to the next. Next to a field, the circle drops to the
   * field's center.
   */
  let {
    number,
    total,
    beside = "text",
    children,
  }: {
    number: number;
    total: number;
    /** What starts the step's content, which sets where the circle sits. */
    beside?: "text" | "field";
    children: Snippet;
  } = $props();

  const localization = getLocalization();
</script>

<li class="flex gap-3">
  <span aria-hidden="true" class="flex w-6 shrink-0 flex-col items-center">
    <span class={cn("w-px shrink-0", beside === "field" ? "h-1" : "h-0", number > 1 && "bg-primary/25")}></span>
    <span class="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/12 text-xs font-semibold text-primary tabular-nums">
      {formatNumber(localization.locale, number)}
    </span>
    <span class={cn("w-px flex-1", number < total && "bg-primary/25")}></span>
  </span>
  {@render children()}
</li>
