<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { SCENE_RESTART_FADE_MS, SCENE_RESTART_PAUSE_MS } from "$lib/utils/scene-loop";

  /**
   * Plays a welcome scene on a loop, zoomed to the room it is given: it runs to its end, rests on its last frame, then
   * fades out and starts over on its own, as the Android access guides do. With reduced motion it shows the last
   * frame as a still. When the room is too small for the drawing to stay legible, it steps aside and leaves the space
   * to the text. The drawing is sized with CSS zoom rather than a transform, so its text is laid out and rasterized at
   * the final size instead of being scaled.
   */
  let {
    width: sceneWidth,
    height: sceneHeight,
    duration,
    description,
    frame,
    class: className = "",
  }: {
    /** The drawing's size in logical pixels. */
    width: number;
    height: number;
    /** How long one run of the scene lasts, in milliseconds. */
    duration: number;
    /** What the scene shows, for screen readers. */
    description: string;
    /** Draws the scene at a moment, given in milliseconds from its start. */
    frame: Snippet<[number]>;
    class?: string;
  } = $props();

  /** Breathing room around the drawing. */
  const SCENE_MARGIN = 16;
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

  const fit = $derived(
    roomWidth > 0 && roomHeight > 0
      ? Math.min((roomWidth - SCENE_MARGIN) / sceneWidth, (roomHeight - SCENE_MARGIN) / sceneHeight)
      : 0,
  );
  const shown = $derived(fit >= MIN_ZOOM);
  const zoom = $derived(Math.min(MAX_ZOOM, fit));

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
              {@render frame(elapsed)}
            {/key}
          </div>
        </div>
      </div>
    {/if}
  </div>
  <figcaption class="sr-only">{description}</figcaption>
</figure>
