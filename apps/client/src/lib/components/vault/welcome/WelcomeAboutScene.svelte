<script lang="ts" module>
  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = 300;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 360;
  const MAX_CANVAS_H = 440;

  export const ABOUT_SCENE_MIN_WIDTH = MIN_CANVAS_W;
  export const ABOUT_SCENE_MAX_WIDTH = MAX_CANVAS_W;
  export const ABOUT_SCENE_MIN_HEIGHT = MIN_CANVAS_H;
  export const ABOUT_SCENE_MAX_HEIGHT = MAX_CANVAS_H;

  /** The Open Grimoire logo, the repository owner's avatar. */
  const LOGO_URL = new URL("../../../../../static/brand/opengrimoire.svg", import.meta.url).href;
</script>

<script lang="ts">
  import type { Component } from "svelte";
  import BookOpen from "@lucide/svelte/icons/book-open";
  import File from "@lucide/svelte/icons/file";
  import Folder from "@lucide/svelte/icons/folder";
  import GitFork from "@lucide/svelte/icons/git-fork";
  import HeartHandshake from "@lucide/svelte/icons/heart-handshake";
  import Scale from "@lucide/svelte/icons/scale";
  import Shield from "@lucide/svelte/icons/shield";
  import Star from "@lucide/svelte/icons/star";
  import Users from "@lucide/svelte/icons/users";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
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
   * files, its community documents, and its license in view, and a tap stars it. The page is a plain sketch of a
   * repository page in the theme's colors, with placeholder bars where the description, commit messages, and dates
   * change over time. A wide canvas lays the page out with a sidebar as a repository host does; a narrow one keeps a
   * single column. Every style is a function of `elapsed`, with no CSS transitions, so text is never handed to a
   * compositing layer mid-animation.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;

  /** The window's frame, matching the other welcome scenes. */
  const BEZEL = 6;
  /** From this canvas width, the page has room for its sidebar. */
  const SIDEBAR_MIN_W = 540;
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";
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
  /** The widths of the placeholder commit messages, one per row, and of the description's lines, as fractions of their room. */
  const MESSAGE_WIDTHS = [0.82, 0.64, 0.9, 0.56, 0.74, 0.48, 0.68];
  const DESCRIPTION_WIDTHS = [1, 0.7];

  interface RepositoryLink {
    icon: Component<{ size?: number; strokeWidth?: number; "aria-hidden"?: "true" }>;
    label: () => string;
  }

  const README: RepositoryLink = { icon: BookOpen, label: () => t("vaultSetup.about.repository.readme") };
  const LICENSE: RepositoryLink = { icon: Scale, label: () => t("vaultSetup.about.repository.license") };
  const CODE_OF_CONDUCT: RepositoryLink = { icon: HeartHandshake, label: () => t("vaultSetup.about.repository.codeOfConduct") };
  const CONTRIBUTING: RepositoryLink = { icon: Users, label: () => t("vaultSetup.about.repository.contributing") };
  const SECURITY: RepositoryLink = { icon: Shield, label: () => t("vaultSetup.about.repository.security") };
  const SECURITY_POLICY: RepositoryLink = { icon: Shield, label: () => t("vaultSetup.about.repository.securityPolicy") };

  /** One language of the repository's code, with its share of the code and the color repository hosts give it. */
  interface RepositoryLanguage {
    name: string;
    share: number;
    color: string;
  }

  /** The repository's languages as its host reports them, largest first; a snapshot, not a live figure. */
  const LANGUAGES: readonly RepositoryLanguage[] = [
    { name: "Rust", share: 0.546, color: "#dea584" },
    { name: "TypeScript", share: 0.288, color: "#3178c6" },
    { name: "Svelte", share: 0.149, color: "#ff3e00" },
    { name: "Kotlin", share: 0.009, color: "#a97bff" },
    { name: "JavaScript", share: 0.007, color: "#f1e05a" },
    { name: "CSS", share: 0.001, color: "#663399" },
  ];
  const LANGUAGE_SHARE: Intl.NumberFormatOptions = { style: "percent", minimumFractionDigits: 1, maximumFractionDigits: 1 };

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);
  const wide = $derived(canvasW >= SIDEBAR_MIN_W);
  /**
   * The repository's documents, the readme and the license first. The sidebar lists them with icons and names the
   * security policy in full; the single column shows them as tabs under the files, with the readme's selected, and
   * shows the tabs after the license only where they fit.
   */
  const links = $derived([README, LICENSE, CODE_OF_CONDUCT, CONTRIBUTING, wide ? SECURITY_POLICY : SECURITY]);
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

