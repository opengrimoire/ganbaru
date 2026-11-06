<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { SCENE_RESTART_FADE_MS, SCENE_RESTART_PAUSE_MS } from "$lib/utils/scene-loop";

  /**
   * Plays a welcome scene on a loop, zoomed to the room it is given: it runs to its end, rests on its last frame, then
   * fades out and starts over on its own, as the Android access guides do. With reduced motion it shows the last
   * frame as a still. The drawing keeps its width and takes the height the room suggests within its bounds, so a tall
   * room such as a phone's gets a taller drawing instead of empty space. When the room is too small for the drawing to
   * stay legible, it steps aside and leaves the space to the text. The drawing is sized with CSS zoom rather than a
   * transform, so its text is laid out and rasterized at the final size instead of being scaled.
   */
  let {
    minWidth,
    maxWidth = minWidth,
    minHeight,
    maxHeight = minHeight,
    duration,
    description,
    frame,
    class: className = "",
  }: {
    /** The bounds of the drawing's width and height, in logical pixels. */
    minWidth: number;
    maxWidth?: number;
    minHeight: number;
    maxHeight?: number;
    /** How long one run of the scene lasts, in milliseconds. */
    duration: number;
    /** What the scene shows, for screen readers. */
    description: string;
    /** Draws the scene at a moment, given in milliseconds from its start, at a size within its bounds. */
    frame: Snippet<[elapsed: number, width: number, height: number]>;
    class?: string;
  } = $props();

  /** Breathing room above and below the drawing; the step's own padding already keeps it off the sides. */
  const SCENE_MARGIN_Y = 16;
  /** Below this scale the drawing stops reading as the app; above it, it starts to look like a poster. */
  const MIN_ZOOM = 0.55;
  const MAX_ZOOM = 1.45;
  const START_DELAY_MS = 350;
  /** How long the last frame stays before the scene starts over. */
  const END_HOLD_MS = 2200;
  /** A longer gap between frames means the page was hidden, so the scene resumes instead of skipping ahead. */
  const MAX_FRAME_MS = 64;

  let roomWidth = $state(0);
  let roomHeight = $state(0);
  let elapsed = $state(0);
  let cycle = $state(0);
  let restarting = $state(false);

  /** The room left for the drawing once its vertical breathing room is set aside. */
  const roomY = $derived(Math.max(0, roomHeight - SCENE_MARGIN_Y));
  /** The largest zoom at which the drawing's smallest size still fits, so the drawing grows before it zooms. */
  const baseZoom = $derived(Math.min(MAX_ZOOM, roomWidth / minWidth, roomY / minHeight));
  const sceneWidth = $derived(baseZoom > 0 ? Math.floor(clamp(roomWidth / baseZoom, minWidth, maxWidth)) : minWidth);
  const sceneHeight = $derived(baseZoom > 0 ? Math.floor(clamp(roomY / baseZoom, minHeight, maxHeight)) : minHeight);
  const fit = $derived(Math.min(roomWidth / sceneWidth, roomY / sceneHeight));
  const shown = $derived(fit >= MIN_ZOOM);
  const zoom = $derived(Math.min(MAX_ZOOM, fit));

  function clamp(value: number, low: number, high: number): number {
    return Math.min(high, Math.max(low, value));
  }

  let frameRequest = 0;
  let timer = 0;
  let last = 0;

  function tick(now: number): void {
    elapsed = Math.min(duration, elapsed + Math.min(now - last, MAX_FRAME_MS));
    last = now;
    if (elapsed < duration) frameRequest = requestAnimationFrame(tick);
    else {
      frameRequest = 0;
      timer = window.setTimeout(restart, END_HOLD_MS);
    }
  }

  function stop(): void {
    cancelAnimationFrame(frameRequest);
    window.clearTimeout(timer);
    frameRequest = 0;
  }

  function play(delay: number): void {
    stop();
    timer = window.setTimeout(() => {
      last = performance.now();
      frameRequest = requestAnimationFrame(tick);
    }, delay);
  }

  /** Fades the last frame out, starts the scene over while it is blank, and fades the first frame back in. */
  function restart(): void {
    restarting = true;
    timer = window.setTimeout(() => {
      elapsed = 0;
      cycle += 1;
      restarting = false;
      play(0);
    }, SCENE_RESTART_FADE_MS + SCENE_RESTART_PAUSE_MS);
  }

  onMount(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) elapsed = duration;
    else play(START_DELAY_MS);
    return stop;
  });
</script>

<figure class={cn("relative min-h-0", className)}>
  <!-- The drawing sits out of the flow so its zoom never changes the room it is measured against. -->
  <div class="absolute inset-0" bind:clientWidth={roomWidth} bind:clientHeight={roomHeight} aria-hidden="true">
    {#if shown}
      <div class="flex h-full items-center justify-center">
        <div style:width={`${sceneWidth * zoom}px`} style:height={`${sceneHeight * zoom}px`}>
          <div
            class={cn("transition-opacity ease-in-out motion-reduce:transition-none", restarting ? "opacity-0" : "opacity-100")}
            style:zoom={zoom}
            style:transition-duration={`${SCENE_RESTART_FADE_MS}ms`}
          >
            {#key cycle}
              {@render frame(elapsed, sceneWidth, sceneHeight)}
            {/key}
          </div>
        </div>
      </div>
    {/if}
  </div>
  <figcaption class="sr-only">{description}</figcaption>
</figure>
