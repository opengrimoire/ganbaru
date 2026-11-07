<script lang="ts">
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Menu from "@lucide/svelte/icons/menu";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Plus from "@lucide/svelte/icons/plus";
  import RotateCw from "@lucide/svelte/icons/rotate-cw";
  import X from "@lucide/svelte/icons/x";
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM } from "$lib/platform";
  import { BLOCKER_APP_HOSTS, BLOCKER_APP_NAMES, type BlockerApp, type BlockerBrowserFrame } from "$lib/vault/welcome-blocker";
  import WelcomeParodyFeed from "./WelcomeParodyFeed.svelte";
  import WelcomeParodyIcon, { MAIL_COLORS } from "./WelcomeParodyIcon.svelte";

  /**
   * A laptop's browser in the distraction blocker's welcome scenes, with its tabs, and the Ganbaru extension and the
   * browser's menu beside the address bar. A tab shows a parody mail inbox, a new tab, a parody app's website, or the
   * extension's real block page, which is in English only, so its text stays in English in every app language. It fills the room it is given.
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
  /** How far each site's feed scrolls at its end, in pixels; Bluedit's tall media needs more to read as a scroll. */
  const FEED_TRAVEL: Readonly<Record<BlockerApp, number>> = { y: 150, bluedit: 280, egogram: 150 };
  /** The inbox's folder labels, as fractions of the side bar's width; the first is the inbox itself, selected. */
  const MAIL_FOLDERS = [0.7, 0.55, 0.8, 0.45, 0.6];
  /** The inbox's messages: each sender's width in pixels, its subject's as a fraction of the room left, and if unread. */
  const MAIL_ROWS: readonly { sender: number; subject: number; unread: boolean }[] = [
    { sender: 42, subject: 0.86, unread: true },
    { sender: 30, subject: 0.64, unread: true },
    { sender: 50, subject: 0.92, unread: false },
    { sender: 36, subject: 0.58, unread: false },
    { sender: 46, subject: 0.78, unread: false },
    { sender: 28, subject: 0.7, unread: false },
    { sender: 40, subject: 0.5, unread: false },
    { sender: 34, subject: 0.84, unread: false },
    { sender: 48, subject: 0.66, unread: false },
    { sender: 32, subject: 0.74, unread: false },
  ];
  /** The compose button's and the selected folder's tint, from the mail site's blue. */
  const COMPOSE_BG = `color-mix(in srgb, ${MAIL_COLORS.blue} 18%, transparent)`;
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
          {#if tab.content === "mail"}
            <WelcomeParodyIcon app="mail" size={11} />
            <span class="min-w-0 flex-1 truncate">{t("vaultSetup.blocker.mailTab")}</span>
          {:else if tab.content === "blank"}
            <span class="min-w-0 flex-1 truncate">{t("vaultSetup.blocker.newTab")}</span>
          {:else if tab.content === "blocked"}
            <img src={appIconUrl} alt="" class="size-3 shrink-0 rounded-[22%]" draggable="false" />
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
      <span class="relative flex h-5 min-w-0 flex-1 items-center gap-1.5 rounded-full bg-foreground/6 px-2.5 text-[10px] text-foreground">
        {#if frame.address.kind === "extension"}
          <span class="flex shrink-0 items-center gap-1 rounded-full bg-background px-1.5 py-px">
            <img src={appIconUrl} alt="" class="size-2.5 rounded-[22%]" draggable="false" />
            {t("common.appName")}
          </span>
          <span class="h-1.5 w-1/4 rounded-full bg-foreground/12"></span>
        {:else if frame.address.kind === "typed"}
          <span class="flex min-w-0 items-center">
            <span class="truncate whitespace-pre">{frame.address.text}</span>
            <span class="h-3 w-px shrink-0 bg-foreground" style:opacity={frame.address.caret ? 1 : 0}></span>
          </span>
        {:else}
          <span class="truncate">{frame.address.host}</span>
        {/if}
        {@render touch(frame.addressTouch)}
      </span>
      <img src={appIconUrl} alt="" class="size-3.5 shrink-0 rounded-[22%]" draggable="false" />
      <Menu size={12} strokeWidth={1.8} class="shrink-0" aria-hidden="true" />
      {#if frame.loading > 0}
        <span class="absolute bottom-0 left-0 h-0.5 bg-primary" style:width={`${frame.loading * 100}%`}></span>
      {/if}
    </div>

    <div class="relative min-h-0 flex-1 overflow-hidden">
      <div class="absolute inset-0" style:opacity={frame.pageIn}>
        {#if page.kind === "feed"}
          <WelcomeParodyFeed app={page.app} scroll={page.scroll} travel={FEED_TRAVEL[page.app]} />
        {:else if page.kind === "mail"}
          <!-- The inbox: a compose button and folders on the side, and a list of messages, the unread ones darker. -->
          <div class="absolute inset-0 flex overflow-hidden bg-foreground/4">
            <div class="flex w-20 shrink-0 flex-col gap-1 px-1.5 pt-2">
              <span class="mb-1 flex h-6 items-center gap-1.5 rounded-lg px-2" style:background-color={COMPOSE_BG}>
                <Pencil size={10} strokeWidth={2} class="shrink-0" aria-hidden="true" />
                <span class="h-1.5 w-7 rounded-full bg-foreground/30"></span>
              </span>
              {#each MAIL_FOLDERS as fraction, index (index)}
                <span class="flex h-3.5 shrink-0 items-center rounded-full px-2" style:background-color={index === 0 ? COMPOSE_BG : undefined}>
                  <span class="h-1.5 rounded-full bg-foreground/15" style:width={`${fraction * 100}%`}></span>
                </span>
              {/each}
            </div>
            <div class="mt-1.5 mr-1.5 flex min-w-0 flex-1 flex-col overflow-hidden rounded-t-lg bg-background">
              {#each MAIL_ROWS as row, index (index)}
                <span class="flex h-6 shrink-0 items-center gap-2 border-b border-border px-2.5">
                  <span
                    class="h-1.5 shrink-0 rounded-full {row.unread ? 'bg-foreground/45' : 'bg-foreground/15'}"
                    style:width={`${row.sender}px`}
                  ></span>
                  <span
                    class="h-1.5 min-w-0 rounded-full {row.unread ? 'bg-foreground/25' : 'bg-foreground/10'}"
                    style:width={`${row.subject * 100}%`}
                  ></span>
                </span>
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
