<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockColorSwatchStyle } from "$lib/notes/blocks/color";
  import { notesSlashMenuItemDomId } from "$lib/notes/editor/accessibility";
  import {
    clampNotesSlashActiveIndex,
    notesRecentSlashCommandKeys,
    recordRecentNotesSlashCommandKey,
    sectionNotesSlashCommandItems,
    type NotesSlashCommand,
    type NotesSlashCommandItem,
  } from "$lib/notes/editor/slash-commands";
  import type { NotesColor } from "$lib/notes/types";
  import { tick } from "svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { notesCaretMenuPosition } from "./caret-menu-position";
  import {
    notesSlashCommandHint as commandHint,
    notesSlashCommandIcon as commandIcon,
    notesSlashCommandLabel,
    notesSlashMenuCatalog,
    notesSlashMenuGroups,
  } from "./slash-menu-presentation";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";
  import Search from "@lucide/svelte/icons/search";
  import Check from "@lucide/svelte/icons/check";

  let {
    query = "",
    canSetColor = true,
    currentColor = "default",
    menuId = undefined,
    menuClass = "absolute left-[calc(var(--notes-depth)*1.25rem+1.75rem)] top-full mt-1",
    blockId = undefined,
    activeIndex = undefined,
    anchor = null,
    onClose = undefined,
    onSelect,
    onActiveIndexChange = undefined,
    onActiveCommandChange = undefined,
  }: {
    query?: string;
    canSetColor?: boolean;
    currentColor?: NotesColor;
    menuId?: string;
    menuClass?: string;
    blockId?: string;
    activeIndex?: number;
    anchor?: HTMLElement | null;
    onClose?: () => void;
    onSelect: (command: NotesSlashCommand) => void;
    onActiveIndexChange?: (index: number) => void;
    onActiveCommandChange?: (command: NotesSlashCommand | null, itemCount: number) => void;
  } = $props();

  const { t } = getLocalization();
  let scrollArea: HTMLDivElement | null = $state(null);
  let localQuery = $state("");
  let localActiveIndex = $state(0);
  let recentKeys = $state<readonly string[]>(notesRecentSlashCommandKeys());
  const searchQuery = $derived(anchor ? query : localQuery || query);
  /** A menu without a caret anchor is placed by CSS, so nudge it back inside a narrow screen. */
  const keepUnanchoredInViewport = (node: HTMLElement) => (anchor ? undefined : keepInViewport(node));
  const catalog = $derived(notesSlashMenuCatalog(canSetColor, t));
  const sections = $derived(sectionNotesSlashCommandItems(catalog, searchQuery, recentKeys));
  const groups = $derived(notesSlashMenuGroups(sections, t));
  const flatItems = $derived(groups.flatMap((group) => group.items));
  const hasResults = $derived(flatItems.length > 0);
  const flatIndexByKey = $derived(new Map(flatItems.map((item, index) => [item.key, index])));
  const safeActiveIndex = $derived(clampNotesSlashActiveIndex(activeIndex ?? localActiveIndex, flatItems.length));

  $effect(() => {
    void searchQuery;
    localActiveIndex = 0;
  });

  $effect(() => {
    const index = safeActiveIndex;
    const command = flatItems[index]?.command ?? null;
    if (activeIndex !== undefined && index !== activeIndex) onActiveIndexChange?.(index);
    onActiveCommandChange?.(command, flatItems.length);
    void tick().then(() => {
      const active = scrollArea?.querySelector<HTMLElement>('[data-active="true"]');
      if (!active || !scrollArea) return;
      const item = active.getBoundingClientRect();
      const viewport = scrollArea.getBoundingClientRect();
      if (item.top < viewport.top) scrollArea.scrollTop -= viewport.top - item.top;
      else if (item.bottom > viewport.bottom) scrollArea.scrollTop += item.bottom - viewport.bottom;
    });
  });

  /** Support keyboard search for insertion menus opened without an editing host. */
  function handleMenuKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Escape" && onClose) { event.preventDefault(); event.stopPropagation(); onClose(); }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault(); event.stopPropagation();
      activate((safeActiveIndex + (event.key === "ArrowDown" ? 1 : flatItems.length - 1)) % Math.max(1, flatItems.length));
    }
    if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement) && flatItems[safeActiveIndex]) {
      event.preventDefault(); event.stopPropagation(); selectCommand(flatItems[safeActiveIndex]);
    }
  }

  function activate(index: number): void {
    localActiveIndex = index;
    onActiveIndexChange?.(index);
  }

  function selectCommand(item: NotesSlashCommandItem): void {
    recentKeys = recordRecentNotesSlashCommandKey(item.key);
    onSelect(item.command);
  }

  function itemIndex(item: NotesSlashCommandItem): number {
    return flatIndexByKey.get(item.key) ?? 0;
  }

  function itemId(item: NotesSlashCommandItem): string | undefined {
    return blockId ? notesSlashMenuItemDomId(blockId, itemIndex(item)) : undefined;
  }

  function commandLabel(command: NotesSlashCommand): string {
    return notesSlashCommandLabel(command, t);
  }
