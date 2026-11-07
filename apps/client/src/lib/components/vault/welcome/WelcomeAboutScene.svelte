<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width the room chooses between these bounds, and a fixed height. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 480;
  const CANVAS_H = 380;

  export const ABOUT_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const ABOUT_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = CANVAS_H;

  /** The Open Grimoire logo, the repository owner's avatar. */
  const LOGO_URL = new URL("../../../../../static/brand/opengrimoire.svg", import.meta.url).href;
</script>

<script lang="ts">
  import BookOpen from "@lucide/svelte/icons/book-open";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import GitFork from "@lucide/svelte/icons/git-fork";
  import Scale from "@lucide/svelte/icons/scale";
  import Star from "@lucide/svelte/icons/star";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    ABOUT_FILES,
    ABOUT_REPOSITORY,
    aboutPage,
    aboutPop,
    aboutPress,
    aboutRow,
    aboutSparks,
    aboutStarred,
    aboutTouch,
  } from "$lib/vault/welcome-about-scene";

  /**
   * One frame of the welcome scene about the app itself: the app's public source repository opens in a browser, with its
   * files and its license in view, and a tap stars it. The page is a plain sketch of a repository page in the theme's
   * colors, with placeholder bars where commit messages and dates change over time. Every style is a function of
   * `elapsed`, with no CSS transitions, so text is never handed to a compositing layer mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const { t } = getLocalization();

  /** The window's frame, matching the other welcome scenes. */
  const BEZEL = 6;
  /** How far the page rises as it appears, and how far each row rises as it comes in. */
  const PAGE_RISE = 10;
  const ROW_RISE = 4;
  /** How far the Star button sinks as it is pressed. */
  const PRESS_SCALE = 0.06;
  /** How much the star grows at the peak of its pop. */
  const POP_GROWTH = 0.5;
  /** A star's gold, the same in every theme. */
  const STAR_GOLD = "#e3b341";
  /** The sparks that burst from the star: how many, how far they fly, and their size. */
  const SPARK_COUNT = 6;
  const SPARK_FROM = 5;
  const SPARK_TRAVEL = 11;
  const SPARK_SIZE = 3;
  const SPARK_ANGLES = Array.from({ length: SPARK_COUNT }, (_, index) => (index / SPARK_COUNT) * 2 * Math.PI);
  /** The widths of the placeholder commit messages, one per row, as fractions of the room they have. */
  const MESSAGE_WIDTHS = [0.82, 0.64, 0.9, 0.56, 0.74, 0.48, 0.68];

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const page = $derived(aboutPage(elapsed));
  const touch = $derived(aboutTouch(elapsed));
  const press = $derived(aboutPress(elapsed));
  const starred = $derived(aboutStarred(elapsed));
  const sparks = $derived(aboutSparks(elapsed));
  /** Sparks fly out fast and slow down as they fade. */
  const sparkDistance = $derived(SPARK_FROM + SPARK_TRAVEL * (1 - (1 - sparks) ** 3));
  const starScale = $derived(1 + POP_GROWTH * Math.sin(Math.PI * aboutPop(elapsed)));
</script>

