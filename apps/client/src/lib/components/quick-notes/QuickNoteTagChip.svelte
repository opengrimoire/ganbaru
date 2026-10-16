<script lang="ts">
  import { tick } from "svelte";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import type { QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";
  import QuickNoteTagNameInput from "./QuickNoteTagNameInput.svelte";

  let {
    tag,
    selected,
    title,
    onSelect,
    onRename,
    onDelete,
    mobileLayout = false,
  }: {
    tag: QuickNoteTag;
    selected: boolean;
    title: string;
    onSelect: () => void;
    /** Rejects with a message to show when the name cannot be saved. */
    onRename: (name: string) => Promise<void>;
    onDelete: () => void;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let chip = $state<HTMLButtonElement | null>(null);
  let menu = $state<HTMLDivElement | null>(null);
  let firstItem = $state<HTMLButtonElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let menuOpen = $state(false);
  let renaming = $state(false);
  let name = $state("");
  let busy = $state(false);
  let error = $state("");
  let left = $state(8);
  let top = $state(8);

  async function openMenu(anchor: HTMLElement | null): Promise<void> {
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    menuOpen = true;
    await tick();
    // Placement uses the rendered height, so a menu opened above the chip sits right against it.
    const height = menu?.getBoundingClientRect().height ?? 0;
    left = Math.min(Math.max(8, rect.left), Math.max(8, window.innerWidth - FLOATING_WIDTH.sm - 8));
    top = rect.bottom + height + 8 <= window.innerHeight
      ? rect.bottom + 4
      : Math.max(8, rect.top - height - 4);
    firstItem?.focus();
  }

  function closeMenu(restoreFocus = true): void {
    menuOpen = false;
    if (restoreFocus) void tick().then(() => chip?.focus());
  }

  async function beginRename(): Promise<void> {
    closeMenu(false);
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

  function requestDelete(): void {
    closeMenu();
    onDelete();
  }

  function handleMenuKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closeMenu();
  }

  const menuStyle = $derived(mobileLayout
    ? "left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem); right: calc(var(--safe-area-right) + 0.5rem); bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem); max-width: 32rem; margin-inline: auto;"
    : `left: ${left}px; top: ${top}px;`);

  $effect(() => {
    if (!menuOpen) return;
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: () => closeMenu() })
      : () => undefined;
    const deactivateFocus = menu ? activateModalFocus(menu, firstItem) : () => undefined;
    window.addEventListener("keydown", handleMenuKeydown, true);
    return () => {
      deactivateBack();
      deactivateFocus();
      window.removeEventListener("keydown", handleMenuKeydown, true);
    };
  });

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
    onInput={() => { error = ""; }}
    onSubmit={() => void submitRename()}
    onCancel={() => cancelRename(true)}
    onBlur={() => { if (!busy && !error) void submitRename(); }}
  />
{:else}
  <!-- A selected chip opens its menu on click, so rename and delete need no extra control beside it. -->
  <button
    bind:this={chip}
    type="button"
    aria-pressed={selected}
    aria-haspopup={selected ? "menu" : undefined}
    aria-expanded={selected ? menuOpen : undefined}
    class={cn("flex shrink-0 items-center text-foreground transition-colors", mobileLayout ? "min-h-12 max-w-40 rounded-xl px-4 text-sm active:bg-accent" : "h-7 max-w-32 rounded-md px-2.5 text-xs hover:bg-accent/60", selected && "bg-accent/60")}
    {title}
    onclick={() => { if (selected) void openMenu(chip); else onSelect(); }}
    oncontextmenu={(event) => { event.preventDefault(); void openMenu(chip); }}
    onkeydown={(event) => {
      if (event.key !== "F2") return;
      event.preventDefault();
      void beginRename();
    }}
  ><span class="truncate">{tag.name}</span></button>
{/if}

{#if menuOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div use:portal class="fixed inset-0 z-90" aria-hidden="true" onclick={() => closeMenu()}></div>
  <div
    bind:this={menu}
    use:portal
    class={mobileLayout ? "surface-floating fixed z-100 flex flex-col overflow-hidden" : "surface-floating fixed z-100 flex w-floating-sm flex-col overflow-hidden"}
    style={menuStyle}
    role="menu"
    tabindex="-1"
    aria-label={t("quickNotes.tag.manage", tag.name)}
  >
    <div class="surface-floating-body">
      <button bind:this={firstItem} type="button" role="menuitem" class="menu-item" onclick={() => void beginRename()}>
        <Pencil class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.rename")}</span>
      </button>
      <button type="button" role="menuitem" class="menu-item text-destructive" onclick={requestDelete}>
        <Trash2 class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.delete")}</span>
      </button>
    </div>
  </div>
{/if}