</script>

<div
  use:notesCaretMenuPosition={{ editor: anchor, offset: 0, scrollArea: () => scrollArea, onOutside: onClose }}
  use:keepUnanchoredInViewport
  id={menuId}
  class={`${anchor ? "" : menuClass} surface-floating z-50 flex max-h-[min(22rem,70vh)] w-floating-lg flex-col overflow-hidden`}
  role="menu"
  onkeydown={handleMenuKey}
  aria-label={t("notes.slashMenu")}
  data-app-floating-surface
  tabindex="-1"
  onmousedown={(event) => { if (!(event.target instanceof HTMLInputElement)) event.preventDefault(); }}
>
  <div class="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2 text-muted-foreground">
    <Search class="size-4 shrink-0" aria-hidden="true" />
    {#if anchor}
      <span class="truncate">{query || t("notes.slashSearch")}</span>
    {:else}
      <input class="field-bare text-foreground" aria-label={t("notes.slashSearch")} placeholder={t("notes.slashSearch")} bind:value={localQuery} />
    {/if}
  </div>
  <div bind:this={scrollArea} class="slash-scroll min-h-0 overflow-y-auto overscroll-contain" use:scrollEdgeFadeAction>
  <div class="surface-floating-body">
  {#if hasResults}
    {#each groups as group}
      {@const items = group.items}
      {#if items.length > 0}
        <div class="menu-label">
          {group.label}
        </div>
        {#each items as item (item.key)}
          {@const Icon = commandIcon(item.command)}
          {@const index = itemIndex(item)}
          {@const active = index === safeActiveIndex}
          <button
            id={itemId(item)}
            class="menu-item"
            type="button"
            role={item.command.kind === "color" ? "menuitemradio" : "menuitem"}
            aria-checked={item.command.kind === "color"
              ? currentColor === item.command.color
              : undefined}
            data-active={active ? "true" : undefined}
            data-highlighted={active ? "" : undefined}
            onmousedown={(event) => event.preventDefault()}
            onpointermove={() => activate(index)}
            onfocus={() => activate(index)}
            onclick={() => selectCommand(item)}
          >
            {#if item.command.kind === "color"}
              <span
                class="notes-color-swatch"
                style={notesBlockColorSwatchStyle(item.command.color)}
                aria-hidden="true"
              >
                A
              </span>
            {:else if Icon}
              <Icon class="size-4 shrink-0" aria-hidden="true" />
            {/if}
            <span class="min-w-0 flex-1 truncate">{commandLabel(item.command)}</span>
            {#if commandHint(item.command)}<span class="text-panel-detail text-muted-foreground" aria-hidden="true">{commandHint(item.command)}</span>{/if}
            {#if item.command.kind === "color" && currentColor === item.command.color}
              <Check class="size-3.5 shrink-0" aria-hidden="true" />
            {/if}
          </button>
        {/each}
      {/if}
    {/each}
  {:else}
    <div class="px-2 py-1.5 text-muted-foreground" role="status">
      {t("notes.slashNoResults")}
    </div>
  {/if}
  </div>
  </div>
  {#if onClose}
    <button class="flex min-h-(--panel-row-height) w-full shrink-0 items-center justify-between border-t border-border px-3 py-2 text-left hover:bg-accent" type="button" onclick={onClose}>
      {t("notes.slashClose")}<kbd class="text-panel-detail text-muted-foreground">Esc</kbd>
    </button>
  {/if}
</div>

<style>
  .notes-color-swatch {
    display: inline-flex;
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--notes-color-swatch-border);
    border-radius: 0.25rem;
    background: var(--notes-color-swatch-bg);
    color: var(--notes-color-swatch-fg);
    font-size: calc(0.65rem * var(--type-scale));
    font-weight: 600;
    line-height: 1;
  }
</style>
