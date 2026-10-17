<script lang="ts">
  import { tick } from "svelte";
  import type { EventColor } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { quickNoteChipClass } from "$lib/quick-notes/tags";
  import type { QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import QuickNoteTagMenu from "./QuickNoteTagMenu.svelte";
  import QuickNoteTagNameInput from "./QuickNoteTagNameInput.svelte";

  /** Moves the tag one place along the tag row. */
  interface TagChipMove {
    canMoveBack: boolean;
    canMoveForward: boolean;
    onMove: (step: -1 | 1) => void;
  }

  let {
    tag,
    selected,
    title,
    theme,
    move,
    onSelect,
    onRename,
    onColor,
    onDelete,
    onPointerDown,
    mobileLayout = false,
  }: {
    tag: QuickNoteTag;
    selected: boolean;
    title: string;
    theme: Theme;
    move: TagChipMove;
    onSelect: () => void;
    /** Rejects with a message to show when the name cannot be saved. */
    onRename: (name: string) => Promise<void>;
    onColor: (color: EventColor) => void;
    onDelete: () => void;
    /** Presses on the chip, which can start dragging it along the row. */
    onPointerDown?: (event: PointerEvent) => void;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let chip = $state<HTMLButtonElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let menuOpen = $state(false);
  let renaming = $state(false);
  let name = $state("");
  let busy = $state(false);
  let error = $state("");
  const colors = $derived(getQuickNoteColor(tag.color, theme));

  function closeMenu(restoreFocus: boolean): void {
    menuOpen = false;
    if (restoreFocus) void tick().then(() => chip?.focus());
  }

  async function beginRename(): Promise<void> {
    name = tag.name;
    error = "";
    renaming = true;
    await tick();
    input?.select();
  }

  function cancelRename(restoreFocus = false): void {
    if (busy) return;
    renaming = false;
    error = "";
    if (restoreFocus) void tick().then(() => chip?.focus());
  }

  async function submitRename(): Promise<void> {
    const value = name.trim();
    if (busy) return;
    if (!value || value === tag.name) {
      cancelRename(true);
      return;
    }
    busy = true;
    error = "";
    try {
      await onRename(value);
      renaming = false;
      void tick().then(() => chip?.focus());
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

  $effect(() => {
    if (!mobileLayout || !renaming) return;
    return mobileBackStack.activate({ handle: () => cancelRename(true) });
  });
</script>

{#if renaming}
  <QuickNoteTagNameInput
    bind:value={name}
    bind:input
    label={t("quickNotes.tag.rename")}
    {error}
    {busy}
    {mobileLayout}
    {colors}
    onInput={() => { error = ""; }}
    onSubmit={() => void submitRename()}
    onCancel={() => cancelRename(true)}
    onBlur={() => { if (!busy && !error) void submitRename(); }}
  />
{:else}
  <!-- A selected chip opens its menu on click, so its actions need no extra control beside it. -->
  <button
    bind:this={chip}
    type="button"
    aria-pressed={selected}
    aria-haspopup={selected ? "menu" : undefined}
    aria-expanded={selected ? menuOpen : undefined}
    class={quickNoteChipClass(mobileLayout, selected)}
    style="--tag-bg: {colors.bg}; --tag-fg: {colors.text};"
    {title}
    onclick={() => { if (selected) menuOpen = true; else onSelect(); }}
    oncontextmenu={(event) => { event.preventDefault(); menuOpen = true; }}
    onpointerdown={onPointerDown}
    onkeydown={(event) => {
      if (event.key !== "F2") return;
      event.preventDefault();
      void beginRename();
    }}
  ><span class="truncate">{tag.name}</span></button>
{/if}

{#if menuOpen}
  <QuickNoteTagMenu
    anchor={chip}
    label={t("quickNotes.tag.manage", tag.name)}
    colorLabel={t("quickNotes.tag.color")}
    color={tag.color}
    {theme}
    {mobileLayout}
    onColor={onColor}
    onRename={() => void beginRename()}
    move={{ orientation: "horizontal", ...move }}
    {onDelete}
    onClose={closeMenu}
  />
{/if}
