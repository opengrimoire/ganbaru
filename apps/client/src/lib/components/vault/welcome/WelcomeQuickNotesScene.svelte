<script lang="ts" module>
  import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

  /** Canvas geometry in logical pixels: a width and a height the room chooses between these bounds. */
  const MIN_CANVAS_W = BUILD_PLATFORM_PROFILE.shell === "mobile" ? 360 : 480;
  const MAX_CANVAS_W = 760;
  const MIN_CANVAS_H = 470;
  const MAX_CANVAS_H = 540;
  /** The bezel around the canvas, matching the other welcome scenes (`p-1.5`). */
  const BEZEL = 6;

  export const QUICK_NOTES_SCENE_MIN_WIDTH = MIN_CANVAS_W + 2 * BEZEL;
  export const QUICK_NOTES_SCENE_MAX_WIDTH = MAX_CANVAS_W + 2 * BEZEL;
  export const QUICK_NOTES_SCENE_MIN_HEIGHT = MIN_CANVAS_H + 2 * BEZEL;
  export const QUICK_NOTES_SCENE_MAX_HEIGHT = MAX_CANVAS_H + 2 * BEZEL;
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import Archive from "@lucide/svelte/icons/archive";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Palette from "@lucide/svelte/icons/palette";
  import Pin from "@lucide/svelte/icons/pin";
  import Plus from "@lucide/svelte/icons/plus";
  import Search from "@lucide/svelte/icons/search";
  import Tag from "@lucide/svelte/icons/tag";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";
  import type { EventColor } from "$lib/calendar/types";
  import WelcomeCursor from "$lib/components/vault/welcome/WelcomeCursor.svelte";
  import WelcomePhoneTopBar from "$lib/components/vault/welcome/WelcomePhoneTopBar.svelte";
  import WelcomeTitleBar, { type WelcomeTitleTab } from "$lib/components/vault/welcome/WelcomeTitleBar.svelte";
  import WelcomeTouch from "$lib/components/vault/welcome/WelcomeTouch.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { quickNoteTagVisibleCount } from "$lib/quick-notes/tags";
  import {
    QUICK_NOTE_DESKTOP_MASONRY_DENSITY,
    QUICK_NOTE_MASONRY_GAP,
    quickNoteMasonryLayout,
    quickNoteMobileMasonryDensity,
    type MasonryPosition,
  } from "$lib/quick-notes/masonry";
  import { getPreferences } from "$lib/stores/preferences.svelte";
  import { getTheme } from "$lib/stores/theme.svelte";
  import { cn } from "$lib/utils";
  import { welcomeCursorFrame, type WelcomeCursorPoint } from "$lib/vault/welcome-cursor";
  import { WELCOME_NO_HOVER, welcomeHoverAt, welcomeHoverSame } from "$lib/vault/welcome-hover";
  import {
    QUICK_NOTES_CAST,
    QUICK_NOTES_CURSOR,
    QUICK_NOTES_FILTERED_NOTES,
    QUICK_NOTES_TAG_ORDER,
    quickNotesFilter,
    quickNotesPickerOpen,
    quickNotesSceneColors,
    quickNotesTouches,
    quickNotesTripProgress,
    type QuickNotesSceneCard,
    type QuickNotesSceneNote,
    type QuickNotesSceneTag,
    type QuickNotesSceneTone,
  } from "$lib/vault/welcome-quick-notes-scene";

  /**
   * One frame of the welcome scene on Quick notes, drawn with the panel's own surfaces: among notes that already carry
   * several tags, two are tagged with the existing Trip tag from their pickers, which gives them the tag's color, and the
   * Trip chip filters the notes down to the ones tagged Trip, which glide together. Tags that do not fit the tag bar
   * collapse into a count, as the tag bar does. Every style is a function of `elapsed`, with no CSS transitions.
   */
  let { elapsed, width, height }: { elapsed: number; width: number; height: number } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const preferences = getPreferences();
  const theme = getTheme();
  const mobileShell = BUILD_PLATFORM_PROFILE.shell === "mobile";

  /** The title bar on desktop, or the phone's top bar. */
  const TOP_BAR_H = mobileShell ? 48 : 36;
  /** The title bar's end padding, as the title bar draws it. */
  const TITLE_BAR_END_PX = 6;
  /** The title bar's tabs: the Calendar, and the Novel project's, which is showing. */
  const TITLE_TABS: readonly WelcomeTitleTab[] = [{ kind: "calendar", active: false }, { kind: "projects", active: true }];
  /** How far the panel keeps from the canvas edges, and from the bar above it, as the panel places itself. */
  const PANEL_INSET_PX = 8;
  const PANEL_TOP_GAP_PX = 4;
  /** The gap between a tag picker and its button, and the room it keeps from the canvas edges, as the picker places itself. */
  const PICKER_GAP_PX = 4;
  const PICKER_INSET_PX = 8;
  /** Below this canvas width, the phone's Add tag keeps only its icon, as it does on narrow screens. */
  const MOBILE_ADD_TAG_LABEL_MIN_W = 400;
  /** A header button, as the panel draws them on each shell. */
  const HEADER_BUTTON_CLASS = mobileShell
    ? "flex min-h-12 min-w-12 shrink-0 items-center justify-center rounded-xl text-foreground"
    : "flex size-7 items-center justify-center rounded-md text-foreground";
  const HEADER_ICON_CLASS = mobileShell ? "size-5" : "size-3.5";
  /** A card's action button, as the card draws them. */
  const ACTION_CLASS = "flex size-7 items-center justify-center rounded-md";
  const ACTION_ACTIVE_CLASS = "bg-black/10 dark:bg-white/10";

  const canvasW = $derived(width - 2 * BEZEL);
  const canvasH = $derived(height - 2 * BEZEL);

  const sceneColors = $derived(quickNotesSceneColors(preferences.quickNotesDefaultColor));
  const tripName = $derived(t("vaultSetup.quickNotes.tags.trip"));
  const pickerOpen = $derived(quickNotesPickerOpen(elapsed, mobileShell));
  const filter = $derived(quickNotesFilter(elapsed, mobileShell));
  const touches = $derived(quickNotesTouches(elapsed));

  /** Whether the desktop pointer has left the canvas, where it hovers the named parts under it. A phone has touches and no hover. */
  const cursorAway = $derived(welcomeCursorFrame(QUICK_NOTES_CURSOR, elapsed).away);
  let cursorPoint = $state<WelcomeCursorPoint | null>(null);
  let hover = $state<readonly string[]>(WELCOME_NO_HOVER);

  let canvas = $state<HTMLDivElement>();
  let masonryWidth = $state(0);
  let measurer = $state<HTMLDivElement>();
  let badgeSample = $state<HTMLSpanElement>();
  let picker = $state<HTMLDivElement>();
  let tagRow = $state<HTMLDivElement>();
  let allChip = $state<HTMLSpanElement>();
  let addTag = $state<HTMLSpanElement>();
  let tagMeasurer = $state<HTMLDivElement>();
  /** How many tags fit in the tag bar beside All and Add tag; the rest collapse into a count, as the tag bar does. */
  let visibleTagCount = $state(QUICK_NOTES_TAG_ORDER.length);

  /** A box in the logical pixels of the element it is measured against. */
  interface Box {
    top: number;
    left: number;
    width: number;
    height: number;
  }

  /** Bumped whenever the canvas resizes or fonts finish loading, so the layout is measured again. */
  let layoutTick = $state(0);
  /** Each note's height as the scene starts, at the masonry's card width, and the height a badge row adds. */
  let cardMeasure = $state<{ heights: readonly number[]; badgeRow: number } | null>(null);
  /** The open picker's button on the canvas, and the picker's size. */
  let pickerLayout = $state<{ anchor: Box; width: number; height: number } | null>(null);

  $effect(() => {
    const bump = () => {
      layoutTick += 1;
    };
    const observer = typeof ResizeObserver === "undefined" || !canvas ? null : new ResizeObserver(bump);
    if (canvas) observer?.observe(canvas);
    const fonts = typeof document === "undefined" ? undefined : document.fonts;
    fonts?.addEventListener("loadingdone", bump);
    return () => {
      observer?.disconnect();
      fonts?.removeEventListener("loadingdone", bump);
    };
  });

  const density = $derived(mobileShell ? quickNoteMobileMasonryDensity(masonryWidth) : QUICK_NOTE_DESKTOP_MASONRY_DENSITY);
  /** The card width the masonry gives its columns, which the measured notes are laid out at. */
  const cardWidth = $derived(
    quickNoteMasonryLayout(masonryWidth, [0], density.minimumCardWidth, QUICK_NOTE_MASONRY_GAP, density.maximumColumns).positions[0]?.width ?? 0,
  );

  /** Measure the notes at their card width once they lay themselves out for the canvas and the language. */
  $effect(() => {
    void layoutTick;
    void cardWidth;
    void localization.locale;
    if (!measurer || !badgeSample || cardWidth <= 0) return;
    const heights = Array.from(measurer.children, (element) => (element instanceof HTMLElement ? element.offsetHeight : 0));
    cardMeasure = { heights, badgeRow: badgeSample.offsetHeight };
  });

  /**
   * Fit the leading tags beside All and Add tag, leaving room for the count of the rest when some do not fit, as the tag
   * bar does. Trip leads the tags and always stays, since the scene selects it.
   */
  $effect(() => {
    void layoutTick;
    void canvasW;
    void localization.locale;
    if (!tagRow || !tagMeasurer) return;
    const gap = Number.parseFloat(getComputedStyle(tagRow).columnGap) || 0;
    const widths = Array.from(tagMeasurer.querySelectorAll<HTMLElement>("[data-measure-tag]"), (element) => element.offsetWidth);
    const moreWidth = tagMeasurer.querySelector<HTMLElement>("[data-measure-more]")?.offsetWidth ?? 0;
    const fixed = (allChip?.offsetWidth ?? 0) + gap + (addTag?.offsetWidth ?? 0) + gap;
    visibleTagCount = Math.max(1, quickNoteTagVisibleCount(widths, tagRow.clientWidth - fixed, gap, moreWidth));
  });

  const visibleTags = $derived(QUICK_NOTES_TAG_ORDER.slice(0, visibleTagCount));
  const hiddenTagCount = $derived(QUICK_NOTES_TAG_ORDER.length - visibleTags.length);

  /** The count of collapsed tags, signed as the tag bar shows it. */
  function moreLabel(count: number): string {
    return formatNumber(localization.locale, count, { signDisplay: "always" });
  }

  /** How far each note has taken the Trip tag. */
  function tripProgress(key: QuickNotesSceneNote): number {
    return quickNotesTripProgress(key, elapsed, mobileShell);
  }

  /** The masonry of every note, and of the notes the filter keeps once it shows only them. */
  const layouts = $derived.by(() => {
    if (!cardMeasure || masonryWidth <= 0) return null;
    const { heights, badgeRow } = cardMeasure;
    const sized = QUICK_NOTES_CAST.map((card, index) => (heights[index] ?? 0) + badgeRow * tripProgress(card.key));
    const lay = (list: readonly number[]) =>
      quickNoteMasonryLayout(masonryWidth, list, density.minimumCardWidth, QUICK_NOTE_MASONRY_GAP, density.maximumColumns);
    const keptIndexes = QUICK_NOTES_FILTERED_NOTES.map((key) => QUICK_NOTES_CAST.findIndex((card) => card.key === key));
    return { all: lay(sized), filtered: lay(keptIndexes.map((index) => sized[index] ?? 0)) };
  });

  /** Where each note is drawn: in its place among all the notes, carried to its filtered place as the filter glides. */
  function cardPlace(key: QuickNotesSceneNote, index: number): MasonryPosition | null {
    const from = layouts?.all.positions[index];
    if (!from) return null;
    const keptIndex = QUICK_NOTES_FILTERED_NOTES.indexOf(key);
    const to = keptIndex < 0 ? undefined : layouts?.filtered.positions[keptIndex];
    if (!to || filter.glide <= 0) return from;
    const mix = (a: number, b: number) => a + (b - a) * filter.glide;
    return { left: mix(from.left, to.left), top: mix(from.top, to.top), width: mix(from.width, to.width) };
  }

  const masonryHeight = $derived(
    layouts ? layouts.all.height + (layouts.filtered.height - layouts.all.height) * filter.glide : 0,
  );

  /** Measure the open picker against its button once it lays itself out. */
  $effect(() => {
    void layoutTick;
    void canvasW;
    void canvasH;
    void localization.locale;
    void layouts;
    const anchor = pickerOpen ? canvas?.querySelector<HTMLElement>(`[data-cursor="quick-notes-tag-${pickerOpen}"]`) : null;
    if (!canvas || !picker || !anchor) {
      pickerLayout = null;
      return;
    }
    pickerLayout = {
      anchor: relBox(anchor, canvas),
      width: picker.offsetWidth,
      height: picker.offsetHeight,
    };
  });

  /**
   * The open picker's place: below its button when it fits there, above it otherwise, and on the roomier side with its
   * options clipped when neither fits.
   */
  const pickerPlace = $derived.by(() => {
    if (!pickerLayout || !pickerOpen) return null;
    const { anchor, width: pickerW, height: pickerH } = pickerLayout;
    const belowTop = anchor.top + anchor.height + PICKER_GAP_PX;
    const belowRoom = canvasH - PICKER_INSET_PX - belowTop;
    const aboveRoom = anchor.top - PICKER_GAP_PX - PICKER_INSET_PX;
    const below = pickerH <= belowRoom || (pickerH > aboveRoom && belowRoom >= aboveRoom);
    const maxHeight = Math.max(0, below ? belowRoom : aboveRoom);
    const drawnH = Math.min(pickerH, maxHeight);
    const left = Math.min(Math.max(PICKER_INSET_PX, anchor.left), Math.max(PICKER_INSET_PX, canvasW - pickerW - PICKER_INSET_PX));
    return { left, top: below ? belowTop : Math.max(PICKER_INSET_PX, anchor.top - PICKER_GAP_PX - drawnH), maxHeight };
  });

  /** Follow the desktop pointer as it moves and as the notes change under it. */
  $effect(() => {
    void elapsed;
    void layoutTick;
    const next = mobileShell || cursorAway || !canvas ? WELCOME_NO_HOVER : welcomeHoverAt(canvas, canvasW, cursorPoint);
    if (!welcomeHoverSame(next, untrack(() => hover))) hover = next;
  });

  /** Whether the part named `name` is under the desktop pointer. */
  function hovered(name: string): boolean {
    return hover.includes(name);
  }

  /** Whether a note shows its actions: under the pointer, or while its tag picker is open. */
  function actionsShown(key: QuickNotesSceneNote): boolean {
    return pickerOpen === key || (pickerOpen === null && hovered(`card:${key}`));
  }

  /** The palette slot of a note color the scene names, the default color when null. */
  function toneColor(tone: QuickNotesSceneTone | null): EventColor {
    return tone === null ? preferences.quickNotesDefaultColor : sceneColors[tone];
  }

  /** A blend of two colors, `amount` of the way from `from` to `to`. */
  function blend(from: string, to: string, amount: number): string {
    if (amount <= 0) return from;
    if (amount >= 1) return to;
    return `color-mix(in srgb, ${to} ${amount * 100}%, ${from})`;
  }

  /** A note's surface: its own color, fading to the Trip tag's as the note takes it, with the text flipping halfway. */
  function cardColors(card: QuickNotesSceneCard): { bg: string; text: string } {
    const from = getQuickNoteColor(toneColor(card.tone), theme.current);
    const progress = tripProgress(card.key);
    if (progress <= 0) return from;
    const to = getQuickNoteColor(sceneColors.trip, theme.current);
    return { bg: blend(from.bg, to.bg, progress), text: progress >= 0.5 ? to.text : from.text };
  }

  /** How far the Trip chip is selected over All, from 0 to 1, crossfading as the filter starts. */
  const tripSelected = $derived(filter.fade);

  /** A tag bar chip's colors for how selected it is: a faint tint unselected, a stronger one under the pointer, the full color selected. */
  function chipStyle(color: EventColor, selected: number, chipHovered: boolean): string {
    const colors = getQuickNoteColor(color, theme.current);
    const rest = chipHovered ? 50 : 30;
    const amount = rest + (100 - rest) * selected;
    const background = amount >= 100 ? colors.bg : `color-mix(in srgb, ${colors.bg} ${amount}%, transparent)`;
    return `background-color: ${background}; color: ${selected >= 0.5 ? colors.text : "var(--foreground)"};`;
  }

  /** The collapsed tags' count chip, tinted with the muted foreground as the tag bar draws it while no hidden tag is selected. */
  function moreChipStyle(chipHovered: boolean): string {
    return `background-color: color-mix(in srgb, var(--muted-foreground) ${chipHovered ? 50 : 30}%, transparent); color: var(--foreground);`;
  }

  /** A tag bar chip, sized as the tag bar draws it on each shell. */
  const CHIP_CLASS = mobileShell
    ? "relative flex min-h-12 max-w-40 shrink-0 items-center rounded-xl px-4 text-sm"
    : "relative flex h-7 max-w-32 shrink-0 items-center rounded-md px-2.5 text-xs";

  const composerColors = $derived.by(() => {
    const from = getQuickNoteColor(preferences.quickNotesDefaultColor, theme.current);
    const to = getQuickNoteColor(sceneColors.trip, theme.current);
    return { bg: blend(from.bg, to.bg, tripSelected), text: tripSelected >= 0.5 ? to.text : from.text };
  });

  function tagName(tag: QuickNotesSceneTag): string {
    return t(`vaultSetup.quickNotes.tags.${tag}`);
  }

  function noteText(key: QuickNotesSceneNote, part: "title" | "body"): string {
    return t(`vaultSetup.quickNotes.notes.${key}.${part}`);
  }

  /**
   * An element's place within `ancestor`, in the ancestor's logical pixels, so the scene's scale and any transform the two
   * share do not count.
   */
  function relBox(element: HTMLElement, ancestor: HTMLElement): Box {
    const rect = element.getBoundingClientRect();
    const origin = ancestor.getBoundingClientRect();
    const scale = ancestor.offsetWidth > 0 && origin.width > 0 ? origin.width / ancestor.offsetWidth : 1;
    return {
      top: (rect.top - origin.top) / scale,
      left: (rect.left - origin.left) / scale,
      width: rect.width / scale,
      height: rect.height / scale,
    };
  }
