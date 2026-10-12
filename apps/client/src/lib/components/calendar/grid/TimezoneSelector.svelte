<script lang="ts">
  import {
    getLocalTimezone,
    getTimezoneInfo,
    searchTimezones,
  } from "$lib/calendar/utils";
  import type { TimezoneAbbrMode } from "$lib/calendar/utils";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import X from "@lucide/svelte/icons/x";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * Maximum number of timezones (including the device tz) the calendar can
   * show side by side. Beyond this the trigger gets crowded and the
   * popover hides the search.
   */
  const MAX_TIMEZONES = 5;

  function triggerLabel(info: ReturnType<typeof getTimezoneInfo>): string {
    if (abbrMode === "acronym") return info.acronym;
    if (abbrMode === "utc") return info.numericOffset;
    return info.columnAbbr;
  }

  let {
    timezones,
    timezoneCount = 1,
    abbrMode = "acronym" as TimezoneAbbrMode,
    onAdd,
    onRemove,
    onReorder,
    onAbbrModeChange,
  }: {
    timezones: string[];
    timezoneCount?: number;
    abbrMode?: TimezoneAbbrMode;
    onAdd: (tz: string) => void;
    onRemove: (index: number) => void;
    onReorder?: (from: number, to: number) => void;
    onAbbrModeChange?: (mode: TimezoneAbbrMode) => void;
  } = $props();

  const { t } = getLocalization();
  const ABBR_MODE_OPTIONS = $derived.by((): { mode: TimezoneAbbrMode; label: string }[] => [
    { mode: "acronym", label: t("calendar.timezone.acronym") },
    { mode: "utc", label: t("calendar.timezone.utcOnly") },
    { mode: "utc-fallback", label: t("calendar.timezone.utcFallback") },
  ]);

  // Width is fixed; height caps the scrollable result list. Both feed into
  // the viewport-aware position math so the popover never spills off-screen.
  const POPOVER_WIDTH = 360;
  const POPOVER_HEIGHT = 480;
  const VIEWPORT_MARGIN = 8;

  let open = $state(false);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let popoverEl: HTMLDivElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();
  let popoverPos = $state({ top: 0, left: 0 });
  let query = $state("");
  let highlightIndex = $state(0);
  let searchHydrated = $state(false);

  // Custom scrollbar metrics. The native scrollbar on `.tz-results` is hidden,
  // and a thumb sibling overlays the right edge of the list.
  let scrollTop = $state(0);
  let scrollHeight = $state(0);
  let clientHeight = $state(0);
  let trackEl: HTMLDivElement | undefined = $state();
  let isDraggingThumb = $state(false);
  const MIN_THUMB_HEIGHT = 24;
  const thumbHeight = $derived(
    scrollHeight > clientHeight && clientHeight > 0
      ? Math.max(MIN_THUMB_HEIGHT, (clientHeight / scrollHeight) * clientHeight)
      : 0,
  );
  const maxScrollTop = $derived(Math.max(0, scrollHeight - clientHeight));
  const thumbTop = $derived(
    maxScrollTop > 0
      ? (scrollTop / maxScrollTop) * (clientHeight - thumbHeight)
      : 0,
  );
  const showScrollbar = $derived(thumbHeight > 0);

  // Drag-and-drop reorder state. `dragFromIndex` is the row that started the
  // drag; `dragOverIndex` is the row currently under the pointer for the
  // insertion-line indicator. Both reset on dragend / drop.
  let dragFromIndex = $state<number | null>(null);
  let dragOverIndex = $state<number | null>(null);

  const localTimezone = getLocalTimezone();

  const EMPTY_TIMEZONE_RESULTS: string[] = [];
  const timezoneResults = $derived(
    open && searchHydrated
      ? searchTimezones(query, timezones)
      : EMPTY_TIMEZONE_RESULTS,
  );

  // Reset highlight whenever the query changes so the first row is selected.
  $effect(() => {
    void query;
    highlightIndex = 0;
  });

  function computePosition() {
    if (!triggerEl) return;
    const rect = triggerEl.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;
    let left = rect.left;
    if (left + POPOVER_WIDTH + VIEWPORT_MARGIN > viewportWidth) {
      left = Math.max(
        VIEWPORT_MARGIN,
        viewportWidth - POPOVER_WIDTH - VIEWPORT_MARGIN,
      );
    }
    let top = rect.bottom + 6;
    if (top + POPOVER_HEIGHT + VIEWPORT_MARGIN > viewportHeight) {
      top = Math.max(VIEWPORT_MARGIN, rect.top - POPOVER_HEIGHT - 6);
    }
    popoverPos = { top, left };
  }

  function toggleOpen() {
    if (open) {
      open = false;
      return;
    }
    query = "";
    highlightIndex = 0;
    computePosition();
    searchHydrated = true;
    open = true;
    requestAnimationFrame(() => inputEl?.focus());
  }

  function close(source: "keyboard" | "pointer" = "pointer") {
    open = false;
    if (source === "keyboard") requestAnimationFrame(() => triggerEl?.focus());
  }

  function handleAdd(tz: string, source: "keyboard" | "pointer") {
    // Defensive: search excludes active tzs so this should be unreachable,
    // but a stale highlight or rapid-fire Enter could re-add otherwise.
    if (timezones.includes(tz)) return;
    const willHitMax = timezones.length + 1 >= MAX_TIMEZONES;
    onAdd(tz);
    query = "";
    highlightIndex = 0;
    if (willHitMax) {
      close(source);
    } else {
      requestAnimationFrame(() => inputEl?.focus());
    }
  }

  function syncScrollMetrics() {
    if (!listEl) return;
    scrollTop = listEl.scrollTop;
    scrollHeight = listEl.scrollHeight;
    clientHeight = listEl.clientHeight;
  }

  function handleScrollbarPointerDown(e: PointerEvent) {
    if (!listEl || !trackEl) return;
    e.preventDefault();
    e.stopPropagation();
    const trackRect = trackEl.getBoundingClientRect();
    const localMaxScroll = maxScrollTop;
    const trackAvail = trackRect.height - thumbHeight;
    if (trackAvail <= 0 || localMaxScroll <= 0) return;

    // Click on the thumb: drag from current scroll position. Click on
    // empty track: jump the thumb's center to the click point and drag
    // from there. Both paths share the same move handler below.
    const clickY = e.clientY - trackRect.top;
    const onThumb = clickY >= thumbTop && clickY <= thumbTop + thumbHeight;
    if (!onThumb) {
      const desiredThumbTop = Math.max(
        0,
        Math.min(trackAvail, clickY - thumbHeight / 2),
      );
      listEl.scrollTop = (desiredThumbTop / trackAvail) * localMaxScroll;
    }
    const anchorScrollTop = listEl.scrollTop;
    const anchorClientY = e.clientY;
    isDraggingThumb = true;
    // Lock body selection while dragging so accidental pointer movement
    // over text in the result list doesn't initiate a selection.
    const prevUserSelect = document.body.style.userSelect;
    document.body.style.userSelect = "none";
    function onPointerMove(ev: PointerEvent) {
      if (!listEl) return;
      const dy = ev.clientY - anchorClientY;
      listEl.scrollTop = anchorScrollTop + (dy / trackAvail) * localMaxScroll;
    }
    function onPointerUp() {
      isDraggingThumb = false;
      document.body.style.userSelect = prevUserSelect;
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    }
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
  }

  function handleRowDragStart(e: DragEvent, i: number) {
    if (!e.dataTransfer) return;
    dragFromIndex = i;
    e.dataTransfer.effectAllowed = "move";
    // Firefox needs setData or the drag never starts.
    e.dataTransfer.setData("text/plain", String(i));
  }

  function handleRowDragOver(e: DragEvent, i: number) {
    if (dragFromIndex === null) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    dragOverIndex = i;
  }

  function handleRowDrop(e: DragEvent, i: number) {
    if (dragFromIndex === null) return;
    e.preventDefault();
    if (dragFromIndex !== i) onReorder?.(dragFromIndex, i);
    dragFromIndex = null;
    dragOverIndex = null;
  }

  function handleRowDragEnd() {
    dragFromIndex = null;
    dragOverIndex = null;
  }

  function scrollHighlightIntoView() {
    if (!listEl) return;
    const item = listEl.children[highlightIndex] as HTMLElement | undefined;
    item?.scrollIntoView({ block: "nearest" });
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      if (timezoneResults.length > 0) {
        highlightIndex = Math.min(timezoneResults.length - 1, highlightIndex + 1);
        scrollHighlightIntoView();
      }
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      if (timezoneResults.length > 0) {
        highlightIndex = Math.max(0, highlightIndex - 1);
        scrollHighlightIntoView();
      }
      return;
    }
    if (e.key === "Enter") {
      const candidate = timezoneResults[highlightIndex];
      if (candidate) {
        e.preventDefault();
        e.stopPropagation();
        handleAdd(candidate, "keyboard");
      }
    }
  }

  // Escape closes the whole portaled popover from any control within it.
  $effect(() => {
    if (!open) return;
    function onKeydown(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close("keyboard");
    }
    window.addEventListener("keydown", onKeydown, true);
    return () => window.removeEventListener("keydown", onKeydown, true);
  });

  $effect(() => {
    if (!open) return;
    function handleClickOutside(e: MouseEvent) {
      const target = e.target as Node;
      if (triggerEl?.contains(target)) return;
      if (popoverEl?.contains(target)) return;
      close();
    }
    function handleScroll(e: Event) {
      // Don't close on scrolls inside the popover's own result list.
      const target = e.target as Node | null;
      if (target && popoverEl?.contains(target)) return;
      close();
    }
    function handleResize() {
      computePosition();
    }
    window.addEventListener("mousedown", handleClickOutside, true);
    window.addEventListener("scroll", handleScroll, true);
    window.addEventListener("resize", handleResize);
    return () => {
      window.removeEventListener("mousedown", handleClickOutside, true);
      window.removeEventListener("scroll", handleScroll, true);
      window.removeEventListener("resize", handleResize);
    };
  });

  // Track the result list's scroll metrics for the custom scrollbar thumb.
  // Refresh on scroll, when the list resizes, and whenever the filtered set
  // changes, since adding or removing rows alters scrollHeight.
  $effect(() => {
    if (!open || !listEl) return;
    syncScrollMetrics();
    const el = listEl;
    const onScroll = () => {
      scrollTop = el.scrollTop;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    const resizeObserver = new ResizeObserver(syncScrollMetrics);
    resizeObserver.observe(el);
    return () => {
      el.removeEventListener("scroll", onScroll);
      resizeObserver.disconnect();
    };
  });

  $effect(() => {
    void timezoneResults;
    if (open && listEl) requestAnimationFrame(syncScrollMetrics);
  });
</script>

<!-- Trigger: subgrid keeps each label aligned under its column's hour ticks. -->
<button
  bind:this={triggerEl}
  type="button"
  class="timezone-trigger relative grid cursor-pointer self-stretch rounded text-[0.866667rem] transition-colors hover:bg-accent"
  style="color: var(--foreground); grid-column: span {timezoneCount}; grid-template-columns: subgrid;"
  onclick={toggleOpen}
  aria-haspopup="dialog"
  aria-expanded={open}
  aria-label={t("calendar.timezone.title")}
>
  {#each timezones as tz}
    {@const info = getTimezoneInfo(tz)}
    <span class="flex items-center justify-center">
      {triggerLabel(info)}
    </span>
  {/each}
</button>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    bind:this={popoverEl}
    use:portal
    class="tz-popover surface-floating fixed z-80 flex flex-col"
    style="top: {popoverPos.top}px; left: {popoverPos.left}px; width: {POPOVER_WIDTH}px; max-height: {POPOVER_HEIGHT}px; --foreground: var(--popover-foreground);"
    role="dialog"
    aria-label={t("calendar.timezone.title")}
    onwheel={(e) => e.stopPropagation()}
  >
    <div class="flex items-center justify-between gap-2 px-3 pt-3 pb-1">
      <div class="flex flex-col">
        <p class="font-semibold text-foreground">{t("calendar.timezone.title")}</p>
        <p class="text-panel-detail text-muted-foreground">{t("calendar.timezone.device", localTimezone)}</p>
      </div>
      <div
        class="flex items-center gap-0.5 rounded-floating-item border border-border p-0.5 text-panel-detail"
        role="group"
        aria-label={t("calendar.timezone.labelFormat")}
      >
        {#each ABBR_MODE_OPTIONS as option (option.mode)}
          {@const isActive = abbrMode === option.mode}
          <button
            type="button"
            class="rounded px-1.5 py-0.5 transition-colors {isActive
              ? 'bg-accent text-foreground'
              : 'text-muted-foreground hover:text-foreground'}"
            aria-pressed={isActive}
            onclick={(e: MouseEvent) => {
              e.stopPropagation();
              onAbbrModeChange?.(option.mode);
            }}
          >
            {option.label}
          </button>
        {/each}
      </div>
    </div>

    <!-- Active timezones -->
    <div class="flex flex-col gap-0.5 px-3">
      {#each timezones as tz, i (tz)}
        {@const info = getTimezoneInfo(tz)}
        {@const isDevice = tz === localTimezone}
        {@const isDragging = dragFromIndex === i}
        {@const isDragTarget = dragOverIndex === i && dragFromIndex !== null && dragFromIndex !== i}
        <!-- Whole row is the drag source. Chevrons and X are draggable=false
             so mousedown on them doesn't start a drag. -->
        <div
          class="group flex cursor-grab items-center justify-between gap-2 rounded-floating-item px-1 py-1 transition-colors active:cursor-grabbing {isDragTarget ? 'bg-accent' : ''} {isDragging ? 'opacity-40' : ''}"
          draggable="true"
          ondragstart={(e: DragEvent) => handleRowDragStart(e, i)}
          ondragover={(e: DragEvent) => handleRowDragOver(e, i)}
          ondragleave={() => { if (dragOverIndex === i) dragOverIndex = null; }}
          ondrop={(e: DragEvent) => handleRowDrop(e, i)}
          ondragend={handleRowDragEnd}
          role="listitem"
        >
          <span
            class="flex shrink-0 items-center text-muted-foreground"
            aria-hidden="true"
          >
            <GripVertical size={12} />
          </span>
          <div class="min-w-0 flex-1 truncate">
            <span class="inline-block w-18 font-medium tabular-nums text-foreground">{info.offsetUtc}</span>
            <span class="ml-1 text-foreground">{info.longName}</span>
            {#if isDevice}
              <span class="ml-1 text-muted-foreground">{t("calendar.timezone.deviceMarker")}</span>
            {/if}
          </div>
          <div class="flex shrink-0 items-center gap-1">
            <button
              type="button"
              draggable="false"
              onclick={(e: MouseEvent) => { e.stopPropagation(); onReorder?.(i, i - 1); }}
              disabled={i === 0}
              class="text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30"
              title={t("calendar.timezone.moveUp")}
              aria-label={t("calendar.timezone.moveUp")}
            >
              <ChevronUp size={12} />
            </button>
            <button
              type="button"
              draggable="false"
              onclick={(e: MouseEvent) => { e.stopPropagation(); onReorder?.(i, i + 1); }}
              disabled={i === timezones.length - 1}
              class="text-muted-foreground hover:text-foreground disabled:cursor-not-allowed disabled:opacity-30"
              title={t("calendar.timezone.moveDown")}
              aria-label={t("calendar.timezone.moveDown")}
            >
              <ChevronDown size={12} />
            </button>
            {#if !isDevice}
              <button
                type="button"
                draggable="false"
                onclick={(e: MouseEvent) => { e.stopPropagation(); onRemove(i); }}
                class="text-muted-foreground hover:text-foreground"
                title={t("calendar.timezone.remove")}
                aria-label={t("calendar.timezone.remove")}
              >
                <X size={12} />
              </button>
            {:else}
              <span class="w-3"></span>
            {/if}
          </div>
        </div>
      {/each}
    </div>

    <!-- Add timezone -->
    {#if timezones.length < MAX_TIMEZONES}
      <div class="px-3 pt-2">
        <input
          bind:this={inputEl}
          type="text"
          bind:value={query}
          placeholder={t("calendar.timezone.searchPlaceholder")}
          aria-label={t("calendar.timezone.searchPlaceholder")}
          class="field w-full text-foreground"
          onclick={(e: MouseEvent) => e.stopPropagation()}
          onkeydown={handleSearchKeydown}
        />
      </div>

      <div class="relative mt-2 flex min-h-0 flex-1 flex-col">
        <div
          bind:this={listEl}
          use:scrollEdgeFadeAction
          class="tz-results min-h-0 flex-1 overflow-y-auto px-2 pb-3 pr-3"
        >
          {#if timezoneResults.length === 0}
            <p class="px-2 py-3 text-center text-panel-detail text-muted-foreground">
              {t("calendar.timezone.noMatches")}
            </p>
          {:else}
            {#each timezoneResults as tz, idx}
              {@const info = getTimezoneInfo(tz)}
              <button
                type="button"
                class="menu-item flex-col items-stretch gap-0"
                data-highlighted={idx === highlightIndex || undefined}
                onmouseenter={() => { highlightIndex = idx; }}
                onclick={(e: MouseEvent) => { e.stopPropagation(); handleAdd(tz, "pointer"); }}
              >
                <div class="truncate text-foreground">
                  <span class="inline-block w-18 font-medium tabular-nums">{info.offsetUtc}</span>
                  <span class="ml-1">{info.longName}</span>
                </div>
                <div class="truncate text-panel-detail text-muted-foreground">
                  {info.city}{#if info.region}, {info.region}{/if}
                </div>
              </button>
            {/each}
          {/if}
        </div>
        <div
          bind:this={trackEl}
          class="tz-scrollbar-track absolute right-0.5 top-0 bottom-0 w-1.5 select-none"
          onpointerdown={handleScrollbarPointerDown}
          aria-hidden="true"
        >
          {#if showScrollbar}
            <div
              class="tz-scrollbar-thumb pointer-events-none absolute left-0 right-0 rounded-full {isDraggingThumb ? 'is-dragging' : ''}"
              style="top: {thumbTop}px; height: {thumbHeight}px;"
            ></div>
          {/if}
        </div>
      </div>
    {:else}
      <p class="px-3 pt-1 pb-3 text-panel-detail text-muted-foreground">{t("calendar.timezone.maximumTimezones", MAX_TIMEZONES)}</p>
    {/if}
  </div>
{/if}

<style>
  /* Native scrollbar hidden in favor of `.tz-scrollbar-thumb` overlay. */
  .tz-results {
    scrollbar-width: none;
  }
  .tz-results::-webkit-scrollbar {
    width: 0;
    height: 0;
    display: none;
  }
  .tz-scrollbar-thumb {
    background-color: var(--border);
    transition: background-color 120ms ease-out;
  }
  .tz-scrollbar-thumb:hover,
  .tz-scrollbar-thumb.is-dragging {
    background-color: var(--accent);
  }

  :global(html[data-focus-intent="keyboard"]) .timezone-trigger:focus {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--ring);
  }
</style>
