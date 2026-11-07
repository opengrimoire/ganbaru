<script lang="ts">
  import appIconUrl from "../../../../../src-tauri/icons/64x64.png";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BLOCKER_APP_NAMES, type BlockerApp, type BlockerNotice, type BlockerPhoneApp } from "$lib/vault/welcome-blocker";
  import WelcomeAndroidNotification from "./WelcomeAndroidNotification.svelte";
  import WelcomeParodyFeed from "./WelcomeParodyFeed.svelte";
  import WelcomeParodyIcon from "./WelcomeParodyIcon.svelte";

  /**
   * A phone in the distraction blocker's welcome scenes, on its home screen with the parody apps in a line below
   * unnamed ones and Ganbaru in the dock. An app opens from its icon and shrinks back into it as Android returns Home, and Ganbaru's block
   * notification slides in from the top as Android draws it.
   */
  let {
    width,
    height,
    focusing = false,
    app = null,
    notice = null,
    touches = {},
  }: {
    width: number;
    height: number;
    /** Whether a Focus phase runs, which the status bar shows. */
    focusing?: boolean;
    app?: BlockerPhoneApp | null;
    notice?: BlockerNotice | null;
    /** The taps on the apps' icons, each from 0 to 1 as its touch shows. */
    touches?: Partial<Record<BlockerApp, number>>;
  } = $props();

  const { t } = getLocalization();

  const BEZEL = 6;
  /** The home screen: its status bar, its icon grid of four columns, and its side padding. */
  const STATUS_H = 18;
  const GRID_COLUMNS = 4;
  const GRID_PAD_X = 8;
  const GRID_TOP = STATUS_H + 14;
  const ICON_LABEL_H = 16;
  /** The home screen's slots, row by row: unnamed apps, with the parody apps in a line on the last row. */
  const HOME_SLOTS: readonly (BlockerApp | null)[] = [null, null, null, null, null, null, null, null, "egogram", "y", "bluedit", null];
  /** How far a label may spill past its column, in pixels, so a longer name fits before it truncates. */
  const LABEL_OVERHANG = 10;
  /** The block notification's gap from the screen's edges and its body text size, in pixels. */
  const NOTICE_MARGIN = 6;
  const NOTICE_FONT_PX = 9.5;
  /** The dock's four apps, unnamed but for Ganbaru in the last one. */
  const DOCK_SLOTS: readonly ("ganbaru" | null)[] = [null, null, null, "ganbaru"];
  const DOCK_BOTTOM = 18;
  /** How far an app's feed scrolls at its end, in pixels. */
  const FEED_TRAVEL = 170;

  const screenW = $derived(width - 2 * BEZEL);
  const screenH = $derived(height - 2 * BEZEL);
  const cellW = $derived((screenW - 2 * GRID_PAD_X) / GRID_COLUMNS);
  const iconSize = $derived(Math.min(30, Math.floor(cellW - 8)));
  const rowH = $derived(iconSize + ICON_LABEL_H);

  /** The center of a home screen slot's icon, in pixels from the screen's corner. */
  function slotCenter(slot: number): { x: number; y: number } {
    const column = slot % GRID_COLUMNS;
    const row = Math.floor(slot / GRID_COLUMNS);
    return { x: GRID_PAD_X + cellW * (column + 0.5), y: GRID_TOP + rowH * row + iconSize / 2 };
  }

  const openOrigin = $derived(app ? slotCenter(HOME_SLOTS.indexOf(app.app)) : { x: screenW / 2, y: screenH / 2 });
</script>

{#snippet placeholder()}
  <span class="block rounded-[30%] bg-foreground/10" style:width={`${iconSize}px`} style:height={`${iconSize}px`}></span>
{/snippet}

<div class="rounded-[26px] bg-foreground/6 p-1.5" style:width={`${width}px`} style:height={`${height}px`}>
  <div class="relative h-full overflow-hidden rounded-[20px] bg-foreground/3 text-foreground">
    <div class="absolute inset-x-0 top-0 z-20 flex items-center gap-1 px-3 text-muted-foreground" style:height={`${STATUS_H}px`}>
      {#if focusing}
        <svg class="size-2.5" viewBox="0 0 24 24" aria-hidden="true">
          <path
            fill="currentColor"
            d="M12,2A10,10 0,1 0,22 12A10,10 0,0 0,12 2M12,5A7,7 0,1 1,5 12A7,7 0,0 1,12 5M11,7H13V12.4L16.4,14.4L15.4,16.1L11,13.5Z"
          />
        </svg>
      {/if}
      <span class="flex-1"></span>
      <span class="h-1.5 w-2.5 rounded-xs bg-current opacity-70"></span>
      <span class="h-1.5 w-3.5 rounded-xs bg-current opacity-70"></span>
    </div>

    {#each HOME_SLOTS as homeApp, index (index)}
      {@const center = slotCenter(index)}
      {@const touch = homeApp === null ? 0 : (touches[homeApp] ?? 0)}
      <span
        class="absolute flex flex-col items-center gap-1"
        style:left={`${center.x - cellW / 2}px`}
        style:top={`${center.y - iconSize / 2}px`}
        style:width={`${cellW}px`}
      >
        <span class="relative">
          {#if homeApp === null}
            {@render placeholder()}
          {:else}
            <WelcomeParodyIcon app={homeApp} size={iconSize} />
          {/if}
          {#if touch > 0}
            <span
              class="pointer-events-none absolute top-1/2 left-1/2 size-6 -translate-1/2 rounded-full bg-foreground/20 ring-2 ring-foreground/35"
              style:opacity={touch}
            ></span>
          {/if}
        </span>
        {#if homeApp === null}
          <span class="h-1 w-5 rounded-full bg-foreground/12"></span>
        {:else}
          <span class="truncate text-[8px] leading-none" style:max-width={`${cellW + LABEL_OVERHANG}px`}>
            {BLOCKER_APP_NAMES[homeApp]}
          </span>
        {/if}
      </span>
    {/each}

    <div class="absolute inset-x-0 flex justify-around px-2" style:bottom={`${DOCK_BOTTOM}px`}>
      {#each DOCK_SLOTS as dockApp, index (index)}
        {#if dockApp === "ganbaru"}
          <img
            src={appIconUrl}
            alt=""
            class="block rounded-[22%]"
            style:width={`${iconSize}px`}
            style:height={`${iconSize}px`}
            draggable="false"
          />
        {:else}
          {@render placeholder()}
        {/if}
      {/each}
    </div>
    <span class="absolute bottom-1.5 left-1/2 h-1 w-10 -translate-x-1/2 rounded-full bg-foreground/30"></span>

    {#if app}
      <div
        class="absolute inset-0 z-10 overflow-hidden"
        style:transform-origin={`${openOrigin.x}px ${openOrigin.y}px`}
        style:transform={`scale(${0.12 + 0.88 * app.open})`}
        style:opacity={Math.min(1, app.open * 3)}
        style:border-radius={`${20 + (1 - app.open) * 40}px`}
      >
        <WelcomeParodyFeed app={app.app} scroll={app.scroll} travel={FEED_TRAVEL} topInset={STATUS_H} phone />
      </div>
    {/if}

    {#if notice}
      <WelcomeAndroidNotification
        shown={notice.shown}
        margin={NOTICE_MARGIN}
        fontSize={NOTICE_FONT_PX}
        title={BLOCKER_APP_NAMES[notice.app]}
        body={notice.reason === "focus" ? t("settings.distractions.mobile.blockedMessage") : t("settings.distractions.mobile.limitMessage")}
        class="z-30"
      />
    {/if}
  </div>
</div>