</script>

<!-- A card's tag badge, as the card draws it. -->
{#snippet badge(name: string)}
  <span class="inline-flex max-w-full truncate rounded-md bg-black/8 px-1.5 py-0.5 text-[0.65rem] font-medium dark:bg-white/10">{name}</span>
{/snippet}

<!-- A note card, as the card draws it; `live` marks the drawn one rather than its measured copy. -->
{#snippet noteCard(card: QuickNotesSceneCard, live: boolean)}
  {@const colors = live ? cardColors(card) : getQuickNoteColor(toneColor(card.tone), theme.current)}
  {@const trip = live ? tripProgress(card.key) : 0}
  {@const shown = live && !mobileShell && actionsShown(card.key)}
  <div class="relative overflow-hidden rounded-xl" style:background-color={colors.bg} style:color={colors.text} data-scene-hover={live ? `card:${card.key}` : undefined}>
    <span class={cn("block px-3.5 pt-3", mobileShell ? "pb-3" : "pb-2")}>
      {#if card.tag}
        <span class="flex max-w-full flex-wrap items-center gap-1 pb-1.5">{@render badge(tagName(card.tag))}</span>
      {:else if trip > 0}
        <!-- The Trip badge opens its row as the note takes the tag, by the height the masonry makes room for. -->
        <span
          class="block overflow-hidden"
          style:height={trip < 1 && cardMeasure ? `${cardMeasure.badgeRow * trip}px` : undefined}
          style:opacity={trip < 1 ? trip : undefined}
        >
          <span class="flex max-w-full flex-wrap items-center gap-1 pb-1.5">{@render badge(tripName)}</span>
        </span>
      {/if}
      <span class="mb-1.5 block wrap-break-word text-[0.92rem] font-semibold leading-snug">{noteText(card.key, "title")}</span>
      <span class="block text-[0.82rem] leading-relaxed">{noteText(card.key, "body")}</span>
    </span>
    {#if !mobileShell}
      <span class="flex min-h-9 items-center gap-0.5 px-2 pb-1.5" style:opacity={live && !shown ? 0 : undefined}>
        <span class={ACTION_CLASS}><Pin class="size-3.5" strokeWidth={1.5} /></span>
        <span class={ACTION_CLASS}><Palette class="size-4" strokeWidth={1.5} /></span>
        <span
          data-cursor={live ? `quick-notes-tag-${card.key}` : undefined}
          data-scene-hover={live ? `tag:${card.key}` : undefined}
          class={cn(ACTION_CLASS, live && (pickerOpen === card.key || (shown && hovered(`tag:${card.key}`))) && ACTION_ACTIVE_CLASS)}
        >
          <Tag class="size-4" strokeWidth={1.5} />
        </span>
        <span class={ACTION_CLASS}><Archive class="size-3.5" strokeWidth={1.5} /></span>
        <span class={ACTION_CLASS}><Trash2 class="size-3.5" strokeWidth={1.5} /></span>
      </span>
    {/if}
  </div>
{/snippet}

<!-- A tag picker's options and footer, as the picker draws them; `live` marks the drawn one rather than its measured copy. -->
{#snippet pickerContent()}
  <div class="surface-floating-body min-h-0 overflow-hidden">
    <span class={cn("menu-item", hovered("option:none") && "bg-accent")} data-scene-hover="option:none">
      <span class="size-3 shrink-0 rounded-full border border-dashed border-muted-foreground"></span>
      <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.none")}</span>
      <Check class="size-3.5 shrink-0" strokeWidth={1.5} />
    </span>
    {#each QUICK_NOTES_TAG_ORDER as tag (tag)}
      <span
        class={cn("menu-item", hovered(`option:${tag}`) && "bg-accent")}
        data-cursor={tag === "trip" ? "quick-notes-option-trip" : undefined}
        data-scene-hover={`option:${tag}`}
      >
        <span class="size-3 shrink-0 rounded-full border border-border" style:background-color={getQuickNoteColor(sceneColors[tag], theme.current).bg}></span>
        <span class="min-w-0 flex-1 truncate">{tagName(tag)}</span>
      </span>
    {/each}
  </div>
  <div class="surface-floating-body flex shrink-0 flex-col border-t border-border">
    <span class={cn("menu-item", hovered("new-tag") && "bg-accent")} data-scene-hover="new-tag">
      <span class="flex size-3 shrink-0 items-center justify-center"><Plus class="size-3.5" strokeWidth={1.5} /></span>
      <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.new")}</span>
    </span>
  </div>
{/snippet}

<div class="rounded-3xl bg-foreground/6 p-1.5">
  <div
    bind:this={canvas}
    class="relative flex flex-col overflow-hidden rounded-2xl bg-background text-foreground"
    style:width={`${canvasW}px`}
    style:height={`${canvasH}px`}
  >
    {#if mobileShell}
      <!-- The phone's top bar, with the Novel project's context switcher and Quick notes open. -->
      <WelcomePhoneTopBar height={TOP_BAR_H} identity="project" quickNotesOpen />
    {:else}
      <!-- The title bar's tabs, with the Novel project showing, and its utilities, with Quick notes open. -->
      <WelcomeTitleBar height={TOP_BAR_H} endPadding={TITLE_BAR_END_PX} tabs={TITLE_TABS} quickNotesOpen />
    {/if}

    <!-- The Quick notes panel, open over the app. -->
    <div
      data-cursor="quick-notes-panel"
      class="surface-dialog absolute flex flex-col overflow-hidden text-foreground"
      style:top={`${TOP_BAR_H + PANEL_TOP_GAP_PX}px`}
      style:left={`${PANEL_INSET_PX}px`}
      style:right={`${PANEL_INSET_PX}px`}
      style:bottom={`${PANEL_INSET_PX}px`}
      style:background-color="var(--cal-bg)"
    >
      <div class={mobileShell ? "shrink-0 px-2 pt-2 pb-2" : "shrink-0 px-3 pt-3 pb-1.5"} style:padding-inline={!mobileShell && canvasW >= 640 ? "1rem" : undefined}>
        <div class={mobileShell ? "flex min-w-0 flex-col gap-1" : "flex min-w-0 items-center gap-1"}>
          <!-- The tag bar: All, the tags, and Add tag. -->
          <div
            bind:this={tagRow}
            class={mobileShell ? "relative flex min-h-12 w-full min-w-0 items-center gap-1" : "relative flex min-w-0 flex-1 items-center gap-1"}
          >
            <span
              bind:this={allChip}
              class={CHIP_CLASS}
              style={chipStyle(preferences.quickNotesDefaultColor, 1 - tripSelected, hovered("chip:all"))}
              data-scene-hover="chip:all"
            >
              <span class="truncate">{t("quickNotes.collection.active")}</span>
            </span>
            {#each visibleTags as tag (tag)}
              {@const selected = tag === "trip" ? tripSelected : 0}
              <span
                class={CHIP_CLASS}
                style={chipStyle(sceneColors[tag], selected, selected <= 0 && hovered(`chip:${tag}`))}
                data-cursor={tag === "trip" ? "quick-notes-chip-trip" : undefined}
                data-scene-hover={`chip:${tag}`}
              >
                <span class="truncate">{tagName(tag)}</span>
                {#if mobileShell && tag === "trip"}<WelcomeTouch opacity={touches.filter} />{/if}
              </span>
            {/each}
            {#if hiddenTagCount > 0}
              <span class={cn(CHIP_CLASS, "gap-1")} style={moreChipStyle(hovered("chip:more"))} data-scene-hover="chip:more">
                <span class="tabular-nums">{moreLabel(hiddenTagCount)}</span>
                <ChevronDown class="size-3.5 shrink-0" strokeWidth={1.5} />
              </span>
            {/if}
            <span
              bind:this={addTag}
              class={cn(
                mobileShell ? "flex min-h-12 shrink-0 items-center gap-1 rounded-xl px-4 text-sm" : "flex h-7 shrink-0 items-center gap-1 rounded-md px-2 text-xs",
                hovered("add-tag") ? "bg-accent text-foreground" : "text-muted-foreground",
              )}
              data-scene-hover="add-tag"
            >
              <Plus class="size-3.5" strokeWidth={1.5} />
              {#if !mobileShell || canvasW >= MOBILE_ADD_TAG_LABEL_MIN_W}<span>{t("quickNotes.tag.add")}</span>{/if}
            </span>
            <!-- Chips measured at their natural width decide how many fit; the clip keeps the copies out of the layout. -->
            <div class="pointer-events-none invisible absolute inset-0 overflow-hidden" aria-hidden="true">
              <div bind:this={tagMeasurer} class="flex w-max gap-1">
                {#each QUICK_NOTES_TAG_ORDER as tag (tag)}
                  <span class={CHIP_CLASS} data-measure-tag><span class="truncate">{tagName(tag)}</span></span>
                {/each}
                <span class={cn(CHIP_CLASS, "gap-1")} data-measure-more>
                  <span class="tabular-nums">{moreLabel(QUICK_NOTES_TAG_ORDER.length)}</span>
                  <ChevronDown class="size-3.5 shrink-0" strokeWidth={1.5} />
                </span>
              </div>
            </div>
          </div>
          <div class={mobileShell ? "flex min-h-12 w-full min-w-0 items-center gap-1" : "ml-auto flex shrink-0 items-center gap-1"}>
            {#if mobileShell}<span class="min-w-0 flex-1 truncate px-2 text-base font-semibold">{t("quickNotes.title")}</span>{/if}
            {#each [{ name: "search", icon: Search }, { name: "archive", icon: Archive }, { name: "trash", icon: Trash2 }] as action (action.name)}
              <span class={cn(HEADER_BUTTON_CLASS, hovered(`header:${action.name}`) && "bg-accent/60")} data-scene-hover={`header:${action.name}`}>
                <action.icon class={HEADER_ICON_CLASS} strokeWidth={1.5} />
              </span>
            {/each}
            <span class={cn(HEADER_BUTTON_CLASS, hovered("header:close") && "bg-accent/60")} data-scene-hover="header:close">
              <X class={mobileShell ? "size-5" : "size-4"} strokeWidth={1.5} />
            </span>
          </div>
        </div>
      </div>

      <div class={mobileShell ? "relative min-h-0 flex-1 overflow-hidden px-3 pt-2 pb-4" : "relative min-h-0 flex-1 overflow-hidden px-3 pt-1.5 pb-3"} style:padding-inline={!mobileShell && canvasW >= 640 ? "1rem" : undefined}>
        <span
          class={cn("mx-auto mb-4 flex min-h-12 items-center rounded-xl px-4 text-left text-sm", mobileShell ? "w-full" : "w-2/3")}
          style:background-color={composerColors.bg}
          style:color={composerColors.text}
        >
          {t("quickNotes.takeNote")}
        </span>

        <!-- The notes in their masonry, each carried to its filtered place as the filter glides. -->
        <div class="relative w-full" bind:clientWidth={masonryWidth} style:height={`${masonryHeight}px`}>
          {#each QUICK_NOTES_CAST as card, index (card.key)}
            {@const place = cardPlace(card.key, index)}
            {@const kept = QUICK_NOTES_FILTERED_NOTES.includes(card.key)}
            {#if place && (kept || filter.fade < 1)}
              <div
                class="absolute top-0 left-0"
                style:width={`${place.width}px`}
                style:transform={`translate(${place.left}px, ${place.top}px)`}
                style:opacity={!kept && filter.fade > 0 ? 1 - filter.fade : undefined}
              >
                {@render noteCard(card, true)}
              </div>
            {/if}
          {/each}
        </div>

        <!-- The notes laid out at their card width as the scene starts, and a badge row, measured for the masonry. -->
        <div class="pointer-events-none invisible absolute top-0 left-0 overflow-hidden" style:width={`${cardWidth}px`}>
          <div bind:this={measurer} class="flex flex-col">
            {#each QUICK_NOTES_CAST as card (card.key)}
              <div>{@render noteCard(card, false)}</div>
            {/each}
          </div>
          <span bind:this={badgeSample} class="flex max-w-full flex-wrap items-center gap-1 pb-1.5">{@render badge(tripName)}</span>
        </div>
      </div>
    </div>

    {#if !mobileShell}
      <!-- The open note's tag picker. It stays laid out while hidden so its size is known. -->
      <div
        bind:this={picker}
        data-scene-cover
        class="surface-floating absolute z-40 flex w-floating-sm flex-col overflow-hidden"
        style:left={`${pickerPlace?.left ?? 0}px`}
        style:top={`${pickerPlace?.top ?? 0}px`}
        style:max-height={pickerPlace ? `${pickerPlace.maxHeight}px` : undefined}
        style:visibility={pickerPlace ? undefined : "hidden"}
      >
        {@render pickerContent()}
      </div>

      <WelcomeCursor script={QUICK_NOTES_CURSOR} {elapsed} width={canvasW} bind:position={cursorPoint} />
    {/if}
  </div>
</div>
