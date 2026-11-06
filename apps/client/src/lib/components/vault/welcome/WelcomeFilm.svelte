<script lang="ts">
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { FILM_BEATS, filmAct, type FilmAct } from "$lib/vault/welcome-film";
  import WelcomeFilmScene, { FILM_SCENE_HEIGHT, FILM_SCENE_WIDTH } from "./WelcomeFilmScene.svelte";

  /**
   * Plays the welcome film once, zoomed to the room it is given, with its chapters underneath and a replay at the
   * end. With reduced motion it shows the last frame as a still. When the room is too small for the canvas to stay
   * legible, the film steps aside and leaves the space to the text and actions. The canvas is sized with CSS zoom
   * rather than a transform, so its text is laid out and rasterized at the final size instead of being scaled.
   */
  let { class: className = "" }: { class?: string } = $props();

  const { t } = getLocalization();

  const SCENE_W = FILM_SCENE_WIDTH;
  const SCENE_H = FILM_SCENE_HEIGHT;
  /** Breathing room around the canvas for its shadow. */
  const SCENE_MARGIN = 16;
  /** Below this scale the drawing stops reading as an app; above it, it starts to look like a poster. */
  const MIN_ZOOM = 0.55;
  const MAX_ZOOM = 1.45;
  const START_DELAY_MS = 350;
  const RESTART_FADE_MS = 300;
  /** A longer gap between frames means the page was hidden, so the film resumes instead of skipping ahead. */
  const MAX_FRAME_MS = 64;
  const ACTS: readonly FilmAct[] = ["plan", "focus", "progress"];

  let width = $state(0);
  let height = $state(0);
  let elapsed = $state(0);
  let cycle = $state(0);
  let fading = $state(false);
  let reducedMotion = $state(false);

  const fit = $derived(width > 0 && height > 0 ? Math.min((width - SCENE_MARGIN) / SCENE_W, (height - SCENE_MARGIN) / SCENE_H) : 0);
  const shown = $derived(fit >= MIN_ZOOM);
  const zoom = $derived(Math.min(MAX_ZOOM, fit));
  const ended = $derived(elapsed >= FILM_BEATS.end);
  const current = $derived(filmAct(elapsed));

  let frame = 0;
  let timer = 0;
  let last = 0;

  function actLabel(act: FilmAct): string {
    switch (act) {
      case "plan": return t("vaultSetup.film.actPlan");
      case "focus": return t("vaultSetup.film.actFocus");
      case "progress": return t("vaultSetup.film.actProgress");
    }
  }

  /** How full an act's bar is: finished acts are full, the playing one fills as it runs, later ones are empty. */
  function actFill(act: FilmAct): number {
    if (reducedMotion) return 1;
    const playing = ACTS.indexOf(current.act);
    const index = ACTS.indexOf(act);
    if (index < playing) return 1;
    return index === playing ? current.progress : 0;
  }

  function tick(now: number): void {
    elapsed = Math.min(FILM_BEATS.end, elapsed + Math.min(now - last, MAX_FRAME_MS));
    last = now;
    frame = elapsed < FILM_BEATS.end ? requestAnimationFrame(tick) : 0;
  }

  function stop(): void {
    cancelAnimationFrame(frame);
    window.clearTimeout(timer);
    frame = 0;
  }

  function play(delay: number): void {
    stop();
    timer = window.setTimeout(() => {
      last = performance.now();
      frame = requestAnimationFrame(tick);
    }, delay);
  }

  function replay(): void {
    if (fading) return;
    stop();
    fading = true;
    timer = window.setTimeout(() => {
      cycle += 1;
      elapsed = 0;
      fading = false;
      play(RESTART_FADE_MS);
    }, RESTART_FADE_MS);
  }

  onMount(() => {
    reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) elapsed = FILM_BEATS.end;
    else play(START_DELAY_MS);
    return stop;
  });
</script>

<figure class={cn("flex min-h-0 flex-col", className)}>
  <!-- The canvas sits out of the flow so its zoom never changes the room it is measured against. -->
  <div class="relative min-h-0 flex-1" bind:clientWidth={width} bind:clientHeight={height} aria-hidden="true">
    {#if shown}
      <div class="absolute inset-0 flex items-center justify-center">
        <div style:width={`${SCENE_W * zoom}px`} style:height={`${SCENE_H * zoom}px`}>
          <div
            class={cn(
              "transition-opacity ease-in-out motion-reduce:transition-none",
              fading ? "opacity-0" : "opacity-100",
            )}
            style:zoom={zoom}
            style:transition-duration={`${RESTART_FADE_MS}ms`}
          >
            {#key cycle}
              <WelcomeFilmScene {elapsed} />
            {/key}
          </div>
        </div>
      </div>
    {/if}
  </div>

  {#if shown}
    <div class="flex shrink-0 items-center justify-center gap-5 pt-2">
      {#each ACTS as act (act)}
        {@const fill = actFill(act)}
        <span
          aria-hidden="true"
          class={cn(
            "flex flex-col items-center gap-1.5 text-xs font-medium transition-colors duration-300",
            act === current.act || ended || reducedMotion ? "text-foreground" : "text-muted-foreground",
          )}
        >
          {actLabel(act)}
          <span class="h-0.5 w-full overflow-hidden rounded-full bg-foreground/10">
            <span class="block h-full origin-left rounded-full bg-primary" style:transform={`scaleX(${fill})`}></span>
          </span>
        </span>
      {/each}
      {#if !reducedMotion}
        <button
          type="button"
          class={cn(
            "-ml-1 flex size-8 items-center justify-center rounded-full text-muted-foreground transition-[opacity,color] duration-300 hover:bg-accent hover:text-foreground",
            ended ? "opacity-100" : "invisible opacity-0",
          )}
          aria-label={t("vaultSetup.film.replay")}
          title={t("vaultSetup.film.replay")}
          onclick={replay}
        >
          <RotateCcw size={15} strokeWidth={1.8} aria-hidden="true" />
        </button>
      {/if}
    </div>
  {/if}
  <figcaption class="sr-only">{t("vaultSetup.film.description")}</figcaption>
</figure>
