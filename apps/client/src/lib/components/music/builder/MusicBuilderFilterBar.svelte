<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Filter from "@lucide/svelte/icons/list-filter";
  import X from "@lucide/svelte/icons/x";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import type {
    MusicItemAvailability,
    MusicItemSort,
    MusicLibrarySourceKind,
    MusicSortDirection,
  } from "$lib/music/library/contracts";

  type MenuKind = "filters" | "sort";
  interface Option<T extends string | null> { value: T; label: string }

  let {
    sourceKind,
    availability,
    snoozed,
    sort,
    direction,
    resultCount,
    onChange,
  }: {
    sourceKind: MusicLibrarySourceKind | null;
    availability: MusicItemAvailability | null;
    snoozed: boolean | null;
    sort: MusicItemSort;
    direction: MusicSortDirection;
    resultCount: number;
    onChange: (patch: {
      sourceKind?: MusicLibrarySourceKind | null;
      availability?: MusicItemAvailability | null;
      snoozed?: boolean | null;
      sort?: MusicItemSort;
      direction?: MusicSortDirection;
    }) => void;
  } = $props();

  const { t } = getLocalization();
  let openMenu = $state<MenuKind | null>(null);
  let menuTop = $state(0);
  let menuLeft = $state(0);
  let menuAnchor: HTMLElement | null = null;
  let menuNode: HTMLElement | null = null;
  const filterCount = $derived(Number(sourceKind !== null) + Number(availability !== null) + Number(snoozed !== null));
  const snoozeValue = $derived<"snoozed" | "active" | null>(snoozed === null ? null : snoozed ? "snoozed" : "active");
  const sourceOptions = $derived<Option<MusicLibrarySourceKind | null>[]>([
    { value: null, label: t("music.builder.allSources") },
    { value: "local-file", label: t("music.builder.local") },
    { value: "youtube-video", label: t("music.builder.youtube") },
  ]);
  const availabilityOptions = $derived<Option<MusicItemAvailability | null>[]>([
    { value: null, label: t("music.builder.allAvailability") },
    { value: "available", label: t("music.builder.available") },
    { value: "missing", label: t("music.builder.missing") },
    { value: "unavailable", label: t("music.builder.unavailable") },
    { value: "ambiguous", label: t("music.builder.ambiguous") },
    { value: "unknown", label: t("music.builder.unknownAvailability") },
  ]);
  const snoozeOptions = $derived<Option<"snoozed" | "active" | null>[]>([
    { value: null, label: t("music.builder.allSnoozeStates") },
    { value: "active", label: t("music.builder.notSnoozed") },
    { value: "snoozed", label: t("music.builder.snoozed") },
  ]);
  const sortOptions = $derived<Option<MusicItemSort>[]>([
    { value: "title", label: t("music.builder.sortTitle") },
    { value: "artist", label: t("music.builder.sortArtist") },
    { value: "album", label: t("music.builder.sortAlbum") },
    { value: "added-to-playlist", label: t("music.builder.sortAddedToPlaylist") },
    { value: "last-played-at", label: t("music.builder.sortLastPlayed") },
    { value: "play-count", label: t("music.builder.sortPlayCount") },
  ]);

  function closeMenu(restoreFocus: boolean): void {
    const anchor = menuAnchor;
    openMenu = null;
    menuAnchor = null;
    if (restoreFocus && anchor?.isConnected) queueMicrotask(() => anchor.focus());
  }

  function toggleMenu(kind: MenuKind, anchor: HTMLElement): void {
    if (openMenu === kind) { closeMenu(true); return; }
    const bounds = anchor.getBoundingClientRect();
    menuTop = Math.max(4, Math.min(bounds.bottom + 5, window.innerHeight - 340));
    menuLeft = Math.max(4, Math.min(bounds.left, window.innerWidth - FLOATING_WIDTH.sm - 4));
    menuAnchor = anchor;
    openMenu = kind;
  }

  function menuAction(node: HTMLElement): { destroy: () => void } {
    menuNode = node;
    const items = (): HTMLButtonElement[] => [...node.querySelectorAll<HTMLButtonElement>("[role='menuitem'], [role='menuitemradio']")];
    const handleKeydown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); closeMenu(true); return; }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      const options = items();
      if (options.length === 0) return;
      event.preventDefault();
      const current = options.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0
        : event.key === "End" ? options.length - 1
        : event.key === "ArrowDown" ? (current + 1 + options.length) % options.length
        : (current - 1 + options.length) % options.length;
      options[next]?.focus();
    };
    node.addEventListener("keydown", handleKeydown);
    queueMicrotask(() => items()[0]?.focus());
    return { destroy: () => { node.removeEventListener("keydown", handleKeydown); if (menuNode === node) menuNode = null; } };
  }

  function handleWindowPointerDown(event: PointerEvent): void {
    if (!openMenu || !(event.target instanceof Node)) return;
    if (menuNode?.contains(event.target) || menuAnchor?.contains(event.target)) return;
    closeMenu(false);
  }

  function selectOption(patch: () => void): void {
    patch();
    closeMenu(true);
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === "Escape" && openMenu) { event.stopPropagation(); closeMenu(true); } }} onpointerdown={handleWindowPointerDown} />

