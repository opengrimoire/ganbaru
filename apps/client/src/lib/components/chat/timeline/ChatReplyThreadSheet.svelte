<!--
  @component
  The phone reply thread: a bottom sheet over the channel that slides in nearly full, opens fully from its handle or
  when its composer takes focus, and closes from a swipe down, a tap above it, or Back.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { createBackdropPress } from "$lib/utils/backdrop-press";
  import ChatReplyThreadPanel from "./ChatReplyThreadPanel.svelte";
  import { ReplyThreadSheetController } from "./reply-thread-sheet-controller.svelte";
  import { REPLY_THREAD_SHEET_SETTLE_MS } from "./reply-thread-sheet";

  let { onClose }: { onClose: () => void } = $props();

  const { t } = getLocalization();
  const androidSystemBackAvailable = platformHasCapability(BUILD_PLATFORM_PROFILE, "system.android-back");
  const mobileBackStack = getMobileBackStack();
  const backdropPress = createBackdropPress();
  let track = $state<HTMLDivElement | undefined>();
  let sheetElement = $state<HTMLDivElement | undefined>();

  const sheet = new ReplyThreadSheetController({
    track: () => track,
    sheet: () => sheetElement,
    onClose: () => onClose(),
  });
  /** The backdrop darkens with how far the sheet is open, so it fades in and out with the slide. */
  const backdropOpacity = $derived(sheet.heights.peek > 0 ? Math.min(1, sheet.visibleHeight / sheet.heights.peek) : 0);
  /** Whether the thread timeline is scrolled away from its top, which the handle marks with a divider below it. */
  let scrolledFromTop = $state(false);

  function noteThreadScroll(event: Event): void {
    const target = event.target;
    if (target instanceof HTMLElement && target.hasAttribute("data-reply-thread-scroll")) scrolledFromTop = target.scrollTop >= 1;
  }

  onMount(() => (androidSystemBackAvailable ? mobileBackStack.activate({ handle: () => sheet.close() }) : undefined));
</script>

<div class="thread-sheet-frame" style:--thread-sheet-settle={`${REPLY_THREAD_SHEET_SETTLE_MS}ms`}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="thread-sheet-backdrop surface-backdrop"
    data-sheet-motion={sheet.motion ?? undefined}
    style:opacity={backdropOpacity}
    onpointerdown={backdropPress.start}
    onclick={(event) => { if (backdropPress.isClick(event)) sheet.close(); }}
  ></div>
  <div bind:this={track} class="thread-sheet-track">
    <div
      bind:this={sheetElement}
      class="thread-sheet rounded-t-sheet"
      data-reply-thread-sheet
      data-sheet-motion={sheet.motion ?? undefined}
      style:height={sheet.cssHeight}
      inert={sheet.closing}
    >
      <div class="thread-sheet-content" style:--thread-sheet-peek={`${sheet.heights.peek}px`} onscrollcapture={noteThreadScroll}>
        <div class="thread-sheet-handle" class:scrolled={scrolledFromTop} data-reply-thread-sheet-handle aria-hidden="true"><span class="thread-sheet-grabber"></span><span class="thread-sheet-title">{t("chat.organization.thread")}</span></div>
        <div class="min-h-0 flex-1"><ChatReplyThreadPanel presentation="dialog" onClose={() => sheet.close()} /></div>
      </div>
    </div>
    {#if sheet.entered && !sheet.closing}<div class="thread-sheet-skirt" aria-hidden="true"></div>{/if}
  </div>
</div>

<style>
  /*
   * The visible viewport down to the system navigation, keyboard or not, so the backdrop and the sheet's skirt cover
   * the space a hiding keyboard leaves while the composer catches up with it.
   */
  .thread-sheet-frame { position:fixed; z-index:50; top:var(--visual-viewport-offset-top); left:var(--visual-viewport-offset-left); width:var(--visual-viewport-width); height:calc(var(--visual-viewport-full-height) - var(--safe-area-bottom)); overflow:clip; pointer-events:none; }
  .thread-sheet-backdrop { position:absolute; inset:0; pointer-events:auto; }
  /* The area the sheet may fill, ending above the keyboard; the sheet rests on its bottom edge. */
  .thread-sheet-track { position:absolute; top:calc(var(--safe-area-top) + 0.5rem); right:var(--safe-area-right); bottom:calc(var(--visual-viewport-full-height) - var(--keyboard-surface-height)); left:var(--safe-area-left); pointer-events:none; }
  /*
   * The sheet's color continued under the keyboard. Android before 11 reports the keyboard only after it has gone and
   * then slides the composer down on its own, so this keeps the sheet solid there instead of showing the channel.
   */
  .thread-sheet-skirt { position:absolute; top:100%; right:0; left:0; height:calc(var(--visual-viewport-full-height) - var(--keyboard-surface-height)); background:var(--cal-bg); pointer-events:none; }
  .thread-sheet { position:absolute; right:0; bottom:0; left:0; max-height:100%; overflow:clip; background:var(--cal-bg); box-shadow:var(--floating-shadow); pointer-events:auto; }
  .thread-sheet[data-sheet-motion="settle"], .thread-sheet-backdrop[data-sheet-motion="settle"] { transition:height var(--thread-sheet-settle) cubic-bezier(0.2, 0.8, 0.2, 1), opacity var(--thread-sheet-settle) ease; }
  /* The thread lays out at the sheet's height but never below the peek, so sliding in or out moves it with the top edge. */
  .thread-sheet-content { display:flex; flex-direction:column; height:100%; min-height:var(--thread-sheet-peek); }
  /*
   * The grabber and the sheet name: the whole strip moves the sheet, and the name says what the sheet holds. Once the
   * timeline scrolls away from its top, a divider like the date dividers separates the strip from it.
   */
  .thread-sheet-handle { display:flex; flex:0 0 auto; flex-direction:column; align-items:center; gap:0.6rem; padding-block:0.75rem 0.5625rem; border-bottom:1px solid transparent; background:var(--cal-bg); touch-action:none; user-select:none; }
  .thread-sheet-handle.scrolled { border-bottom-color:var(--border); }
  .thread-sheet-grabber { width:2.25rem; height:2px; border-radius:9999px; background:var(--foreground); }
  .thread-sheet-title { color:var(--foreground); font-size:var(--chat-organizational-font-size,calc(0.875rem * var(--type-scale))); font-weight:500; line-height:1; }
  @media (prefers-reduced-motion: reduce) { .thread-sheet[data-sheet-motion="settle"], .thread-sheet-backdrop[data-sheet-motion="settle"] { transition:none; } }
</style>
