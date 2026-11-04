<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesBlockColorSwatchStyle } from "$lib/notes/blocks/color";
  import type { NotesHeadingBlockType } from "$lib/notes/blocks/factory";
  import type { NotesInsertableBlockType } from "$lib/notes/blocks/insertion";
  import { notesSlashMenuItemDomId } from "$lib/notes/editor/accessibility";
  import {
    clampNotesSlashActiveIndex,
    notesSlashCommandItems,
    notesRecentSlashCommandKeys,
    recordRecentNotesSlashCommandKey,
    sectionNotesSlashCommandItems,
    type NotesSlashAction,
    type NotesSlashCommand,
    type NotesSlashCommandItem,
  } from "$lib/notes/editor/slash-commands";
  import type { NotesColor } from "$lib/notes/types";
  import { tick, type Component } from "svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { notesCaretMenuPosition } from "./caret-menu-position";
  import { keepInViewport } from "$lib/utils/keep-in-viewport";
  import Search from "@lucide/svelte/icons/search";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import Check from "@lucide/svelte/icons/check";
  import Code from "@lucide/svelte/icons/code";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Copy from "@lucide/svelte/icons/copy";
  import Database from "@lucide/svelte/icons/database";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FileIcon from "@lucide/svelte/icons/file";
  import FileText from "@lucide/svelte/icons/file-text";
  import Heading1 from "@lucide/svelte/icons/heading-1";
  import Heading2 from "@lucide/svelte/icons/heading-2";
  import Heading3 from "@lucide/svelte/icons/heading-3";
  import Heading4 from "@lucide/svelte/icons/heading-4";
  import Heading5 from "@lucide/svelte/icons/heading-5";
  import Heading6 from "@lucide/svelte/icons/heading-6";
  import ImageIcon from "@lucide/svelte/icons/image";
  import LinkIcon from "@lucide/svelte/icons/link";
  import List from "@lucide/svelte/icons/list";
  import ListCollapse from "@lucide/svelte/icons/list-collapse";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import MessageSquareWarning from "@lucide/svelte/icons/message-square-warning";
  import Minus from "@lucide/svelte/icons/minus";
  import MousePointerClick from "@lucide/svelte/icons/mouse-pointer-click";
  import Music from "@lucide/svelte/icons/eighth-notes";
  import Pilcrow from "@lucide/svelte/icons/pilcrow";
  import Quote from "@lucide/svelte/icons/quote";
  import Route from "@lucide/svelte/icons/route";
  import Sigma from "@lucide/svelte/icons/sigma";
  import SquareCheck from "@lucide/svelte/icons/square-check";
  import Table2 from "@lucide/svelte/icons/table-2";
  import Trash2 from "@lucide/svelte/icons/trash";
  import Video from "@lucide/svelte/icons/video";

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
  const catalog = $derived(notesSlashCommandItems({ canSetColor }).map((item) => ({
    ...item,
    searchText: `${item.searchText} ${commandLabel(item.command).normalize("NFD").replace(/\p{M}/gu, "").toLowerCase()}`,
  })));
  const sections = $derived(sectionNotesSlashCommandItems(catalog, searchQuery, recentKeys));
  const groups = $derived([
    { label: t("notes.slashRecent"), items: sections.recent },
    { label: t("notes.slashBlocks"), items: sections.blocks.filter((item) => blockGroup(item.command) === "basic") },
    { label: t("notes.slashMedia"), items: sections.blocks.filter((item) => blockGroup(item.command) === "media") },
    { label: t("notes.slashAdvanced"), items: sections.blocks.filter((item) => blockGroup(item.command) === "advanced") },
    { label: t("notes.slashActions"), items: sections.actions },
    { label: t("notes.slashColors"), items: sections.colors },
  ]);
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

  /** Group supported content types for menu browsing. */
  function blockGroup(command: NotesSlashCommand): "basic" | "media" | "advanced" {
    if (command.kind === "toggle_heading") return "advanced";
    if (command.kind !== "block") return "basic";
    switch (command.blockType) {
      case "image": case "video": case "audio": case "file": case "pdf": case "bookmark": case "link_preview": case "embed":
        return "media";
      case "child_database": case "breadcrumb": case "table_of_contents": case "column_list": case "table": case "tab": case "template": case "button": case "equation": case "code":
        return "advanced";
      default: return "basic";
    }
  }

  /** Show familiar Markdown cues beside basic content types. */
  function commandHint(command: NotesSlashCommand): string {
    if (command.kind !== "block") return "";
    switch (command.blockType) {
      case "heading_1": return "#";
      case "heading_2": return "##";
      case "heading_3": return "###";
      case "heading_4": return "####";
      case "heading_5": return "#####";
      case "heading_6": return "######";
      case "bulleted_list_item": return "-";
      case "numbered_list_item": return "1.";
      case "to_do": return "[]";
      case "toggle": return ">";
      default: return "";
    }
  }

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
    switch (command.kind) {
      case "block":
        return blockLabel(command.blockType);
      case "toggle_heading":
        return toggleHeadingLabel(command.headingType);
      case "action":
        return actionLabel(command.action);
      case "color":
        return colorLabel(command.color);
    }
  }

  function blockLabel(type: NotesInsertableBlockType): string {
    switch (type) {
      case "paragraph":
        return t("notes.blockType.paragraph");
      case "heading_1":
        return t("notes.blockType.heading1");
      case "heading_2":
        return t("notes.blockType.heading2");
      case "heading_3":
        return t("notes.blockType.heading3");
      case "heading_4":
        return t("notes.blockType.heading4");
      case "heading_5":
        return t("notes.blockType.heading5");
      case "heading_6":
        return t("notes.blockType.heading6");
      case "bulleted_list_item":
        return t("notes.blockType.bullet");
      case "numbered_list_item":
        return t("notes.blockType.numbered");
      case "to_do":
        return t("notes.blockType.todo");
      case "toggle":
        return t("notes.blockType.toggle");
      case "callout":
        return t("notes.blockType.callout");
      case "quote":
        return t("notes.blockType.quote");
      case "child_page":
        return t("notes.blockType.childPage");
      case "child_database":
        return t("notes.blockType.childDatabase");
      case "breadcrumb":
        return t("notes.blockType.breadcrumb");
      case "table_of_contents":
        return t("notes.blockType.tableOfContents");
      case "column_list":
        return t("notes.blockType.columns");
      case "table":
        return t("notes.blockType.table");
      case "tab":
        return t("notes.blockType.tab");
      case "image":
        return t("notes.blockType.image");
      case "video":
        return t("notes.blockType.video");
      case "audio":
        return t("notes.blockType.audio");
      case "file":
        return t("notes.blockType.file");
      case "pdf":
        return t("notes.blockType.pdf");
      case "bookmark":
        return t("notes.blockType.bookmark");
      case "link_preview":
        return t("notes.blockType.linkPreview");
      case "template":
        return t("notes.blockType.template");
      case "button":
        return t("notes.blockType.button");
      case "embed":
        return t("notes.blockType.embed");
      case "equation":
        return t("notes.blockType.equation");
      case "divider":
        return t("notes.blockType.divider");
      case "code":
        return t("notes.blockType.code");
    }
  }

  function toggleHeadingLabel(type: NotesHeadingBlockType): string {
    switch (type) {
      case "heading_1":
        return t("notes.blockType.toggleHeading1");
      case "heading_2":
        return t("notes.blockType.toggleHeading2");
      case "heading_3":
        return t("notes.blockType.toggleHeading3");
      case "heading_4":
        return t("notes.blockType.toggleHeading4");
      case "heading_5":
        return t("notes.blockType.toggleHeading5");
      case "heading_6":
        return t("notes.blockType.toggleHeading6");
    }
  }

  function actionLabel(action: NotesSlashAction): string {
    switch (action) {
      case "copy_link":
        return t("notes.copyBlockLink");
      case "duplicate":
        return t("notes.duplicateBlock");
      case "move_up":
        return t("notes.moveBlockUp");
      case "move_down":
        return t("notes.moveBlockDown");
      case "delete":
        return t("notes.deleteBlock");
    }
  }

  function colorLabel(color: NotesColor): string {
    switch (color) {
      case "default":
        return t("notes.blockColor.default");
      case "gray":
        return t("notes.blockColor.gray");
      case "brown":
        return t("notes.blockColor.brown");
      case "orange":
        return t("notes.blockColor.orange");
      case "yellow":
        return t("notes.blockColor.yellow");
      case "green":
        return t("notes.blockColor.green");
      case "blue":
        return t("notes.blockColor.blue");
      case "purple":
        return t("notes.blockColor.purple");
      case "pink":
        return t("notes.blockColor.pink");
      case "red":
        return t("notes.blockColor.red");
      case "gray_background":
        return t("notes.blockColor.grayBackground");
      case "brown_background":
        return t("notes.blockColor.brownBackground");
      case "orange_background":
        return t("notes.blockColor.orangeBackground");
      case "yellow_background":
        return t("notes.blockColor.yellowBackground");
      case "green_background":
        return t("notes.blockColor.greenBackground");
      case "blue_background":
        return t("notes.blockColor.blueBackground");
      case "purple_background":
        return t("notes.blockColor.purpleBackground");
      case "pink_background":
        return t("notes.blockColor.pinkBackground");
      case "red_background":
        return t("notes.blockColor.redBackground");
    }
  }

  function commandIcon(command: NotesSlashCommand): Component | null {
    switch (command.kind) {
      case "block":
        return blockIcon(command.blockType);
      case "toggle_heading":
        return toggleHeadingIcon(command.headingType);
      case "action":
        return actionIcon(command.action);
      case "color":
        return null;
    }
  }

  function blockIcon(type: NotesInsertableBlockType): Component {
    switch (type) {
      case "paragraph":
        return Pilcrow;
      case "heading_1":
        return Heading1;
      case "heading_2":
        return Heading2;
      case "heading_3":
        return Heading3;
      case "heading_4":
        return Heading4;
      case "heading_5":
        return Heading5;
      case "heading_6":
        return Heading6;
      case "bulleted_list_item":
        return List;
      case "numbered_list_item":
        return ListOrdered;
      case "to_do":
        return SquareCheck;
      case "toggle":
        return ListCollapse;
      case "callout":
        return MessageSquareWarning;
      case "quote":
        return Quote;
      case "child_page":
        return FileText;
      case "child_database":
        return Database;
      case "breadcrumb":
        return Route;
      case "table_of_contents":
        return ListTree;
      case "column_list":
        return Columns2;
      case "table":
        return Table2;
      case "tab":
        return Columns2;
      case "image":
        return ImageIcon;
      case "video":
        return Video;
      case "audio":
        return Music;
      case "file":
        return FileIcon;
      case "pdf":
        return FileText;
      case "bookmark":
        return Bookmark;
      case "link_preview":
        return LinkIcon;
      case "template":
        return FileText;
      case "button":
        return MousePointerClick;
      case "embed":
        return ExternalLink;
      case "equation":
        return Sigma;
      case "divider":
        return Minus;
      case "code":
        return Code;
    }
  }

  function toggleHeadingIcon(type: NotesHeadingBlockType): Component {
    switch (type) {
      case "heading_1":
        return Heading1;
      case "heading_2":
        return Heading2;
      case "heading_3":
        return Heading3;
      case "heading_4":
        return Heading4;
      case "heading_5":
        return Heading5;
      case "heading_6":
        return Heading6;
    }
  }

  function actionIcon(action: NotesSlashAction): Component {
    switch (action) {
      case "copy_link":
        return LinkIcon;
      case "duplicate":
        return Copy;
      case "move_up":
        return ArrowUp;
      case "move_down":
        return ArrowDown;
      case "delete":
        return Trash2;
    }
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
