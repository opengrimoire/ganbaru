<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesInsertableBlockType } from "$lib/notes/blocks/insertion";
  import {
    NOTES_TEXT_CONTEXT_SUBMENU_WIDTH,
    notesTextContextMenuPosition,
    notesTextContextSubmenuPosition,
    type NotesTextMenuPoint,
    type NotesTextMenuRect,
  } from "$lib/notes/editor/text-context-menu";
  import type { NotesRichTextAnnotationName } from "$lib/notes/rich-text/core";
  import type { NotesBlockType, NotesColor, NotesRichTextAnnotations } from "$lib/notes/types";
  import { dismissOnOutside } from "$lib/utils/dismiss-on-outside";
  import {
    SUBMENU_AIM_TOLERANCES,
    SUBMENU_CLOSE_DELAY_MS,
    SUBMENU_OPEN_DELAY_MS,
    isPointerAimingAtSubmenu,
    type MenuAimPoint,
  } from "$lib/utils/menu-aim";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import Bold from "@lucide/svelte/icons/bold";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import Code from "@lucide/svelte/icons/code";
  import Copy from "@lucide/svelte/icons/copy";
  import Heading1 from "@lucide/svelte/icons/heading-1";
  import Heading2 from "@lucide/svelte/icons/heading-2";
  import Heading3 from "@lucide/svelte/icons/heading-3";
  import Heading4 from "@lucide/svelte/icons/heading-4";
  import Heading5 from "@lucide/svelte/icons/heading-5";
  import Heading6 from "@lucide/svelte/icons/heading-6";
  import Italic from "@lucide/svelte/icons/italic";
  import LinkIcon from "@lucide/svelte/icons/link";
  import List from "@lucide/svelte/icons/list";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Minus from "@lucide/svelte/icons/minus";
  import Palette from "@lucide/svelte/icons/palette";
  import PencilLine from "@lucide/svelte/icons/pencil-line";
  import Pilcrow from "@lucide/svelte/icons/pilcrow";
  import Quote from "@lucide/svelte/icons/quote";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Sigma from "@lucide/svelte/icons/sigma";
  import SquareCheck from "@lucide/svelte/icons/square-check";
  import { onDestroy, tick } from "svelte";
  import NotesTextColorPalette from "./NotesTextColorPalette.svelte";

  type Submenu = "format" | "paragraph" | "insert";

  let {
    position,
    focusOnOpen,
    annotations,
    blockType,
    hasSelection,
    canFormatSelection,
    canSetCalloutBackground = false,
    calloutBackgroundColor,
    canOpenLink,
    onToggleAnnotation,
    onColorSelect,
    onCreateEquation,
    onCreateComment,
    onCreateSuggestion,
    onOpenLink,
    onCopyBlockLink,
    onConvert,
    onInsert,
    onCut,
    onCopy,
    onPaste,
    onPastePlainText,
    onClose,
  }: {
    position: NotesTextMenuPoint;
    focusOnOpen: boolean;
    annotations: NotesRichTextAnnotations;
    blockType: NotesBlockType;
    hasSelection: boolean;
    canFormatSelection: boolean;
    canSetCalloutBackground?: boolean;
    calloutBackgroundColor?: NotesColor;
    canOpenLink: boolean;
    onToggleAnnotation: (name: NotesRichTextAnnotationName) => void;
    onColorSelect: (color: NotesColor) => void;
    onCreateEquation: () => void;
    onCreateComment: () => void;
    onCreateSuggestion: () => void;
    onOpenLink: () => void;
    onCopyBlockLink: () => Promise<void> | void;
    onConvert: (type: NotesBlockType) => void;
    onInsert: (type: NotesInsertableBlockType) => void;
    onCut: () => Promise<void>;
    onCopy: () => Promise<void>;
    onPaste: () => Promise<void>;
    onPastePlainText: () => Promise<void>;
    onClose: (restoreFocus?: boolean) => void;
  } = $props();

  const { t } = getLocalization();
  const itemClass = "menu-item";
  const paragraphItems = [
    { type: "paragraph", labelKey: "notes.blockType.paragraph", icon: Pilcrow },
    { type: "heading_1", labelKey: "notes.blockType.heading1", icon: Heading1 },
    { type: "heading_2", labelKey: "notes.blockType.heading2", icon: Heading2 },
    { type: "heading_3", labelKey: "notes.blockType.heading3", icon: Heading3 },
    { type: "heading_4", labelKey: "notes.blockType.heading4", icon: Heading4 },
    { type: "heading_5", labelKey: "notes.blockType.heading5", icon: Heading5 },
    { type: "heading_6", labelKey: "notes.blockType.heading6", icon: Heading6 },
    { type: "bulleted_list_item", labelKey: "notes.blockType.bullet", icon: List },
    { type: "numbered_list_item", labelKey: "notes.blockType.numbered", icon: ListOrdered },
    { type: "to_do", labelKey: "notes.blockType.todo", icon: SquareCheck },
    { type: "quote", labelKey: "notes.blockType.quote", icon: Quote },
    { type: "code", labelKey: "notes.blockType.code", icon: Code },
  ] as const;
  const insertItems = [
    { type: "paragraph", labelKey: "notes.blockType.paragraph", icon: Pilcrow },
    { type: "bulleted_list_item", labelKey: "notes.blockType.bullet", icon: List },
    { type: "numbered_list_item", labelKey: "notes.blockType.numbered", icon: ListOrdered },
    { type: "to_do", labelKey: "notes.blockType.todo", icon: SquareCheck },
    { type: "code", labelKey: "notes.blockType.code", icon: Code },
    { type: "divider", labelKey: "notes.blockType.divider", icon: Minus },
  ] as const;

  let viewport = $state({
    width: typeof window === "undefined" ? 0 : window.innerWidth,
    height: typeof window === "undefined" ? 0 : window.innerHeight,
  });
  let rootElement = $state<HTMLDivElement | null>(null);
  let submenuElement = $state<HTMLDivElement | null>(null);
  let paletteElement = $state<HTMLDivElement | null>(null);
  let submenu = $state<Submenu | null>(null);
  let submenuAnchor = $state<NotesTextMenuRect | null>(null);
  let submenuPosition = $state<NotesTextMenuPoint | null>(null);
  let paletteAnchor = $state<NotesTextMenuRect | null>(null);
  let palettePosition = $state<NotesTextMenuPoint | null>(null);
  let actionError = $state<string | null>(null);
  const rootPosition = $derived(notesTextContextMenuPosition(
    position,
    viewport,
    rootElement?.offsetHeight,
  ));

  $effect(() => {
    if (!focusOnOpen) return;
    void position.x;
    void position.y;
    void tick().then(() => rootElement?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus());
  });

  $effect(() => {
    if (!submenu || !submenuAnchor || !submenuElement) return;
    submenuPosition = notesTextContextSubmenuPosition(
      submenuAnchor,
      { width: submenuElement.offsetWidth, height: submenuElement.offsetHeight },
      viewport,
    );
  });

  $effect(() => {
    if (!paletteAnchor || !paletteElement) return;
    palettePosition = notesTextContextSubmenuPosition(
      paletteAnchor,
      { width: paletteElement.offsetWidth, height: paletteElement.offsetHeight },
      viewport,
    );
  });

  function rectOf(element: HTMLElement): NotesTextMenuRect {
    const rect = element.getBoundingClientRect();
    return { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
  }

  function openSubmenu(next: Submenu, element: HTMLElement, shouldFocusFirst = false): void {
    cancelHover();
    submenu = next;
    submenuAnchor = rectOf(element);
    submenuPosition = null;
    paletteAnchor = null;
    palettePosition = null;
    if (shouldFocusFirst) {
      void tick().then(() => submenuElement?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus());
    }
  }

  function openPalette(element: HTMLElement, shouldFocusFirst = false): void {
    cancelHover();
    paletteAnchor = rectOf(element);
    palettePosition = null;
    if (shouldFocusFirst) {
      void tick().then(() => paletteElement?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus());
    }
  }

  function runAction(action: () => void | Promise<void>): void {
    actionError = null;
    try {
      void Promise.resolve(action()).then(() => onClose()).catch((error: unknown) => {
        console.error("Notes text context menu action failed", error);
        actionError = t("notes.contextMenuActionFailed");
      });
    } catch (error) {
      console.error("Notes text context menu action failed", error);
      actionError = t("notes.contextMenuActionFailed");
    }
  }

  function closeSubmenus(): void {
    submenu = null;
    submenuAnchor = null;
    paletteAnchor = null;
  }

  let pointer: MenuAimPoint | null = null;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;

  onDestroy(cancelHover);

  function trackPointer(event: PointerEvent): void {
    pointer = { x: event.clientX, y: event.clientY };
  }

  function cancelHover(): void {
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = null;
  }

  /** Whether the pointer currently rests inside an open flyout. */
  function pointerInside(element: HTMLElement | null): boolean {
    if (!element || !pointer) return false;
    const rect = element.getBoundingClientRect();
    return pointer.x >= rect.left && pointer.x <= rect.right && pointer.y >= rect.top && pointer.y <= rect.bottom;
  }

  /** Whether the pointer has moved from `origin` toward the open flyout that sits beside `anchor`. */
  function aimingAtFlyout(element: HTMLElement | null, anchor: NotesTextMenuRect | null, origin: MenuAimPoint | null): boolean {
    if (!element || !anchor || !pointer) return false;
    const rect = element.getBoundingClientRect();
    return isPointerAimingAtSubmenu({
      origin,
      point: pointer,
      submenu: { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom },
      side: rect.left >= anchor.right - 1 ? "right" : "left",
      ...SUBMENU_AIM_TOLERANCES,
    });
  }

  /**
   * Apply a hover change after `delay`, deferring it while the pointer keeps travelling toward the open flyout so a
   * diagonal path across sibling rows does not switch or close it, and dropping it once the pointer reaches the flyout.
   */
  function scheduleHover(action: () => void, delay: number, flyout: () => [HTMLElement | null, NotesTextMenuRect | null]): void {
    cancelHover();
    const origin = pointer;
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      const [element, anchor] = flyout();
      if (pointerInside(element)) return;
      if (aimingAtFlyout(element, anchor, origin)) {
        scheduleHover(action, delay, flyout);
        return;
      }
      action();
    }, delay);
  }

  const openSubmenuFlyout = (): [HTMLElement | null, NotesTextMenuRect | null] => [submenuElement, submenuAnchor];
  const openPaletteFlyout = (): [HTMLElement | null, NotesTextMenuRect | null] => [paletteElement, paletteAnchor];

  function hoverSubmenuRow(next: Submenu, element: HTMLElement): void {
    if (submenu === next) {
      cancelHover();
      return;
    }
    scheduleHover(() => openSubmenu(next, element), SUBMENU_OPEN_DELAY_MS, openSubmenuFlyout);
  }

  function hoverPlainRow(): void {
    if (!submenu) {
      cancelHover();
      return;
    }
    scheduleHover(closeSubmenus, SUBMENU_CLOSE_DELAY_MS, openSubmenuFlyout);
  }

  function hoverPaletteRow(element: HTMLElement): void {
    if (paletteAnchor) {
      cancelHover();
      return;
    }
    scheduleHover(() => openPalette(element), SUBMENU_OPEN_DELAY_MS, openPaletteFlyout);
  }

  function hoverPlainSubmenuRow(): void {
    if (!paletteAnchor) {
      cancelHover();
      return;
    }
    scheduleHover(() => { paletteAnchor = null; }, SUBMENU_CLOSE_DELAY_MS, openPaletteFlyout);
  }

  function updateViewport(): void {
    viewport = { width: window.innerWidth, height: window.innerHeight };
  }
