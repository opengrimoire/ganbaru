<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import X from "@lucide/svelte/icons/x";
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
  import { BLOCKER_APP_HOSTS, BLOCKER_APP_NAMES, type BlockerApp, type BlockerBrowserPage } from "$lib/vault/welcome-blocker";
  import WelcomeParodyFeed from "./WelcomeParodyFeed.svelte";
  import WelcomeParodyIcon from "./WelcomeParodyIcon.svelte";

  /**
   * A laptop's browser in the distraction blocker's welcome scenes, with one tab on a parody app's website and the
   * Ganbaru extension beside the address bar. When the site is blocked, the tab shows the extension's real block page,
   * which is in English only, so its text stays in English in every app language. It fills the room it is given.
   */
  let { app, page, pageIn }: { app: BlockerApp; page: BlockerBrowserPage; pageIn: number } = $props();

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
  /** Apple's windows keep their controls on the left; other systems keep them on the right. */
  const CONTROLS_LEADING = BUILD_PLATFORM === "macos" || BUILD_PLATFORM === "ios";

  const host = $derived(BLOCKER_APP_HOSTS[app]);
</script>

{#snippet controls()}
  <span class="flex h-6 shrink-0 items-center gap-1 px-1" aria-hidden="true">
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
    <span class="size-2 rounded-full bg-foreground/15"></span>
  </span>
{/snippet}

<div class="h-full rounded-3xl bg-foreground/6 p-1.5">
  <div class="relative flex h-full flex-col overflow-hidden rounded-2xl bg-background text-foreground">
    <div class="flex h-8 shrink-0 items-end gap-1 bg-foreground/5 px-1.5">
      {#if CONTROLS_LEADING}{@render controls()}{/if}
      <span class="flex h-6 max-w-36 min-w-0 flex-1 items-center gap-1.5 rounded-t-md bg-background px-2 text-[10px]">
        {#if page.kind === "block"}
          <img src={appIconUrl} alt="" class="size-3 shrink-0 rounded-full" draggable="false" />
          <span class="min-w-0 flex-1 truncate" lang={PAGE_LOCALE}>{PAGE_TEXT.tabTitle}</span>
        {:else}
          <WelcomeParodyIcon {app} size={11} />
          <span class="min-w-0 flex-1 truncate">{BLOCKER_APP_NAMES[app]}</span>
        {/if}
        <X size={10} strokeWidth={1.8} class="shrink-0 opacity-60" aria-hidden="true" />
      </span>
      <span class="mb-1 grid size-5 shrink-0 place-items-center rounded-md text-muted-foreground">
        <Plus size={12} strokeWidth={1.8} aria-hidden="true" />
      </span>
      <span class="flex-1"></span>
      {#if !CONTROLS_LEADING}{@render controls()}{/if}
    </div>

    <div class="flex h-8 shrink-0 items-center gap-2 border-b border-border px-2.5 text-muted-foreground">
      <ArrowLeft size={12} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      <ArrowRight size={12} strokeWidth={1.8} class="shrink-0 opacity-50" aria-hidden="true" />
      <RotateCw size={11} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      <span class="flex h-5 min-w-0 flex-1 items-center gap-1.5 rounded-full bg-foreground/6 px-2.5 text-[10px] text-foreground">
        {#if page.kind === "block"}
          <span class="flex shrink-0 items-center gap-1 rounded-full bg-background px-1.5 py-px">
            <img src={appIconUrl} alt="" class="size-2.5 rounded-full" draggable="false" />
            {t("common.appName")}
          </span>
          <span class="h-1.5 w-1/4 rounded-full bg-foreground/12"></span>
        {:else}
          <span class="truncate">{host}</span>
        {/if}
      </span>
      <img src={appIconUrl} alt="" class="size-3.5 shrink-0 rounded-full" draggable="false" />
    </div>

    <div class="relative min-h-0 flex-1 overflow-hidden">
      <div class="absolute inset-0" style:opacity={pageIn}>
        {#if page.kind === "feed"}
          <WelcomeParodyFeed {app} scroll={page.scroll} travel={FEED_TRAVEL} />
        {:else}
          <div
            class="absolute inset-0 flex flex-col items-center justify-center gap-1.5 px-4 text-center"
            lang={PAGE_LOCALE}
            style:background-color={BLOCK_PAGE.bg}
            style:color={BLOCK_PAGE.fg}
          >
            <span class="mb-1.5"><WelcomeParodyIcon {app} size={22} /></span>
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
