<script lang="ts">
  import { tick } from "svelte";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import type { EventColor } from "$lib/calendar/types";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { quickNoteChipClass, quickNoteTagVisibleCount, quickNoteViewShortcut } from "$lib/quick-notes/tags";
  import type { QuickNoteTag } from "$lib/quick-notes/types";
  import type { Theme } from "$lib/themes";
  import { createSortableDrag } from "$lib/utils/sortable-drag.svelte";
  import QuickNoteTagChip from "./QuickNoteTagChip.svelte";
  import QuickNoteTagManager from "./QuickNoteTagManager.svelte";
  import QuickNoteTagMenu from "./QuickNoteTagMenu.svelte";

  type TagOverflowComponent = typeof import("./QuickNoteTagOverflow.svelte").default;

  /**
   * The Quick notes view row: All, then each tag in its color, then the control that adds a tag. Tags that do not fit
   * collapse into a button that lists every tag, so the row never scrolls sideways. Chips drag along the row to reorder.
   */
  let {
    tags,
    allSelected,
    selectedTagId,
    defaultColor,
    theme,
    moving,
    onSelect,
    onCreate,
    onRename,
    onColor,
    onMove,
    onDelete,
    onDefaultColor,
    mobileLayout = false,
  }: {
    tags: readonly QuickNoteTag[];
    /** Whether All is the current view. */
    allSelected: boolean;
    /** The tag of the current view, or null when the view is not a tag. */
    selectedTagId: string | null;
    /** Color of new untagged notes, which All shows. */
    defaultColor: EventColor;
    theme: Theme;
    /** Whether a tag move is still saving, which holds back the next one. */
    moving: boolean;
    onSelect: (tagId: string | null) => void;
    onCreate: (name: string, color: EventColor) => Promise<QuickNoteTag>;
    /** Rejects with a message to show when the name cannot be saved. */
    onRename: (tag: QuickNoteTag, name: string) => Promise<void>;
    onColor: (tag: QuickNoteTag, color: EventColor) => void;
    /** Moves the tag to `targetIndex` in the full tag order. */
    onMove: (id: string, targetIndex: number) => void | Promise<void>;
    onDelete: (tag: QuickNoteTag) => void;
    onDefaultColor: (color: EventColor) => void;
    mobileLayout?: boolean;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  let row = $state<HTMLDivElement | null>(null);
  let allChip = $state<HTMLButtonElement | null>(null);
  let creator = $state<HTMLDivElement | null>(null);
  let measurer = $state<HTMLDivElement | null>(null);
  let moreButton = $state<HTMLButtonElement | null>(null);
  let visibleCount = $state(Number.POSITIVE_INFINITY);
  let allMenuOpen = $state(false);
  let overflowOpen = $state(false);
  // The full tag list is needed only when tags overflow the row, so it stays out of the resident panel bundle.
  let TagOverflow = $state<TagOverflowComponent | null>(null);
  let overflowLoad: Promise<void> | null = null;

  const visibleTags = $derived(tags.slice(0, visibleCount));
  const hiddenCount = $derived(tags.length - visibleTags.length);
  const selectedHidden = $derived(selectedTagId !== null && !visibleTags.some((tag) => tag.id === selectedTagId));
  const selectedTag = $derived(tags.find((tag) => tag.id === selectedTagId) ?? null);
  const allColors = $derived(getQuickNoteColor(defaultColor, theme));
  const usedColors = $derived(tags.map((tag) => tag.color));
  const moreColors = $derived(selectedHidden && selectedTag
    ? getQuickNoteColor(selectedTag.color, theme)
    : { bg: "var(--muted-foreground)", text: "var(--foreground)" });

  const drag = createSortableDrag({
    getIds: () => visibleTags.map((tag) => tag.id),
    canStart: () => !moving,
    commit: (id, targetIndex) => onMove(id, targetIndex),
    touchHold: true,
    axis: "x",
  });

  function tagTitle(tag: QuickNoteTag, index: number): string {
    const shortcut = quickNoteViewShortcut(index + 1);
    return shortcut ? `${tag.name} (${t("calendar.toolbar.shortcutKey", shortcut)})` : tag.name;
  }

  function moreLabel(count: number): string {
    return formatNumber(localization.locale, count, { signDisplay: "always" });
  }

  /** Fits the leading tags beside All and the add control, leaving room for the overflow button when some do not fit. */
  function measure(): void {
    if (!row || !measurer) return;
    const gap = Number.parseFloat(getComputedStyle(row).columnGap) || 0;
    const widths = [...measurer.querySelectorAll<HTMLElement>("[data-measure-tag]")].map((element) => element.offsetWidth);
    const moreWidth = measurer.querySelector<HTMLElement>("[data-measure-more]")?.offsetWidth ?? 0;
    const fixed = (allChip?.offsetWidth ?? 0) + gap + (creator?.offsetWidth ?? 0) + gap;
    visibleCount = quickNoteTagVisibleCount(widths, row.clientWidth - fixed, gap, moreWidth);
  }

  function closeAllMenu(restoreFocus: boolean): void {
    allMenuOpen = false;
    if (restoreFocus) void tick().then(() => allChip?.focus());
  }

  function loadOverflow(): Promise<void> {
    if (TagOverflow) return Promise.resolve();
    overflowLoad ??= import("./QuickNoteTagOverflow.svelte")
      .then((module) => { TagOverflow = module.default; })
      .finally(() => { overflowLoad = null; });
    return overflowLoad;
  }

  async function toggleOverflow(): Promise<void> {
    if (overflowOpen) {
      overflowOpen = false;
      return;
    }
    try {
      await loadOverflow();
    } catch (error: unknown) {
      console.warn("Quick notes tag list failed to load", error);
      return;
    }
    if (hiddenCount > 0) overflowOpen = true;
  }

  function closeOverflow(restoreFocus: boolean): void {
    overflowOpen = false;
    if (restoreFocus) void tick().then(() => moreButton?.focus());
  }

  $effect(() => {
    if (!row || !measurer) return;
    const observer = new ResizeObserver(measure);
    for (const element of [row, measurer, allChip, creator]) if (element) observer.observe(element);
    return () => observer.disconnect();
  });

  $effect(() => {
    void tags;
    void mobileLayout;
    void tick().then(measure);
  });

  $effect(() => {
    if (hiddenCount === 0) overflowOpen = false;
  });

  $effect(() => () => drag.reset());
</script>

<div
  bind:this={row}
  class={mobileLayout ? "relative flex min-h-12 w-full min-w-0 items-center gap-1" : "relative flex min-w-0 flex-1 items-center gap-1"}
  role="group"
  aria-label={t("quickNotes.title")}
>
  <!-- Like a tag chip, a selected All opens its menu on click, here to choose the color new notes start with. -->
  <button
    bind:this={allChip}
    type="button"
    aria-pressed={allSelected}
    aria-haspopup={allSelected ? "menu" : undefined}
    aria-expanded={allSelected ? allMenuOpen : undefined}
    class={quickNoteChipClass(mobileLayout, allSelected)}
    style="--tag-bg: {allColors.bg}; --tag-fg: {allColors.text};"
    title={`${t("quickNotes.collection.active")} (${t("calendar.toolbar.shortcutKey", "1")})`}
    onclick={() => { if (allSelected) allMenuOpen = true; else onSelect(null); }}
    oncontextmenu={(event) => { event.preventDefault(); allMenuOpen = true; }}
  ><span class="truncate">{t("quickNotes.collection.active")}</span></button>
  {#each visibleTags as tag, index (tag.id)}
    <div
      class="flex shrink-0"
      {@attach drag.item(tag.id)}
    >
      <QuickNoteTagChip
        {tag}
        selected={selectedTagId === tag.id}
        title={tagTitle(tag, index)}
        {theme}
        {mobileLayout}
        move={{
          canMoveBack: !moving && index > 0,
          canMoveForward: !moving && index < tags.length - 1,
          onMove: (step) => void onMove(tag.id, index + step),
        }}
        onSelect={() => onSelect(tag.id)}
        onRename={(name) => onRename(tag, name)}
        onColor={(color) => onColor(tag, color)}
        onDelete={() => onDelete(tag)}
        onPointerDown={(event) => drag.pointerDown(event, tag.id)}
      />
    </div>
  {/each}
  {#if hiddenCount > 0}
    <button
      bind:this={moreButton}
      type="button"
      class="{quickNoteChipClass(mobileLayout, selectedHidden)} gap-1"
      style="--tag-bg: {moreColors.bg}; --tag-fg: {moreColors.text};"
      aria-label={t("quickNotes.tag.hiddenCount", hiddenCount)}
      title={t("quickNotes.tag.hiddenCount", hiddenCount)}
      aria-haspopup="dialog"
      aria-expanded={overflowOpen}
      onclick={() => void toggleOverflow()}
    >
      <span class="tabular-nums">{moreLabel(hiddenCount)}</span>
      <ChevronDown class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
    </button>
  {/if}
  <div bind:this={creator} class="flex shrink-0">
    <QuickNoteTagManager tagCount={tags.length} {usedColors} {defaultColor} {theme} {onCreate} {mobileLayout} />
  </div>

  <!-- Chips measured at their natural width decide how many fit; the clip keeps the copies out of the layout. -->
  <div class="pointer-events-none invisible absolute inset-0 overflow-hidden" aria-hidden="true" inert>
    <div bind:this={measurer} class="flex w-max gap-1">
      {#each tags as tag (tag.id)}
        <span class={quickNoteChipClass(mobileLayout, false)} data-measure-tag><span class="truncate">{tag.name}</span></span>
      {/each}
      <span class="{quickNoteChipClass(mobileLayout, false)} gap-1" data-measure-more>
        <span class="tabular-nums">{moreLabel(tags.length)}</span>
        <ChevronDown class="size-3.5 shrink-0" strokeWidth={1.5} />
      </span>
    </div>
  </div>
</div>

{#if allMenuOpen}
  <QuickNoteTagMenu
    anchor={allChip}
    label={t("quickNotes.tag.defaultColor")}
    colorLabel={t("quickNotes.tag.defaultColor")}
    color={defaultColor}
    {theme}
    {mobileLayout}
    onColor={onDefaultColor}
    onClose={closeAllMenu}
  />
{/if}

{#if overflowOpen && TagOverflow}
  <TagOverflow
    anchor={moreButton}
    {tags}
    {selectedTagId}
    {theme}
    {mobileLayout}
    canMove={!moving}
    onSelect={(tagId) => { closeOverflow(true); onSelect(tagId); }}
    {onRename}
    {onColor}
    {onMove}
    onDelete={(tag) => { closeOverflow(false); onDelete(tag); }}
    onClose={closeOverflow}
  />
{/if}
