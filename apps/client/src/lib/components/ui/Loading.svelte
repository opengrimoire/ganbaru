<script lang="ts">
  import { onMount } from "svelte";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * `fill` centers a quiet spinner in the area the content will occupy, `rows` sketches list or menu rows where the
   * content is a list, and `inline` shows a small spinner inside a line of text or a value slot.
   */
  type LoadingLayout = "fill" | "rows" | "inline";

  /** Loads that finish before this delay never show anything, so fast surfaces do not flash. */
  const REVEAL_DELAY_MS = 300;
  /** Skeleton row widths cycle through these fractions so the sketch does not read as a solid block. */
  const ROW_WIDTHS = ["62%", "48%", "71%", "39%", "56%"] as const;

  let {
    label,
    layout = "fill",
    rows = 3,
    delay = REVEAL_DELAY_MS,
    class: className,
  }: {
    /** What is loading, announced to assistive technology only; defaults to the generic loading text. */
    label?: string;
    layout?: LoadingLayout;
    /** Number of skeleton rows for the `rows` layout. */
    rows?: number;
    delay?: number;
    class?: string;
  } = $props();

  const { t } = getLocalization();
  let revealed = $state(false);

  onMount(() => {
    if (delay <= 0) {
      revealed = true;
      return;
    }
    const timer = setTimeout(() => { revealed = true; }, delay);
    return () => clearTimeout(timer);
  });
</script>

<!-- Inline loading renders a span so it stays valid inside phrasing content such as labels and headings. -->
<svelte:element
  this={layout === "inline" ? "span" : "div"}
  class={cn("loading", `loading-${layout}`, className)}
  role="status"
  aria-busy="true"
  data-loading={layout}
  data-revealed={revealed || undefined}
><span class="sr-only">{label ?? t("common.loading")}</span>{#if layout === "rows"}{#each { length: Math.max(1, rows) }, index (index)}<span class="loading-row" aria-hidden="true"><span class="loading-bar" style:width={ROW_WIDTHS[index % ROW_WIDTHS.length]}></span></span>{/each}{:else}<span class="loading-spinner" aria-hidden="true"><LoaderCircle strokeWidth={1.8} aria-hidden="true" /></span>{/if}</svelte:element>

<style>
  /* The components layer lets utility classes passed through `class` override color, padding, and size. */
  @layer components {
    .loading { color: var(--muted-foreground); opacity: 0; transition: opacity 160ms ease; }
    .loading[data-revealed] { opacity: 1; }
    .loading-fill { display: grid; width: 100%; height: 100%; min-height: inherit; place-items: center; padding: 1rem; }
    .loading-fill .loading-spinner :global(svg) { width: 1.25rem; height: 1.25rem; }
    .loading-inline { display: inline-flex; height: 1lh; vertical-align: top; align-items: center; }
    .loading-inline .loading-spinner :global(svg) { width: 1em; height: 1em; }
    .loading-rows { display: flex; width: 100%; flex-direction: column; }
    .loading-row { display: flex; min-height: var(--panel-row-height); align-items: center; padding: 0.25rem 0.5rem; }
    .loading-bar { height: 0.625em; border-radius: 999px; background: color-mix(in srgb, var(--muted-foreground) 12%, transparent); }
    .loading-spinner { display: inline-flex; animation: loading-spin 900ms linear infinite; }
    .loading[data-revealed] .loading-bar { animation: loading-pulse 1600ms ease-in-out infinite; }
    @media (prefers-reduced-motion: reduce) {
      .loading-spinner, .loading[data-revealed] .loading-bar { animation: none; }
    }
  }
  @keyframes loading-spin {
    to { transform: rotate(360deg); }
  }
  @keyframes loading-pulse {
    50% { opacity: 0.55; }
  }
</style>
