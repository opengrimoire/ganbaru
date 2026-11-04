<script lang="ts">
  import { onMount, tick } from "svelte";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash";
  import type { EventColor } from "$lib/calendar/types";
  import MoveStepMenuItems from "$lib/components/ui/MoveStepMenuItems.svelte";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import { portal } from "$lib/utils/portal";
  import QuickNoteColorSwatches from "./QuickNoteColorSwatches.svelte";

  const EDGE_PX = 8;
  const GAP_PX = 4;

  /** One place back or forward in the tag order, with the direction the tags run in where the menu opened. */
  interface TagMenuMove {
    orientation: "vertical" | "horizontal";
    canMoveBack: boolean;
    canMoveForward: boolean;
    onMove: (step: -1 | 1) => void;
  }

  /**
   * The menu of a Quick notes view: rename, move, and delete actions when they apply, and a palette for its color. It
   * opens beside `anchor`, or as a bottom sheet on phones.
   */
  let {
    anchor,
    label,
    colorLabel,
    color,
    theme,
    onColor,
    onRename,
    move,
    onDelete,
    onClose,
    mobileLayout = false,
  }: {
    anchor: HTMLElement | null;
    /** Names the menu. */
    label: string;
    /** Titles the palette group. */
    colorLabel: string;
    color: EventColor;
    theme: Theme;
    onColor: (color: EventColor) => void;
    onRename?: () => void;
    move?: TagMenuMove;
    onDelete?: () => void;
    /** Closes the menu; `restoreFocus` is false when the chosen action places focus itself. */
    onClose: (restoreFocus: boolean) => void;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let menu = $state<HTMLDivElement | null>(null);
  let left = $state(EDGE_PX);
  let top = $state(EDGE_PX);
  let placed = $state(false);

  function place(): void {
    if (!anchor || !menu) return;
    const rect = anchor.getBoundingClientRect();
    // Placement uses the rendered height, so a menu opened above its anchor sits right against it.
    const height = menu.getBoundingClientRect().height;
    left = Math.min(Math.max(EDGE_PX, rect.left), Math.max(EDGE_PX, window.innerWidth - FLOATING_WIDTH.sm - EDGE_PX));
    top = rect.bottom + GAP_PX + height + EDGE_PX <= window.innerHeight
      ? rect.bottom + GAP_PX
      : Math.max(EDGE_PX, rect.top - height - GAP_PX);
    placed = true;
  }

  /** The first action, or the current color when the menu has only the palette. */
  function initialFocus(): HTMLElement | null {
    return menu?.querySelector<HTMLElement>("[data-tag-menu-action]")
      ?? menu?.querySelector<HTMLElement>("[aria-checked='true']")
      ?? null;
  }

  function act(action: (() => void) | undefined): void {
    onClose(false);
    action?.();
  }

  function chooseColor(next: EventColor): void {
    onClose(true);
    if (next !== color) onColor(next);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    onClose(true);
  }

  const menuStyle = $derived(mobileLayout
    ? "left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem); right: calc(var(--safe-area-right) + 0.5rem); bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem); max-width: 32rem; max-height: calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - 1rem); margin-inline: auto;"
    : `left: ${left}px; top: ${top}px;${placed ? "" : " visibility: hidden;"}`);

  onMount(() => {
    void tick().then(place);
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: () => onClose(true) })
      : () => undefined;
    const deactivateFocus = menu ? activateModalFocus(menu, initialFocus()) : () => undefined;
    window.addEventListener("keydown", handleKeydown, true);
    return () => {
      deactivateBack();
      deactivateFocus();
      window.removeEventListener("keydown", handleKeydown, true);
    };
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div use:portal class="fixed inset-0 z-90" aria-hidden="true" onclick={() => onClose(true)}></div>
<div
  bind:this={menu}
  use:portal
  class={mobileLayout ? "surface-floating fixed z-100 flex flex-col overflow-y-auto overscroll-contain" : "surface-floating fixed z-100 flex w-floating-sm flex-col"}
  style={menuStyle}
  role="menu"
  tabindex="-1"
  aria-label={label}
>
  <div class="surface-floating-body flex flex-col">
    {#if onRename}
      <button type="button" role="menuitem" class="menu-item" data-tag-menu-action onclick={() => act(onRename)}>
        <Pencil strokeWidth={1.5} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.rename")}</span>
      </button>
    {/if}
    {#if move}
      <MoveStepMenuItems
        canMoveUp={move.canMoveBack}
        canMoveDown={move.canMoveForward}
        orientation={move.orientation}
        onMove={move.onMove}
        onClose={() => onClose(true)}
      />
    {/if}
    {#if onRename || move}<hr class="menu-separator" />{/if}
    <div class="menu-label">{colorLabel}</div>
    <QuickNoteColorSwatches
      {color}
      {theme}
      label={colorLabel}
      onSelect={chooseColor}
      columns="repeat(8, minmax(0, 1fr))"
      class={mobileLayout ? "gap-1.5 px-2 pb-2" : "gap-1 px-2 pb-1.5"}
      swatchClass={mobileLayout ? "aspect-square w-full rounded-lg" : "aspect-square w-full rounded-[3px]"}
      inMenu
    />
    {#if onDelete}
      <hr class="menu-separator" />
      <button type="button" role="menuitem" class="menu-item menu-item-destructive" onclick={() => act(onDelete)}>
        <Trash2 strokeWidth={1.5} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.delete")}</span>
      </button>
    {/if}
  </div>
</div>