{#snippet controls()}
  <span class="flex shrink-0 gap-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

{#snippet link(item: RepositoryLink)}
  <span class="flex h-4 shrink-0 items-center gap-1.5 whitespace-nowrap">
    <item.icon size={13} strokeWidth={1.8} aria-hidden="true" />
    {item.label()}
  </span>
{/snippet}

{#snippet actions()}
  <div class="flex shrink-0 items-center gap-2 text-[11px] font-medium">
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
          <Star size={13} strokeWidth={1.8} color={starred ? STAR_GOLD : "currentColor"} fill={starred ? STAR_GOLD : "none"} aria-hidden="true" />
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
          <span class="pointer-events-none absolute size-6 rounded-full bg-foreground/20 ring-2 ring-foreground/35" style:opacity={touch}></span>
        {/if}
      </span>
      {starred ? t("vaultSetup.about.repository.starred") : t("vaultSetup.about.repository.star")}
    </span>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5" style:width={`${width}px`} style:height={`${height}px`}>
  <div
    class="relative flex flex-col overflow-hidden rounded-2xl bg-background text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    <!-- The browser's bar, with the repository's address and the window's controls on the system's side. -->
    <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-3">
      {#if CONTROLS_LEADING}{@render controls()}{/if}
      <span class="min-w-0 flex-1 truncate rounded-full bg-foreground/6 px-2.5 py-0.5 text-center text-[10px] text-muted-foreground">
        {ABOUT_REPOSITORY.host}/{ABOUT_REPOSITORY.owner}/{ABOUT_REPOSITORY.name}
      </span>
      {#if !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    {#if page > 0}
      <div
        class="flex min-h-0 flex-1 flex-col gap-2.5 px-3.5 py-3"
        style:opacity={page}
        style:transform={`translateY(${(1 - page) * PAGE_RISE}px)`}
      >
        <!-- The repository's name, with its actions beside it when there is room and below it when there is not. -->
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
          {#if wide}
            <span class="flex-1"></span>
            {@render actions()}
          {/if}
        </div>
        {#if !wide}{@render actions()}{/if}

        <div class="flex min-h-0 flex-1 gap-4">
          <!-- The file list, with placeholder bars for the latest commit and each entry's last change. -->
          <div class="flex min-w-0 flex-1 flex-col overflow-hidden rounded-lg border border-border">
            <div class="flex h-7 shrink-0 items-center gap-2 border-b border-border bg-foreground/4 px-2.5">
              <span class="size-3.5 shrink-0 rounded-full bg-foreground/15"></span>
              {@render bar("w-2/5")}
              <span class="flex-1"></span>
              {@render bar("w-10")}
            </div>
            {#each ABOUT_FILES as file, index (file.name)}
              {@const row = aboutRow(elapsed, index)}
              <div
                class="flex min-h-5.5 flex-1 basis-0 items-center gap-2 border-b border-border/60 px-2.5 last:border-b-0"
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

          {#if wide}
            <!-- The sidebar: placeholder bars for the description, the repository's documents, and its languages. -->
            <div class="flex w-48 shrink-0 flex-col gap-2.5 overflow-hidden pt-1 text-[11px] text-muted-foreground">
              <div class="flex flex-col gap-2">
                {#each DESCRIPTION_WIDTHS as fraction, index (index)}{@render bar("", fraction)}{/each}
              </div>
              <div class="flex flex-col gap-2">
                {#each links as item, index (index)}{@render link(item)}{/each}
              </div>
              <div class="flex flex-col gap-1.5 border-t border-border pt-3">
                <span class="text-[12px] font-semibold text-foreground">{t("vaultSetup.about.repository.languages")}</span>
                <span class="flex h-1.5 gap-px overflow-hidden rounded-full" aria-hidden="true">
                  {#each LANGUAGES as language (language.name)}
                    <span class="min-w-px" style:flex-grow={language.share} style:background-color={language.color}></span>
                  {/each}
                </span>
                <span class="flex flex-wrap gap-x-2.5 gap-y-1 text-[10px]">
                  {#each LANGUAGES as language (language.name)}
                    <span class="flex items-center gap-1 whitespace-nowrap">
                      <span class="size-1.5 rounded-full" style:background-color={language.color}></span>
                      <span class="font-semibold text-foreground">{language.name}</span>
                      {formatNumber(localization.locale, language.share, LANGUAGE_SHARE)}
                    </span>
                  {/each}
                </span>
              </div>
            </div>
          {/if}
        </div>

        {#if !wide}
          <!-- The documents' tabs: the readme's is selected, and the tabs after the license wrap out of sight when there is no room for them. -->
          <div class="flex h-6 shrink-0 flex-wrap gap-x-4 gap-y-2 overflow-hidden border-b border-border text-[11px] text-muted-foreground">
            {#each links as item, index (index)}
              <span
                class={`flex h-6 shrink-0 items-center whitespace-nowrap ${item === README ? "border-b-2 border-primary font-semibold text-foreground" : ""}`}
              >
                {item.label()}
              </span>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>