{#snippet bar(widthClass: string, fraction?: number)}
  <span
    class={`block h-1.5 rounded-full bg-foreground/12 ${widthClass}`}
    style:width={fraction === undefined ? undefined : `${fraction * 100}%`}
  ></span>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5" style:width={`${width}px`} style:height={`${height}px`}>
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-background text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    <!-- The browser's bar, with the repository's address. -->
    <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
      <span class="flex gap-1" aria-hidden="true">
        <span class="size-2 rounded-full bg-foreground/15"></span>
        <span class="size-2 rounded-full bg-foreground/15"></span>
        <span class="size-2 rounded-full bg-foreground/15"></span>
      </span>
      <span class="min-w-0 flex-1 truncate rounded-full bg-foreground/6 px-2.5 py-0.5 text-center text-[10px] text-muted-foreground">
        {ABOUT_REPOSITORY.host}/{ABOUT_REPOSITORY.owner}/{ABOUT_REPOSITORY.name}
      </span>
    </div>

    {#if page > 0}
      <div
        class="flex min-h-0 flex-1 flex-col gap-2.5 px-3.5 py-3"
        style:opacity={page}
        style:transform={`translateY(${(1 - page) * PAGE_RISE}px)`}
      >
        <!-- The repository's name and its actions. -->
        <div class="flex min-w-0 items-center gap-2">
          <span class="flex size-6 shrink-0 items-center justify-center rounded-md bg-white p-0.5">
            <img src={LOGO_URL} alt="" class="size-full" />
          </span>
          <span class="min-w-0 truncate text-[13px]">
            <span class="text-muted-foreground">{ABOUT_REPOSITORY.owner} /</span>
            <span class="font-semibold">{ABOUT_REPOSITORY.name}</span>
          </span>
          <span class="shrink-0 rounded-full border border-border px-1.5 py-px text-[9.5px] text-muted-foreground">
            {t("vaultSetup.about.repository.public")}
          </span>
        </div>
        <div class="flex items-center gap-2 text-[11px] font-medium">
          <span class="flex h-7 items-center gap-1.5 rounded-md border border-border bg-foreground/4 px-2.5">
            <GitFork size={13} strokeWidth={1.8} aria-hidden="true" />
            {t("vaultSetup.about.repository.fork")}
          </span>
          <span
            class="flex h-7 items-center gap-1.5 rounded-md border border-border bg-foreground/4 px-2.5"
            style:transform={`scale(${1 - PRESS_SCALE * press})`}
          >
            <span class="relative flex size-3.25 items-center justify-center">
              <span class="flex" style:transform={`scale(${starScale})`}>
                <Star
                  size={13}
                  strokeWidth={1.8}
                  color={starred ? STAR_GOLD : "currentColor"}
                  fill={starred ? STAR_GOLD : "none"}
                  aria-hidden="true"
                />
              </span>
              {#if sparks > 0 && sparks < 1}
                {#each SPARK_ANGLES as angle, index (index)}
                  <span
                    class="absolute rounded-full"
                    style:width={`${SPARK_SIZE}px`}
                    style:height={`${SPARK_SIZE}px`}
                    style:background-color={STAR_GOLD}
                    style:opacity={1 - sparks}
                    style:transform={`translate(${Math.cos(angle) * sparkDistance}px, ${Math.sin(angle) * sparkDistance}px)`}
                  ></span>
                {/each}
              {/if}
              {#if touch > 0}
                <span
                  class="pointer-events-none absolute size-6 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
                  style:opacity={touch}
                ></span>
              {/if}
            </span>
            {starred ? t("vaultSetup.about.repository.starred") : t("vaultSetup.about.repository.star")}
          </span>
        </div>

        <!-- The file list, with placeholder bars for the latest commit and each entry's last change. -->
        <div class="overflow-hidden rounded-lg border border-border">
          <div class="flex h-7 items-center gap-2 border-b border-border bg-foreground/4 px-2.5">
            <span class="size-3.5 shrink-0 rounded-full bg-foreground/15"></span>
            {@render bar("w-2/5")}
            <span class="flex-1"></span>
            {@render bar("w-10")}
          </div>
          {#each ABOUT_FILES as file, index (file.name)}
            {@const row = aboutRow(elapsed, index)}
            <div
              class="flex h-6.5 items-center gap-2 border-b border-border/60 px-2.5 last:border-b-0"
              style:opacity={row}
              style:transform={`translateY(${(1 - row) * ROW_RISE}px)`}
            >
              {#if file.folder}
                <Folder size={13} strokeWidth={1.8} class="shrink-0 text-primary" fill="currentColor" fill-opacity="0.3" aria-hidden="true" />
              {:else}
                <File size={13} strokeWidth={1.8} class="shrink-0 text-muted-foreground" aria-hidden="true" />
              {/if}
              <span class="w-18 shrink-0 truncate text-[11px]">{file.name}</span>
              <span class="min-w-0 flex-1">{@render bar("", MESSAGE_WIDTHS[index % MESSAGE_WIDTHS.length])}</span>
              {@render bar("w-8 shrink-0")}
            </div>
          {/each}
        </div>

        <!-- The repository's readme and license. -->
        <div class="flex items-center gap-4 text-[11px] text-muted-foreground">
          <span class="flex items-center gap-1.5">
            <BookOpen size={13} strokeWidth={1.8} aria-hidden="true" />
            {t("vaultSetup.about.repository.readme")}
          </span>
          <span class="flex items-center gap-1.5">
            <Scale size={13} strokeWidth={1.8} aria-hidden="true" />
            {t("vaultSetup.about.repository.license")}
          </span>
        </div>
      </div>
    {/if}
  </div>
</div>