</script>

<svelte:window onresize={updateViewport} onpointermove={trackPointer} />

<div class="contents" use:dismissOnOutside={{ onDismiss: (reason) => onClose(reason === "escape") }}>
<div
  bind:this={rootElement}
  class="surface-floating fixed z-50 flex flex-col overflow-hidden"
  style:left={`${rootPosition.left}px`}
  style:top={`${rootPosition.top}px`}
  style:width={`${rootPosition.width}px`}
  style:max-height={`${rootPosition.maxHeight}px`}
  style:visibility={rootElement ? "visible" : "hidden"}
  role="menu"
  aria-label={t("notes.textContextMenu")}
  tabindex="-1"
  data-app-floating-surface
  onmousedown={(event) => event.preventDefault()}
>
  <div class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
  <button class={itemClass} type="button" role="menuitem" disabled={!canOpenLink} onmouseenter={hoverPlainRow} onclick={() => runAction(onOpenLink)}>
    <LinkIcon class="size-4 shrink-0" aria-hidden="true" />
    <span class="flex-1">{t("notes.openLinkEditor")}</span>
  </button>
  <button class={itemClass} type="button" role="menuitem" onmouseenter={hoverPlainRow} onclick={() => runAction(onCopyBlockLink)}>
    <LinkIcon class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.copyBlockLink")}</span>
  </button>
  <div class="menu-separator" role="separator"></div>
  <button
    class={itemClass}
    type="button"
    role="menuitem"
    aria-haspopup="menu"
    aria-expanded={submenu === "format"}
    disabled={!canFormatSelection && !canSetCalloutBackground}
    onmouseenter={(event) => hoverSubmenuRow("format", event.currentTarget)}
    onclick={(event) => openSubmenu("format", event.currentTarget, event.detail === 0)}
  >
    <Palette class="size-4 shrink-0" aria-hidden="true" />
    <span class="flex-1">{t("notes.contextMenuFormat")}</span>
    <ChevronRight class="size-4 shrink-0" aria-hidden="true" />
  </button>
  <button
    class={itemClass}
    type="button"
    role="menuitem"
    aria-haspopup="menu"
    aria-expanded={submenu === "paragraph"}
    onmouseenter={(event) => hoverSubmenuRow("paragraph", event.currentTarget)}
    onclick={(event) => openSubmenu("paragraph", event.currentTarget, event.detail === 0)}
  >
    <Pilcrow class="size-4 shrink-0" aria-hidden="true" />
    <span class="flex-1">{t("notes.contextMenuParagraph")}</span>
    <ChevronRight class="size-4 shrink-0" aria-hidden="true" />
  </button>
  <button
    class={itemClass}
    type="button"
    role="menuitem"
    aria-haspopup="menu"
    aria-expanded={submenu === "insert"}
    onmouseenter={(event) => hoverSubmenuRow("insert", event.currentTarget)}
    onclick={(event) => openSubmenu("insert", event.currentTarget, event.detail === 0)}
  >
    <List class="size-4 shrink-0" aria-hidden="true" />
    <span class="flex-1">{t("notes.contextMenuInsert")}</span>
    <ChevronRight class="size-4 shrink-0" aria-hidden="true" />
  </button>
  <button class={itemClass} type="button" role="menuitem" disabled={!canFormatSelection} onmouseenter={hoverPlainRow} onclick={() => runAction(onCreateComment)}>
    <MessageSquare class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.inlineComment")}</span>
  </button>
  <button class={itemClass} type="button" role="menuitem" disabled={!canFormatSelection} onmouseenter={hoverPlainRow} onclick={() => runAction(onCreateSuggestion)}>
    <PencilLine class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.inlineSuggestion")}</span>
  </button>
  <div class="menu-separator" role="separator"></div>
  <button class={itemClass} type="button" role="menuitem" disabled={!hasSelection} onmouseenter={hoverPlainRow} onclick={() => runAction(onCut)}>
    <Scissors class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.cutSelection")}</span>
  </button>
  <button class={itemClass} type="button" role="menuitem" disabled={!hasSelection} onmouseenter={hoverPlainRow} onclick={() => runAction(onCopy)}>
    <Copy class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.copySelection")}</span>
  </button>
  <button class={itemClass} type="button" role="menuitem" onmouseenter={hoverPlainRow} onclick={() => runAction(onPaste)}>
    <ClipboardPaste class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.pasteSelection")}</span>
  </button>
  <button class={itemClass} type="button" role="menuitem" onmouseenter={hoverPlainRow} onclick={() => runAction(onPastePlainText)}>
    <ClipboardPaste class="size-4 shrink-0" aria-hidden="true" />
    <span>{t("notes.contextMenuPastePlainText")}</span>
  </button>
  {#if actionError}
    <div class="px-2 py-1 text-panel-detail text-destructive" role="alert">{actionError}</div>
  {/if}
  </div>
</div>

  {#if submenu && submenuAnchor}
    <div
      bind:this={submenuElement}
      class="surface-floating fixed z-60 flex max-h-[min(26rem,calc(100vh-1rem))] flex-col overflow-hidden"
      style:left={`${submenuPosition?.x ?? 0}px`}
      style:top={`${submenuPosition?.y ?? 0}px`}
      style:width={`${NOTES_TEXT_CONTEXT_SUBMENU_WIDTH}px`}
      style:visibility={submenuPosition ? "visible" : "hidden"}
      role="menu"
      aria-label={submenu === "format" ? t("notes.contextMenuFormat") : submenu === "paragraph" ? t("notes.contextMenuParagraph") : t("notes.contextMenuInsert")}
      tabindex="-1"
      data-app-floating-surface
      onmousedown={(event) => event.preventDefault()}
    >
      <div class="surface-floating-body min-h-0 flex-1 overflow-y-auto" use:scrollEdgeFadeAction>
      {#if submenu === "format"}
        <button class={itemClass} type="button" role="menuitemcheckbox" onmouseenter={hoverPlainSubmenuRow} aria-checked={annotations.bold} onclick={() => runAction(() => onToggleAnnotation("bold"))}><Bold class="size-4 shrink-0" aria-hidden="true" /><span class="flex-1">{t("notes.bold")}</span>{#if annotations.bold}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}</button>
        <button class={itemClass} type="button" role="menuitemcheckbox" onmouseenter={hoverPlainSubmenuRow} aria-checked={annotations.italic} onclick={() => runAction(() => onToggleAnnotation("italic"))}><Italic class="size-4 shrink-0" aria-hidden="true" /><span class="flex-1">{t("notes.italic")}</span>{#if annotations.italic}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}</button>
        <button class={itemClass} type="button" role="menuitemcheckbox" onmouseenter={hoverPlainSubmenuRow} aria-checked={annotations.underline} onclick={() => runAction(() => onToggleAnnotation("underline"))}>
          <svg class="size-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M5.5 4.5v7.25c0 3.75 2.5 5.75 6.5 5.75s6.5-2 6.5-5.75V4.5" /><path d="M4.5 20.5h15" /></svg>
          <span class="flex-1">{t("notes.underline")}</span>
          {#if annotations.underline}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
        <button class={itemClass} type="button" role="menuitemcheckbox" onmouseenter={hoverPlainSubmenuRow} aria-checked={annotations.strikethrough} onclick={() => runAction(() => onToggleAnnotation("strikethrough"))}>
          <svg class="size-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M17.5 5.5c-1.3-1.2-3.1-1.8-5.5-1.8-3.6 0-5.9 1.7-5.9 4.4 0 2.2 1.9 3.5 5.9 4.3 4 .8 5.9 2 5.9 4.2 0 2.8-2.3 4.5-5.9 4.5-2.4 0-4.2-.6-5.5-1.8" /><path d="M3 12.25h18" stroke-width="1.2" /></svg>
          <span class="flex-1">{t("notes.strikethrough")}</span>
          {#if annotations.strikethrough}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
        </button>
        <button class={itemClass} type="button" role="menuitemcheckbox" onmouseenter={hoverPlainSubmenuRow} aria-checked={annotations.code} onclick={() => runAction(() => onToggleAnnotation("code"))}><Code class="size-4 shrink-0" aria-hidden="true" /><span class="flex-1">{t("notes.inlineCode")}</span>{#if annotations.code}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}</button>
        <button class={itemClass} type="button" role="menuitem" aria-haspopup="menu" aria-expanded={Boolean(paletteAnchor)} onmouseenter={(event) => hoverPaletteRow(event.currentTarget)} onclick={(event) => openPalette(event.currentTarget, event.detail === 0)}><Palette class="size-4 shrink-0" aria-hidden="true" /><span class="flex-1">{t("notes.textColor")}</span><ChevronRight class="size-4 shrink-0" aria-hidden="true" /></button>
        <button class={itemClass} type="button" role="menuitem" onmouseenter={hoverPlainSubmenuRow} onclick={() => runAction(onCreateEquation)}><Sigma class="size-4 shrink-0" aria-hidden="true" /><span>{t("notes.inlineEquation")}</span></button>
      {:else if submenu === "paragraph"}
        {#each paragraphItems as item}
          {@const Icon = item.icon}
          <button class={itemClass} type="button" role="menuitemradio" aria-checked={blockType === item.type} onclick={() => runAction(() => onConvert(item.type))}>
            <Icon class="size-4 shrink-0" aria-hidden="true" />
            <span class="min-w-0 flex-1 truncate">{t(item.labelKey)}</span>
            {#if blockType === item.type}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
          </button>
        {/each}
      {:else}
        {#each insertItems as item}
          {@const Icon = item.icon}
          <button class={itemClass} type="button" role="menuitem" onclick={() => runAction(() => onInsert(item.type))}>
            <Icon class="size-4 shrink-0" aria-hidden="true" />
            <span class="min-w-0 truncate">{t(item.labelKey)}</span>
          </button>
        {/each}
      {/if}
      </div>
    </div>
  {/if}

  {#if submenu === "format" && paletteAnchor}
    <div
      bind:this={paletteElement}
      class="fixed z-70"
      style:left={`${palettePosition?.x ?? 0}px`}
      style:top={`${palettePosition?.y ?? 0}px`}
      style:width={`${NOTES_TEXT_CONTEXT_SUBMENU_WIDTH}px`}
      style:visibility={palettePosition ? "visible" : "hidden"}
    >
      <NotesTextColorPalette currentColor={annotations.color} {calloutBackgroundColor} onSelect={(color) => runAction(() => onColorSelect(color))} />
    </div>
  {/if}
</div>
