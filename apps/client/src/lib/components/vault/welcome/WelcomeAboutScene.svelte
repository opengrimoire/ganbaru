<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 384;
  const MAX_CANVAS_W = 600;
  const MIN_CANVAS_H = 256;
  const MAX_CANVAS_H = 360;

  export const ABOUT_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const ABOUT_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = MAX_CANVAS_H;
</script>

<script lang="ts">
  import User from "@lucide/svelte/icons/user";
  import { getEventColor } from "$lib/calendar/utils";
  import type { EventColor } from "$lib/calendar/types";
  import { getTheme } from "$lib/stores/theme.svelte";
  import {
    ABOUT_EDIT_ORIGINS,
    ABOUT_INITIAL_ROWS,
    ABOUT_LINKS,
    ABOUT_ROW_COUNT,
    aboutDevice,
    aboutLink,
    aboutRow,
    aboutRowGlow,
    aboutTravel,
  } from "$lib/vault/welcome-about-scene";

  /**
   * One frame of the welcome scene about the app itself: one person's laptop, desktop, and phone, holding the same
   * things and linked directly to each other, with no machine in the middle. Each device in turn adds something that
   * travels straight to the other two, so the data lives on the person's devices and moves only between them. Every
   * style is a function of `elapsed`, with no CSS transitions, so text is never handed to a compositing layer
   * mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const theme = getTheme();

  /** Breathing room at the top and bottom, which grows a little with the canvas. */
  const MIN_PAD = 10;
  const PAD_SHARE = 0.15;
  /** How far the laptop and the desktop sit from the center on the narrowest canvas, and how much more a wider one spreads them. */
  const MIN_SPREAD = 100;
  const SPREAD_SHARE = 0.3;
  /** How far the devices rise into place as they appear. */
  const RISE = 6;
  /** The space a link leaves between its end and a device. */
  const LINK_GAP = 6;
  const PACKET_R = 4;
  const ROW_H = 10;

  /** Palette slots, so the drawing follows the active theme's event colors. */
  const PINK = 1;
  const ORANGE = 7;
  const GREEN = 12;
  const BLUE = 18;
  const PURPLE = 22;

  /** The person's things, the same on every device, each a color and how much of the row its text fills. */
  const ROWS: readonly { color: EventColor; share: number }[] = [
    { color: BLUE, share: 0.72 },
    { color: GREEN, share: 0.5 },
    { color: ORANGE, share: 0.62 },
    { color: PINK, share: 0.45 },
    { color: PURPLE, share: 0.56 },
  ].slice(0, ABOUT_ROW_COUNT);

  type DeviceKind = "laptop" | "desktop" | "phone";
  /** The devices in the order the scene counts them: the laptop at the top left, the desktop at the top right, the phone below. */
  const DEVICES: readonly { kind: DeviceKind; width: number; height: number }[] = [
    { kind: "laptop", width: 120, height: 76 },
    { kind: "desktop", width: 132, height: 84 },
    { kind: "phone", width: 50, height: 92 },
  ];
  const LAPTOP_BASE = { width: 140, height: 5 };
  const DESKTOP_NECK = { width: 16, height: 10 };
  const DESKTOP_FOOT = { width: 52, height: 4 };

  const BEZEL = "color-mix(in srgb, var(--foreground) 8%, var(--background))";
  const STAND = "color-mix(in srgb, var(--foreground) 14%, var(--background))";

  interface Box {
    cx: number;
    cy: number;
    width: number;
    height: number;
  }

  const center = $derived(width / 2);
  const pad = $derived(MIN_PAD + (height - MIN_CANVAS_H) * PAD_SHARE);
  const spread = $derived(MIN_SPREAD + (width - MIN_CANVAS_W) * SPREAD_SHARE);

  /** Where each device's screen sits, before it rises into place. */
  const boxes = $derived<Box[]>(
    DEVICES.map((device, index) => {
      const cx = index === 0 ? center - spread : index === 1 ? center + spread : center;
      const top = index === 2 ? height - pad - device.height : pad;
      return { cx, cy: top + device.height / 2, width: device.width, height: device.height };
    }),
  );

  /** Where the line from `from` toward `to` leaves `from`'s screen, a small gap beyond its edge. */
  function edgePoint(from: Box, to: Box): { x: number; y: number } {
    const dx = to.cx - from.cx;
    const dy = to.cy - from.cy;
    const scale = Math.min(
      dx === 0 ? Infinity : (from.width / 2 + LINK_GAP) / Math.abs(dx),
      dy === 0 ? Infinity : (from.height / 2 + LINK_GAP) / Math.abs(dy),
    );
    return { x: from.cx + dx * scale, y: from.cy + dy * scale };
  }

  /** The visible ends of the link between devices `from` and `to`. */
  function linkEnds(from: number, to: number): { x1: number; y1: number; x2: number; y2: number } {
    const start = edgePoint(boxes[from], boxes[to]);
    const end = edgePoint(boxes[to], boxes[from]);
    return { x1: start.x, y1: start.y, x2: end.x, y2: end.y };
  }

  /** Where the thing from edit `edit` is on its way from its origin to device `target`. */
  function packetPoint(edit: number, target: number, progress: number): { x: number; y: number } {
    const path = linkEnds(ABOUT_EDIT_ORIGINS[edit], target);
    return { x: path.x1 + (path.x2 - path.x1) * progress, y: path.y1 + (path.y2 - path.y1) * progress };
  }

  function rowColor(row: number): string {
    return getEventColor(ROWS[row].color, theme.current).bg;
  }