<div class="filter-root flex min-h-10 shrink-0 items-center gap-1.5 border-b border-border/45 px-2 py-1.5" style={`--filter-menu-top:${menuTop}px;--filter-menu-left:${menuLeft}px`} aria-label={t("music.builder.filters")}>
  <div class="relative shrink-0">
    <button type="button" aria-haspopup="menu" aria-expanded={openMenu === "filters"} class:active={filterCount > 0} class="filter-pill" onclick={(event) => toggleMenu("filters", event.currentTarget)}>
      <Filter size={12} strokeWidth={1.7} />{t("music.builder.filters")}{#if filterCount > 0}<span class="filter-count">{filterCount}</span>{/if}<ChevronDown size={11} />
    </button>
    {#if openMenu === "filters"}
      <div use:menuAction role="menu" class="filter-menu surface-floating fixed z-70 flex w-floating-sm flex-col overflow-hidden">
        <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
          <p class="menu-label">{t("music.builder.sources")}</p>
          {#each sourceOptions as option (option.value)}<button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={option.value === sourceKind} onclick={() => onChange({ sourceKind: option.value })}>{option.label}{#if option.value === sourceKind}<Check size={12} />{/if}</button>{/each}
          <p class="menu-label">{t("music.builder.availability")}</p>
          {#each availabilityOptions as option (option.value)}<button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={option.value === availability} onclick={() => onChange({ availability: option.value })}>{option.label}{#if option.value === availability}<Check size={12} />{/if}</button>{/each}
          <p class="menu-label">{t("music.builder.snoozed")}</p>
          {#each snoozeOptions as option (option.value)}<button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={option.value === snoozeValue} onclick={() => onChange({ snoozed: option.value === null ? null : option.value === "snoozed" })}>{option.label}{#if option.value === snoozeValue}<Check size={12} />{/if}</button>{/each}
        </div>
      </div>
    {/if}
  </div>

  {#if filterCount > 0}
    <button type="button" class="clear-button" onclick={() => onChange({ sourceKind: null, availability: null, snoozed: null })} aria-label={t("music.builder.clearFilters")}><X size={12} /></button>
  {/if}

  <div class="relative ml-auto shrink-0">
    <button type="button" aria-haspopup="menu" aria-expanded={openMenu === "sort"} class="filter-pill" onclick={(event) => toggleMenu("sort", event.currentTarget)}>
      {sortOptions.find((option) => option.value === sort)?.label ?? t("music.builder.sort")}{#if direction === "ascending"}<ArrowUp size={11} />{:else}<ArrowDown size={11} />{/if}<ChevronDown size={11} />
    </button>
    {#if openMenu === "sort"}
      <div use:menuAction role="menu" class="filter-menu surface-floating fixed z-70 flex w-floating-sm flex-col overflow-hidden">
        <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto">
          {#each sortOptions as option (option.value)}<button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={option.value === sort} onclick={() => selectOption(() => onChange({ sort: option.value }))}>{option.label}{#if option.value === sort}<Check size={12} />{/if}</button>{/each}
          <div role="separator" class="menu-separator"></div>
          <button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={direction === "ascending"} onclick={() => selectOption(() => onChange({ direction: "ascending" }))}>{t("music.builder.ascending")}{#if direction === "ascending"}<Check size={12} />{/if}</button>
          <button type="button" class="menu-item justify-between" role="menuitemradio" aria-checked={direction === "descending"} onclick={() => selectOption(() => onChange({ direction: "descending" }))}>{t("music.builder.descending")}{#if direction === "descending"}<Check size={12} />{/if}</button>
        </div>
      </div>
    {/if}
  </div>
  <span class="shrink-0 px-1 text-[0.64rem] tabular-nums text-muted-foreground">{t("music.builder.resultCount", resultCount)}</span>
</div>

<style>
  .filter-pill { display: inline-flex; height: 1.75rem; align-items: center; gap: 0.3rem; border: 1px solid color-mix(in srgb, var(--border) 75%, transparent); border-radius: 999px; background: color-mix(in srgb, var(--card) 78%, transparent); padding-inline: 0.6rem; color: var(--muted-foreground); font-size: calc(0.65rem * var(--type-scale)); white-space: nowrap; }
  .filter-pill:hover, .filter-pill.active { border-color: color-mix(in srgb, var(--primary) 35%, var(--border)); color: var(--foreground); }
  .filter-pill.active { background: color-mix(in srgb, var(--primary) 9%, var(--card)); }
  .filter-count { min-width: 1rem; border-radius: 999px; background: var(--secondary); padding-inline: 0.25rem; text-align: center; font-size: calc(0.55rem * var(--type-scale)); line-height: calc(1rem * var(--type-scale)); }
  .clear-button { display: grid; height: 1.75rem; width: 1.75rem; place-items: center; border-radius: 999px; color: var(--muted-foreground); }
  .clear-button:hover, .clear-button:focus-visible { background: var(--accent); color: var(--accent-foreground); outline: none; }
  .filter-menu { top: var(--filter-menu-top); left: var(--filter-menu-left); max-height: min(20rem, 65vh); }
</style>
