<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import X from "@lucide/svelte/icons/x";
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
  import { BLOCKER_APP_HOSTS, BLOCKER_APP_NAMES, type BlockerBrowserFrame } from "$lib/vault/welcome-blocker";
  import WelcomeParodyFeed from "./WelcomeParodyFeed.svelte";
  import WelcomeParodyIcon from "./WelcomeParodyIcon.svelte";

  /**
   * A laptop's browser in the distraction blocker's welcome scenes, with its tabs and the Ganbaru extension beside the
   * address bar. A tab shows a work document, a new tab, a parody app's website, or the extension's real block page,
   * which is in English only, so its text stays in English in every app language. It fills the room it is given.
   */
  let { frame }: { frame: BlockerBrowserFrame } = $props();

  const { t } = getLocalization();

  /** The extension's block page text, which it shows in English only. */
  const PAGE_LOCALE = "en";
  const PAGE_TEXT = {
    tabTitle: "Blocked during focus",
    focusTitle: (host: string) => `${host} is blocked`,
    focusCopy: "Stay strong and keep moving forward.",
    focusTime: (minutes: number) => `${minutes} min left in focus`,
    limitTitle: (host: string) => `${host} reached today's limit`,
    limitCopy: "Come back after the limit resets.",
    limitTime: "Daily limit reached",
    actions: ["Recheck", "Close tab"],
  } as const;
  /** The block page's colors, which the extension keeps dark in every theme. */
  const BLOCK_PAGE = { bg: "#0f1110", fg: "#f4f5f0", muted: "#aab5ac", button: "#f4f5f0", buttonFg: "#101010" } as const;
  /** How far the feed scrolls at its end, in pixels. */
  const FEED_TRAVEL = 150;
  /** The work document's placeholder lines, as fractions of the page's width; 0 leaves a gap between paragraphs. */
  const DOC_WIDTHS = [1, 0.94, 0.97, 0.62, 0, 1, 0.9, 0.96, 0.85, 0.48, 0, 0.98, 0.92, 0.7];
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";

  const page = $derived(frame.page);
</script>

