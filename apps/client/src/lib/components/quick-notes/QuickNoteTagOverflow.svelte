<script lang="ts">
  import { onMount, tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import type { EventColor } from "$lib/calendar/types";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { QUICK_NOTE_TAG_NAME_MAX_CHARS, type QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import { portal } from "$lib/utils/portal";
  import { createSortableDrag } from "$lib/utils/sortable-drag.svelte";
  import QuickNoteTagMenu from "./QuickNoteTagMenu.svelte";

  const EDGE_PX = 8;
  const GAP_PX = 4;

  /**
   * Every tag in a list beside the button that opens it, for when the tag row cannot show them all. Rows select a tag,
   * drag to reorder, and open the same menu as the chips to rename, move, recolor, or delete.
   */
  let {
    anchor,
    tags,
    selectedTagId,
    theme,
    canMove,
    onSelect,
    onRename,
    onColor,
    onMove,
    onDelete,
    onClose,
    mobileLayout = false,
  }: {
    anchor: HTMLElement | null;
    tags: readonly QuickNoteTag[];
    selectedTagId: string | null;
    theme: Theme;
    /** Whether a reorder may start now, false while an earlier move is still saving. */
    canMove: boolean;
    onSelect: (tagId: string) => void;
    /** Rejects with a message to show when the name cannot be saved. */
    onRename: (tag: QuickNoteTag, name: string) => Promise<void>;
    onColor: (tag: QuickNoteTag, color: EventColor) => void;
    /** Moves the tag to `targetIndex` in the full tag order. */
    onMove: (id: string, targetIndex: number) => void | Promise<void>;
    onDelete: (tag: QuickNoteTag) => void;
    /** Closes the list; `restoreFocus` is false when the chosen action places focus itself. */
    onClose: (restoreFocus: boolean) => void;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let list = $state<HTMLDivElement | null>(null);
  let left = $state(EDGE_PX);
  let top = $state(EDGE_PX);
  let maxHeight = $state<number | null>(null);
  let placed = $state(false);
  let menuTagId = $state<string | null>(null);
  let menuAnchor = $state<HTMLElement | null>(null);
  let menuTargetId: string | null = null;
  let renamingId = $state<string | null>(null);
  let name = $state("");
  let input = $state<HTMLInputElement | null>(null);
  let busy = $state(false);
  let error = $state("");
  const menuTag = $derived(tags.find((tag) => tag.id === menuTagId) ?? null);
  const tagIds = (): string[] => tags.map((tag) => tag.id);

  const drag = createSortableDrag({
    getIds: tagIds,
    canStart: () => canMove && renamingId === null,
    commit: (id, targetIndex) => onMove(id, targetIndex),
    touchHold: true,
  });

  function place(): void {
    if (!anchor || !list) return;
    const rect = anchor.getBoundingClientRect();
    const height = list.scrollHeight;
    const below = window.innerHeight - rect.bottom - GAP_PX - EDGE_PX;
    const above = rect.top - GAP_PX - EDGE_PX;
    const openBelow = height <= below || below >= above;
    const room = openBelow ? below : above;
    maxHeight = height > room ? room : null;
    const shown = Math.min(height, room);
    left = Math.min(Math.max(EDGE_PX, rect.right - FLOATING_WIDTH.sm), Math.max(EDGE_PX, window.innerWidth - FLOATING_WIDTH.sm - EDGE_PX));
    top = openBelow ? rect.bottom + GAP_PX : Math.max(EDGE_PX, rect.top - shown - GAP_PX);
    placed = true;
  }

  function stepMove(tag: QuickNoteTag, step: -1 | 1): void {
    const index = tags.findIndex((current) => current.id === tag.id);
    if (index < 0) return;
    void onMove(tag.id, index + step);
  }

  function openMenu(tag: QuickNoteTag, trigger: HTMLElement): void {
    menuAnchor = trigger;
    menuTagId = tag.id;
    menuTargetId = tag.id;
  }

  /** Runs a menu action on the tag the menu opened for, which the menu closes before running its actions. */
  function withMenuTarget(action: (tag: QuickNoteTag) => void): void {
    const tag = tags.find((current) => current.id === menuTargetId);
    if (tag) action(tag);
  }

  function closeMenu(restoreFocus: boolean): void {
    const trigger = menuAnchor;
    menuTagId = null;
    if (restoreFocus) void tick().then(() => trigger?.focus());
  }

  async function beginRename(tag: QuickNoteTag): Promise<void> {
    name = tag.name;
    error = "";
    renamingId = tag.id;
    await tick();
    input?.select();
  }

  function rowButton(tagId: string): HTMLElement | null {
    return list?.querySelector<HTMLElement>(`[data-tag-row="${CSS.escape(tagId)}"]`) ?? null;
  }

  function cancelRename(restoreFocus = false): void {
    if (busy) return;
    const tagId = renamingId;
    renamingId = null;
    error = "";
    if (restoreFocus && tagId) void tick().then(() => rowButton(tagId)?.focus());
  }

  async function submitRename(tag: QuickNoteTag): Promise<void> {
    const value = name.trim();
    if (busy) return;
    if (!value || value === tag.name) {
      cancelRename(true);
      return;
    }
    busy = true;
    error = "";
    try {
      await onRename(tag, value);
      renamingId = null;
      void tick().then(() => rowButton(tag.id)?.focus());
    } catch (cause: unknown) {
      error = cause instanceof Error ? cause.message : String(cause);
    } finally {
      busy = false;
    }
    // The input is disabled while saving, so it can take focus back only once busy clears.
    if (!error) return;
    await tick();
    input?.focus();
  }

  function handleKeydown(event: KeyboardEvent): void {
    // The tag menu and the rename field take their own Escape first.
    if (event.key !== "Escape" || menuTagId !== null || renamingId !== null || drag.draggedId !== null) return;
    event.preventDefault();
    event.stopPropagation();
    onClose(true);
  }

  const listStyle = $derived(mobileLayout
    ? "left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem); right: calc(var(--safe-area-right) + 0.5rem); bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem); max-width: 32rem; max-height: calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - 1rem); margin-inline: auto;"
    : `left: ${left}px; top: ${top}px;${maxHeight === null ? "" : ` max-height: ${maxHeight}px;`}${placed ? "" : " visibility: hidden;"}`);

  onMount(() => {
    void tick().then(place);
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: () => onClose(true) })
      : () => undefined;
    const selected = selectedTagId ? rowButton(selectedTagId) : null;
    const deactivateFocus = list ? activateModalFocus(list, selected) : () => undefined;
    window.addEventListener("keydown", handleKeydown, true);
    return () => {
      drag.reset();
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
  bind:this={list}
  use:portal
  class={mobileLayout ? "surface-floating fixed z-100 flex flex-col overflow-y-auto overscroll-contain" : "surface-floating fixed z-100 flex w-floating-sm flex-col overflow-y-auto overscroll-contain"}
  style={listStyle}
  role="dialog"
  aria-modal="true"
  aria-label={t("quickNotes.tag.all")}
  tabindex="-1"
  inert={menuTag !== null}
>
  <div class="surface-floating-body flex flex-col">
    <div class="menu-label">{t("quickNotes.tag.all")}</div>
    {#each tags as tag (tag.id)}
      {@const colors = getQuickNoteColor(tag.color, theme)}
      {@const selected = tag.id === selectedTagId}
      <div
        class="flex items-center gap-0.5 rounded-md"
        {@attach drag.item(tag.id)}
      >
        {#if renamingId === tag.id}
          <form
            class="flex min-h-(--panel-row-height) min-w-0 flex-1 items-center gap-2 px-2"
            data-quick-note-tag-creator
            onsubmit={(event) => { event.preventDefault(); void submitRename(tag); }}
          >
            <span class="size-3 shrink-0 rounded-full border border-border" style="background-color: {colors.bg};" aria-hidden="true"></span>
            <input
              bind:this={input}
              bind:value={name}
              type="text"
              maxlength={QUICK_NOTE_TAG_NAME_MAX_CHARS}
              class="field min-w-0 flex-1"
              aria-label={t("quickNotes.tag.rename")}
              aria-invalid={error ? "true" : undefined}
              title={error || undefined}
              disabled={busy}
              oninput={() => { error = ""; }}
              onblur={() => { if (!busy && !error) void submitRename(tag); }}
              onkeydown={(event) => {
                if (event.key !== "Escape") return;
                event.preventDefault();
                event.stopPropagation();
                cancelRename(true);
              }}
            />
            {#if error}<span class="sr-only" role="alert">{error}</span>{/if}
          </form>
        {:else}
          <button
            type="button"
            class="menu-item min-w-0 flex-1"
            aria-pressed={selected}
            data-tag-row={tag.id}
            onpointerdown={(event) => drag.pointerDown(event, tag.id)}
            onclick={() => onSelect(tag.id)}
            oncontextmenu={(event) => { event.preventDefault(); openMenu(tag, event.currentTarget); }}
            onkeydown={(event) => {
              if (event.key !== "F2") return;
              event.preventDefault();
              void beginRename(tag);
            }}
          >
            <span class="size-3 shrink-0 rounded-full border border-border" style="background-color: {colors.bg};" aria-hidden="true"></span>
            <span class="min-w-0 flex-1 truncate">{tag.name}</span>
            {#if selected}<Check strokeWidth={1.5} aria-hidden="true" />{/if}
          </button>
          <button
            type="button"
            class="menu-item w-auto shrink-0 justify-center px-1.5"
            aria-label={t("quickNotes.tag.manage", tag.name)}
            title={t("quickNotes.tag.manage", tag.name)}
            aria-haspopup="menu"
            aria-expanded={menuTagId === tag.id}
            onclick={(event) => openMenu(tag, event.currentTarget)}
          ><Ellipsis strokeWidth={1.5} aria-hidden="true" /></button>
        {/if}
      </div>
    {/each}
  </div>
</div>

{#if menuTag}
  {@const index = tags.indexOf(menuTag)}
  <QuickNoteTagMenu
    anchor={menuAnchor}
    label={t("quickNotes.tag.manage", menuTag.name)}
    colorLabel={t("quickNotes.tag.color")}
    color={menuTag.color}
    {theme}
    {mobileLayout}
    onColor={(color) => withMenuTarget((tag) => onColor(tag, color))}
    onRename={() => withMenuTarget((tag) => void beginRename(tag))}
    move={{
      orientation: "vertical",
      canMoveBack: canMove && index > 0,
      canMoveForward: canMove && index < tags.length - 1,
      onMove: (step) => withMenuTarget((tag) => stepMove(tag, step)),
    }}
    onDelete={() => withMenuTarget(onDelete)}
    onClose={closeMenu}
  />
{/if}
