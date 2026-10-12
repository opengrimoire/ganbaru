<script lang="ts">
  import { tick } from "svelte";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { QUICK_NOTE_TAG_NAME_MAX_CHARS, type QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import { cn } from "$lib/utils";
  import { portal } from "$lib/utils/portal";

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
  let menuButton = $state<HTMLButtonElement | null>(null);
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
  const MENU_HEIGHT = 84;

  async function openMenu(anchor: HTMLElement | null): Promise<void> {
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const width = FLOATING_WIDTH.sm;
    left = Math.min(Math.max(8, rect.left), Math.max(8, window.innerWidth - width - 8));
    top = rect.bottom + MENU_HEIGHT + 8 <= window.innerHeight
      ? rect.bottom + 4
      : Math.max(8, rect.top - MENU_HEIGHT - 4);
    menuOpen = true;
    await tick();
    firstItem?.focus();
  }

  function closeMenu(restoreFocus = true): void {
    menuOpen = false;
    if (restoreFocus) void tick().then(() => (menuButton ?? chip)?.focus());
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
      await tick();
      input?.focus();
    } finally {
      busy = false;
    }
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
  <form
    class={cn("field flex shrink-0 items-center gap-1 py-0 text-foreground", mobileLayout ? "min-h-12 w-44" : "h-7 w-32", error && "border-destructive focus-within:border-destructive")}
    title={error || t("quickNotes.tag.rename")}
    data-quick-note-tag-creator
    onsubmit={(event) => { event.preventDefault(); void submitRename(); }}
  >
    <input
      bind:this={input}
      type="text"
      bind:value={name}
      maxlength={QUICK_NOTE_TAG_NAME_MAX_CHARS}
      class={mobileLayout ? "field-bare min-h-12 text-base caret-primary" : "field-bare text-xs caret-primary"}
      aria-label={t("quickNotes.tag.rename")}
      aria-invalid={error ? "true" : undefined}
      disabled={busy}
      oninput={() => { error = ""; }}
      onkeydown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        cancelRename(true);
      }}
      onblur={() => { if (!busy && !error) void submitRename(); }}
    />
    {#if error}<span class="sr-only" role="alert">{error}</span>{/if}
  </form>
{:else}
  <div class={cn("flex shrink-0 items-center rounded-md transition-colors", mobileLayout && "rounded-xl", selected && "bg-accent/60")}>
    <button
      bind:this={chip}
      type="button"
      aria-pressed={selected}
      class={`flex items-center text-foreground transition-colors ${mobileLayout ? "min-h-12 max-w-40 rounded-xl px-4 text-sm active:bg-accent" : "h-7 max-w-32 rounded-md px-2.5 text-xs hover:bg-accent/60"} ${selected ? "pr-1" : ""}`}
      {title}
      onclick={onSelect}
      oncontextmenu={(event) => { event.preventDefault(); void openMenu(chip); }}
    ><span class="truncate">{tag.name}</span></button>
    {#if selected}
      <button
        bind:this={menuButton}
        type="button"
        class={mobileLayout ? "flex min-h-12 min-w-10 items-center justify-center rounded-xl text-muted-foreground active:bg-accent" : "flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent/60 hover:text-foreground"}
        aria-label={t("quickNotes.tag.manage", tag.name)}
        title={t("quickNotes.tag.manage", tag.name)}
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onclick={() => { if (menuOpen) closeMenu(); else void openMenu(menuButton); }}
      ><Ellipsis class={mobileLayout ? "size-5" : "size-3.5"} strokeWidth={1.5} aria-hidden="true" /></button>
    {/if}
  </div>
{/if}

{#if menuOpen}
  <button
    use:portal
    type="button"
    class="fixed inset-0 z-90 cursor-default"
    aria-label={t("common.close")}
    onclick={() => closeMenu()}
  ></button>
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