{#snippet controls()}
  <span class="flex h-6 shrink-0 items-center gap-1 px-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

{#snippet touch(shown: number)}
  {#if shown > 0}
    <span
      class="pointer-events-none absolute top-1/2 left-1/2 size-5 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
      style:opacity={shown}
    ></span>
  {/if}
{/snippet}

<div class="h-full rounded-3xl bg-foreground/6 p-1.5">
  <div class="relative flex h-full flex-col overflow-hidden rounded-2xl bg-background text-foreground">
    <div class="flex h-8 shrink-0 items-end gap-1 bg-foreground/5 px-1.5">
      {#if CONTROLS_LEADING}{@render controls()}{/if}
      {#each frame.tabs as tab, index (index)}
        <span
          class="flex h-6 max-w-32 min-w-0 flex-1 items-center gap-1.5 rounded-t-md px-2 text-[10px]"
          class:bg-background={tab.active}
          class:text-muted-foreground={!tab.active}
        >
          {#if tab.content === "work"}
            <FileText size={11} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate">{t("vaultSetup.blocker.workTab")}</span>
          {:else if tab.content === "blank"}
            <span class="min-w-0 flex-1 truncate">{t("vaultSetup.blocker.newTab")}</span>
          {:else if tab.content === "blocked"}
            <img src={appIconUrl} alt="" class="size-3 shrink-0 rounded-full" draggable="false" />
            <span class="min-w-0 flex-1 truncate" lang={PAGE_LOCALE}>{PAGE_TEXT.tabTitle}</span>
          {:else}
            <WelcomeParodyIcon app={tab.content} size={11} />
            <span class="min-w-0 flex-1 truncate">{BLOCKER_APP_NAMES[tab.content]}</span>
          {/if}
          <span class="relative flex shrink-0">
            <X size={10} strokeWidth={1.8} class="opacity-60" aria-hidden="true" />
            {@render touch(tab.closeTouch ?? 0)}
          </span>
        </span>
      {/each}
      <span class="relative mb-1 grid size-5 shrink-0 place-items-center rounded-md text-muted-foreground">
        <Plus size={12} strokeWidth={1.8} aria-hidden="true" />
        {@render touch(frame.newTabTouch)}
      </span>
      <span class="flex-1"></span>
      {#if !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    <div class="relative flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5 text-muted-foreground">
      <ArrowLeft size={12} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      <ArrowRight size={12} strokeWidth={1.8} class="shrink-0 opacity-50" aria-hidden="true" />
      <RotateCw size={11} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      <span class="flex h-5 min-w-0 flex-1 items-center gap-1.5 rounded-full bg-foreground/6 px-2.5 text-[10px] text-foreground">
        {#if frame.address.kind === "extension"}
          <span class="flex shrink-0 items-center gap-1 rounded-full bg-background px-1.5 py-px">
            <img src={appIconUrl} alt="" class="size-2.5 rounded-full" draggable="false" />
            {t("common.appName")}
          </span>
          <span class="h-1.5 w-1/4 rounded-full bg-foreground/12"></span>
        {:else if frame.address.kind === "placeholder"}
          <span class="h-1.5 w-2/5 rounded-full bg-foreground/12"></span>
        {:else if frame.address.kind === "typed"}
          <span class="flex min-w-0 items-center">
            <span class="truncate whitespace-pre">{frame.address.text}</span>
            <span class="h-3 w-px shrink-0 bg-foreground" style:opacity={frame.address.caret ? 1 : 0}></span>
          </span>
        {:else}
          <span class="truncate">{frame.address.host}</span>
        {/if}
      </span>
      <img src={appIconUrl} alt="" class="size-3.5 shrink-0 rounded-full" draggable="false" />
      {#if frame.loading > 0}
        <span class="absolute bottom-0 left-0 h-0.5 bg-primary" style:width={`${frame.loading * 100}%`}></span>
      {/if}
    </div>

    <div class="relative min-h-0 flex-1 overflow-hidden">
      <div class="absolute inset-0" style:opacity={frame.pageIn}>
        {#if page.kind === "feed"}
          <WelcomeParodyFeed app={page.app} scroll={page.scroll} travel={FEED_TRAVEL} />
        {:else if page.kind === "work"}
          <div class="absolute inset-0 overflow-hidden bg-foreground/3 px-4 pt-3">
            <div class="mx-auto flex max-w-64 flex-col gap-1.5 rounded-sm bg-background px-4 py-4">
              <span class="mb-1.5 block h-2.5 w-1/2 rounded-full bg-foreground/25"></span>
              {#each DOC_WIDTHS as fraction, index (index)}
                {#if fraction > 0}
                  <span class="block h-1.5 shrink-0 rounded-full bg-foreground/12" style:width={`${fraction * 100}%`}></span>
                {:else}
                  <span class="block h-1.5 shrink-0"></span>
                {/if}
              {/each}
            </div>
          </div>
        {:else if page.kind === "blank"}
          <div class="absolute inset-0 bg-background"></div>
        {:else}
          {@const host = BLOCKER_APP_HOSTS[page.app]}
          <div
            class="absolute inset-0 flex flex-col items-center justify-center gap-1.5 px-4 text-center"
            lang={PAGE_LOCALE}
            style:background-color={BLOCK_PAGE.bg}
            style:color={BLOCK_PAGE.fg}
          >
            <span class="mb-1.5"><WelcomeParodyIcon app={page.app} size={22} /></span>
            <p class="max-w-full truncate text-[15px] leading-tight font-bold">
              {page.reason === "focus" ? PAGE_TEXT.focusTitle(host) : PAGE_TEXT.limitTitle(host)}
            </p>
            <p class="text-[10.5px] leading-snug" style:color={BLOCK_PAGE.muted}>
              {page.reason === "focus" ? PAGE_TEXT.focusCopy : PAGE_TEXT.limitCopy}
            </p>
            <p class="text-[10px] font-bold">{page.reason === "focus" ? PAGE_TEXT.focusTime(page.minutesLeft) : PAGE_TEXT.limitTime}</p>
            <div class="mt-3 flex gap-2 text-[10px] font-semibold">
              {#each PAGE_TEXT.actions as action (action)}
                <span
                  class="flex h-6 min-w-16 items-center justify-center rounded-md px-3"
                  style:background-color={BLOCK_PAGE.button}
                  style:color={BLOCK_PAGE.buttonFg}
                >
                  {action}
                </span>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>
