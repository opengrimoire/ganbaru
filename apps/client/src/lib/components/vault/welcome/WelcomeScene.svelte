<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { cn } from "$lib/utils";
  import { SCENE_RESTART_FADE_MS, SCENE_RESTART_PAUSE_MS } from "$lib/utils/scene-loop";

  /** How long the last frame stays before the scene starts over, unless the scene sets its own hold. */
  const END_HOLD_MS = 2200;

  /**
   * Plays a welcome scene, zoomed to the room it is given: it runs to its end, rests on its last frame, then fades out
   * and starts over on its own, as the Android access guides do, unless it is told to play once and stay on its last
   * frame, or it is a still drawing with no duration. With reduced motion it shows one frame as a still, the last unless it is given
   * another. The drawing keeps its width and takes the height the room suggests within its bounds, so a tall
   * room such as a phone's gets a taller drawing instead of empty space. When the room is too small for the drawing to
   * stay legible, it steps aside and leaves the space to the text. The drawing is sized with CSS zoom rather than a
   * transform, so its text is laid out and rasterized at the final size instead of being scaled.
   */
  let {
    minWidth,
    maxWidth = minWidth,
    minHeight,
    maxHeight = minHeight,
    duration = 0,
    loop = true,
    hold = END_HOLD_MS,
    still,
    ownZoom = false,
    description,
    frame,
    class: className = "",
  }: {
    /** The bounds of the drawing's width and height, in logical pixels. */
    minWidth: number;
    maxWidth?: number;
    minHeight: number;
    maxHeight?: number;
    /** How long one run of the scene lasts, in milliseconds; omitted for a still drawing that never plays. */
    duration?: number;
    /** Whether the scene starts over once it ends, or stays on its last frame. */
    loop?: boolean;
    /** How long the last frame rests before the scene starts over, in milliseconds. */
    hold?: number;
    /** The moment shown as a still with reduced motion, in milliseconds from the start; the last frame by default. */
    still?: number;
    /**
     * Whether the drawing zooms against its own smallest size up to a larger cap, instead of the shared scale of the
     * scenes drawn as the app's window; for drawings that are not the app, such as a repository page or a folder.
     */
    ownZoom?: boolean;
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
  const MAX_ZOOM = 1.2;
  /** The cap for a drawing with its own zoom, which has no app scale to match. */
  const OWN_MAX_ZOOM = 1.45;
  /**
   * The height every scene is zoomed against, the smallest canvas of the scenes drawn as the app's window, bezel
   * included. A scene whose own smallest canvas is shorter takes this zoom too and grows taller to fill the room, so
   * every screen draws the app at the same scale.
   */
  const ZOOM_REFERENCE_HEIGHT = 482;
  const START_DELAY_MS = 350;
  /** A longer gap between frames means the page was hidden, so the scene resumes instead of skipping ahead. */
  const MAX_FRAME_MS = 64;

  let roomWidth = $state(0);
  let roomHeight = $state(0);
  let elapsed = $state(0);
  let cycle = $state(0);
  let restarting = $state(false);

  /** The room left for the drawing once its vertical breathing room is set aside. */
  const roomY = $derived(Math.max(0, roomHeight - SCENE_MARGIN_Y));
  const maxZoom = $derived(ownZoom ? OWN_MAX_ZOOM : MAX_ZOOM);
  const zoomHeight = $derived(ownZoom ? minHeight : Math.max(minHeight, ZOOM_REFERENCE_HEIGHT));
  /**
   * The largest zoom at which the drawing's smallest size, or the shared reference height when that is taller, still
   * fits, so the drawing grows before it zooms.
   */
  const baseZoom = $derived(Math.min(maxZoom, roomWidth / minWidth, roomY / zoomHeight));
  const sceneWidth = $derived(baseZoom > 0 ? Math.floor(clamp(roomWidth / baseZoom, minWidth, maxWidth)) : minWidth);
  const sceneHeight = $derived(baseZoom > 0 ? Math.floor(clamp(roomY / baseZoom, minHeight, maxHeight)) : minHeight);
  const fit = $derived(Math.min(roomWidth / sceneWidth, roomY / sceneHeight));
  const shown = $derived(fit >= MIN_ZOOM);
  const zoom = $derived(Math.min(maxZoom, fit));

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
      if (loop) timer = window.setTimeout(restart, hold);
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
    if (duration <= 0) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) elapsed = Math.min(duration, still ?? duration);
    else play(START_DELAY_MS);
    return stop;
  });
</script>

<figure class={cn("relative isolate min-h-0", className)}>
  <!--
    The drawing sits out of the flow so its zoom never changes the room it is measured against. It ignores the real
    pointer, so only the scene's own pointer hovers what it draws. The figure isolates its stacking, so the scene's own
    layers, such as its pointer, stay under the screen's dialogs.
  -->
  <div class="pointer-events-none absolute inset-0" bind:clientWidth={roomWidth} bind:clientHeight={roomHeight} aria-hidden="true">
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