</script>

{#snippet screen(index: number)}
  {@const phone = DEVICES[index].kind === "phone"}
  <div class="flex h-full flex-col" class:px-1.5={!phone} class:pt-1.5={!phone} class:px-1={phone} class:pt-2.5={phone}>
    {#each ROWS as _, row (row)}
      {@const shown = aboutRow(elapsed, index, row)}
      {@const glow = aboutRowGlow(elapsed, index, row)}
      <div
        class="relative flex shrink-0 items-center gap-1 rounded-sm px-0.5"
        style:height={`${ROW_H}px`}
        style:top={`${(1 - shown) * 3}px`}
        style:opacity={shown}
        style:background-color={glow > 0 ? `color-mix(in srgb, ${rowColor(row)} ${Math.round(glow * 22)}%, transparent)` : undefined}
      >
        <span class="size-1.5 shrink-0 rounded-[2px]" style:background-color={rowColor(row)}></span>
        <span class="h-0.75 rounded-full bg-foreground/20" style:width={`${ROWS[row].share * 100}%`}></span>
      </div>
    {/each}
  </div>
{/snippet}

<div class="relative" style:width={`${width}px`} style:height={`${height}px`}>
  <!-- The direct links between the devices, and the new things travelling along them. -->
  <svg class="absolute inset-0 overflow-visible" {width} {height} aria-hidden="true">
    {#each ABOUT_LINKS as [from, to], link (link)}
      {@const drawn = aboutLink(elapsed, link)}
      {#if drawn > 0}
        {@const path = linkEnds(from, to)}
        <line
          x1={path.x1}
          y1={path.y1}
          x2={path.x1 + (path.x2 - path.x1) * drawn}
          y2={path.y1 + (path.y2 - path.y1) * drawn}
          class="stroke-muted-foreground"
          stroke-width="1.5"
          stroke-dasharray="3 4"
          stroke-linecap="round"
          opacity="0.5"
        />
      {/if}
    {/each}
    {#each ABOUT_EDIT_ORIGINS as origin, edit (edit)}
      {@const progress = aboutTravel(elapsed, edit)}
      {#if progress > 0 && progress < 1}
        {@const color = rowColor(ABOUT_INITIAL_ROWS + edit)}
        {@const fade = Math.min(1, progress * 8, (1 - progress) * 6)}
        {#each DEVICES as _, target (target)}
          {#if target !== origin}
            {@const point = packetPoint(edit, target, progress)}
            <circle cx={point.x} cy={point.y} r={PACKET_R + 3} fill={color} opacity={0.25 * fade} />
            <circle cx={point.x} cy={point.y} r={PACKET_R} fill={color} opacity={fade} />
          {/if}
        {/each}
      {/if}
    {/each}
  </svg>

  <!-- One person's devices, each holding the same things. -->
  {#each DEVICES as device, index (index)}
    {@const shown = aboutDevice(elapsed, index)}
    {#if shown > 0}
      {@const box = boxes[index]}
      <div
        class="absolute"
        style:left={`${box.cx - device.width / 2}px`}
        style:top={`${box.cy - device.height / 2 + (1 - shown) * RISE}px`}
        style:opacity={shown}
      >
        <div
          class="relative p-1"
          class:rounded-lg={device.kind !== "phone"}
          class:rounded-xl={device.kind === "phone"}
          style:width={`${device.width}px`}
          style:height={`${device.height}px`}
          style:background-color={BEZEL}
        >
          <div class="h-full overflow-hidden bg-background" class:rounded-sm={device.kind !== "phone"} class:rounded-lg={device.kind === "phone"}>
            {@render screen(index)}
          </div>
          <span class="absolute -right-2 -bottom-2 z-10 grid size-4.5 place-items-center rounded-full bg-primary text-primary-foreground ring-2 ring-background">
            <User size={10} strokeWidth={2.4} aria-hidden="true" />
          </span>
        </div>
        {#if device.kind === "laptop"}
          <div
            class="relative rounded-b-md"
            style:left={`${(device.width - LAPTOP_BASE.width) / 2}px`}
            style:width={`${LAPTOP_BASE.width}px`}
            style:height={`${LAPTOP_BASE.height}px`}
            style:background-color={STAND}
          ></div>
        {:else if device.kind === "desktop"}
          <div
            class="relative"
            style:left={`${(device.width - DESKTOP_NECK.width) / 2}px`}
            style:width={`${DESKTOP_NECK.width}px`}
            style:height={`${DESKTOP_NECK.height}px`}
            style:background-color={BEZEL}
          ></div>
          <div
            class="relative rounded-t-sm"
            style:left={`${(device.width - DESKTOP_FOOT.width) / 2}px`}
            style:width={`${DESKTOP_FOOT.width}px`}
            style:height={`${DESKTOP_FOOT.height}px`}
            style:background-color={STAND}
          ></div>
        {/if}
      </div>
    {/if}
  {/each}
</div>
