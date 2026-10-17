<script lang="ts">
  import type { MemorySample } from "$lib/diagnostics/memory-samples";
  import { formatElapsed, pickTicks } from "$lib/diagnostics/memory-samples";

  let {
    samples,
    width = 256,
    height = 64,
    onHover,
  }: {
    samples: MemorySample[];
    width?: number;
    height?: number;
    /**
     * Lifts the currently hovered sample to the parent so the existing
     * per-process panel above the chart can mirror the hovered values
     * instead of duplicating them in a tooltip.
     */
    onHover?: (sample: MemorySample | null) => void;
  } = $props();

  const PAD_X = 2;
  const PAD_TOP = 4;
  const PAD_BOTTOM = 14;

  let hoverX = $state<number | null>(null);

  const innerWidth = $derived(width - PAD_X * 2);
  const innerHeight = $derived(height - PAD_TOP - PAD_BOTTOM);

  const tMin = $derived(samples.length > 0 ? samples[0].t : 0);
  const tMax = $derived(samples.length > 0 ? samples[samples.length - 1].t : 1);

  const yBounds = $derived.by(() => {
    if (samples.length === 0) return { min: 0, max: 1 };
    let min = Infinity;
    let max = 0;
    for (const s of samples) {
      if (s.totalMb < min) min = s.totalMb;
      if (s.totalMb > max) max = s.totalMb;
    }
    // Pad the visible range so the line never sits exactly on the top or
    // bottom edge. Floor at 0 to keep the chart anchored when memory is low.
    const span = max - min;
    const pad = Math.max(5, span * 0.15);
    return { min: Math.max(0, min - pad), max: max + pad };
  });

  function xOf(t: number): number {
    const span = Math.max(1, tMax - tMin);
    return PAD_X + ((t - tMin) / span) * innerWidth;
  }

  function yOf(mb: number): number {
    const span = Math.max(1, yBounds.max - yBounds.min);
    return PAD_TOP + innerHeight - ((mb - yBounds.min) / span) * innerHeight;
  }

  const path = $derived.by(() => {
    if (samples.length < 2) return "";
    const parts: string[] = [];
    for (let i = 0; i < samples.length; i++) {
      const s = samples[i];
      parts.push(`${i === 0 ? "M" : "L"} ${xOf(s.t).toFixed(1)} ${yOf(s.totalMb).toFixed(1)}`);
    }
    return parts.join(" ");
  });

  const xTicks = $derived.by(() => {
    if (samples.length < 2) return [];
    return pickTicks(tMin, tMax, innerWidth).map((t) => ({ t, label: formatElapsed(t) }));
  });

  const hoverSample = $derived.by(() => {
    if (hoverX === null || samples.length === 0) return null;
    const span = Math.max(1, tMax - tMin);
    const tHover = tMin + ((hoverX - PAD_X) / innerWidth) * span;
    let nearest = samples[0];
    let bestDiff = Math.abs(samples[0].t - tHover);
    for (let i = 1; i < samples.length; i++) {
      const diff = Math.abs(samples[i].t - tHover);
      if (diff < bestDiff) {
        bestDiff = diff;
        nearest = samples[i];
      }
    }
    return nearest;
  });

  /**
   * First and last x-axis ticks anchor to the chart edge so their labels
   * don't bleed past the SVG box (which clips by default). Middle ticks
   * stay centered on their tick mark.
   */
  function tickAnchor(index: number, total: number): "start" | "middle" | "end" {
    if (index === 0) return "start";
    if (index === total - 1) return "end";
    return "middle";
  }

  function onMove(e: PointerEvent) {
    const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
    hoverX = e.clientX - rect.left;
  }

  function onLeave() {
    hoverX = null;
  }

  $effect(() => {
    onHover?.(hoverSample);
  });

  // Time chip on the x-axis. Width is sized for the longest label
  // `formatElapsed` will produce in a one-hour buffer (`Xm Ys` or `1h Ym`).
  const CHIP_WIDTH = 56;
  const CHIP_HEIGHT = 12;
  const chipLeft = $derived.by(() => {
    if (!hoverSample) return 0;
    const target = xOf(hoverSample.t) - CHIP_WIDTH / 2;
    return Math.max(0, Math.min(width - CHIP_WIDTH, target));
  });
</script>

<div style="width: {width}px;">
  {#if samples.length < 2}
    <div
      class="flex w-full items-center justify-center text-[0.666667rem] text-muted-foreground"
      style="height: {height}px;"
    >
      Collecting samples...
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg
      {width}
      {height}
      onpointermove={onMove}
      onpointerleave={onLeave}
      class="block"
    >
      <path
        d={path}
        fill="none"
        stroke="currentColor"
        stroke-width="1.25"
        stroke-linejoin="round"
        class="text-foreground"
      />
      {#each xTicks as tick, i}
        <line
          x1={xOf(tick.t)}
          y1={PAD_TOP + innerHeight}
          x2={xOf(tick.t)}
          y2={PAD_TOP + innerHeight + 2}
          class="stroke-muted-foreground/40"
          stroke-width="1"
        />
        <text
          x={xOf(tick.t)}
          y={height - 2}
          text-anchor={tickAnchor(i, xTicks.length)}
          class="fill-muted-foreground"
          style="font-size: calc(0.6rem * var(--type-scale));"
        >{tick.label}</text>
      {/each}
      {#if hoverSample}
        <line
          x1={xOf(hoverSample.t)}
          y1={PAD_TOP}
          x2={xOf(hoverSample.t)}
          y2={PAD_TOP + innerHeight}
          class="stroke-muted-foreground/60"
          stroke-width="1"
          stroke-dasharray="2 2"
        />
        <circle
          cx={xOf(hoverSample.t)}
          cy={yOf(hoverSample.totalMb)}
          r="2.5"
          class="fill-foreground"
        />
        <!--
          Time chip pinned to the x-axis. Drawn after the regular ticks so
          its filled background covers the closest one cleanly without
          fading the others, keeping the time scale readable on either side.
        -->
        <rect
          x={chipLeft}
          y={height - CHIP_HEIGHT - 1}
          width={CHIP_WIDTH}
          height={CHIP_HEIGHT}
          rx="2"
          class="fill-foreground"
        />
        <text
          x={chipLeft + CHIP_WIDTH / 2}
          y={height - 3}
          text-anchor="middle"
          class="fill-background"
          style="font-size: calc(0.6rem * var(--type-scale));"
        >{formatElapsed(hoverSample.t)}</text>
      {/if}
    </svg>
  {/if}
</div>
