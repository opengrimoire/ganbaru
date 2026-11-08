<script lang="ts" module>
  /** Canvas bounds of the distraction blocker's welcome scenes, in logical pixels. */
  export const BLOCKER_SCENE_MIN_WIDTH = 300;
  export const BLOCKER_SCENE_MAX_WIDTH = 760;
  export const BLOCKER_SCENE_MIN_HEIGHT = 400;
  export const BLOCKER_SCENE_MAX_HEIGHT = 500;
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * The layout of the distraction blocker's welcome scenes: a header above a laptop's browser and a phone. A wide canvas
   * shows both devices side by side; a narrow one moves the camera to the device that acts, as `pan` goes from 0 (the
   * browser) to 1 (the phone).
   */
  let {
    width,
    height,
    headerHeight,
    pan,
    header,
    browser,
    phone,
  }: {
    width: number;
    height: number;
    headerHeight: number;
    pan: number;
    header: Snippet;
    browser: Snippet<[width: number]>;
    phone: Snippet<[width: number, height: number]>;
  } = $props();

  /** The gap below the header. */
  const HEADER_GAP = 12;
  /** From this canvas width, both devices fit side by side; below it the camera moves between them. */
  const WIDE_MIN_W = 520;
  const DEVICE_GAP = 16;
  /** The phone's width against its height, and its tallest size beside the browser. */
  const PHONE_ASPECT = 0.5;
  const PHONE_MAX_H = 380;

  const bodyH = $derived(height - headerHeight - HEADER_GAP);
  const wide = $derived(width >= WIDE_MIN_W);
  const phoneH = $derived(wide ? Math.min(bodyH, PHONE_MAX_H) : bodyH);
  const phoneW = $derived(Math.round(phoneH * PHONE_ASPECT));
  const browserW = $derived(wide ? width - phoneW - DEVICE_GAP : width);
  const phoneX = $derived(browserW + DEVICE_GAP);
  const cameraX = $derived(wide ? 0 : pan * (phoneX - (width - phoneW) / 2));
</script>

<div class="relative overflow-hidden" style:width={`${width}px`} style:height={`${height}px`}>
  <div class="absolute inset-x-0 top-0 flex items-center justify-center" style:height={`${headerHeight}px`}>
    {@render header()}
  </div>
  <div
    class="absolute left-0"
    style:top={`${headerHeight + HEADER_GAP}px`}
    style:height={`${bodyH}px`}
    style:width={`${phoneX + phoneW}px`}
    style:transform={`translateX(${-cameraX}px)`}
  >
    <div class="absolute top-0 left-0" style:width={`${browserW}px`} style:height={`${bodyH}px`}>{@render browser(browserW)}</div>
    <div class="absolute" style:left={`${phoneX}px`} style:top={`${(bodyH - phoneH) / 2}px`}>{@render phone(phoneW, phoneH)}</div>
  </div>
</div>
