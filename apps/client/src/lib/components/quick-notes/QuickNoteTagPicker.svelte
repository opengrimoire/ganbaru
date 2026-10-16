<script lang="ts">
  import { tick } from "svelte";
  import Check from "@lucide/svelte/icons/check";
  import Plus from "@lucide/svelte/icons/plus";
  import Tag from "@lucide/svelte/icons/tag";
  import type { EventColor } from "$lib/calendar/types";
  import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { activateModalFocus } from "$lib/modal-focus";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { pickQuickNoteTagColor } from "$lib/quick-notes/tags";
  import { QUICK_NOTE_TAG_LIMIT, QUICK_NOTE_TAG_NAME_MAX_CHARS, type QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import { portal } from "$lib/utils/portal";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import QuickNoteColorSwatches from "./QuickNoteColorSwatches.svelte";

  let {
    tagId,
    tags,
    theme,
    defaultColor,
    onSelect,
    onCreate,
    buttonClass = "",
    mobileLayout = false,
  }: {
    tagId: string | null;
    tags: readonly QuickNoteTag[];
    theme: Theme;
    /** Color of untagged notes, which a new tag avoids. */
    defaultColor: EventColor;
    onSelect: (tagId: string | null) => void;
    /** Creates a tag and resolves to it, or rejects with an error whose message is shown; without it the picker offers no new tag. */
    onCreate?: (name: string, color: EventColor) => Promise<QuickNoteTag>;
    buttonClass?: string;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let open = $state(false);
  let button = $state<HTMLButtonElement | null>(null);
  let firstOption = $state<HTMLButtonElement | null>(null);
  let picker = $state<HTMLDivElement | null>(null);
  let anchor: DOMRect | null = null;
  let left = $state(8);
  let top = $state(8);
  let creating = $state(false);
  let name = $state("");
  let newColor = $state<EventColor>(0);
  let busy = $state(false);
  let createError = $state("");
  let nameInput = $state<HTMLInputElement | null>(null);
  let newTagButton = $state<HTMLButtonElement | null>(null);
  const canCreate = $derived(onCreate !== undefined && tags.length < QUICK_NOTE_TAG_LIMIT);

  /** Places the picker against its button, above it when the rendered height does not fit below. */
  function place(): void {
    if (!anchor || !picker || mobileLayout) return;
    const height = picker.getBoundingClientRect().height;
    left = Math.min(Math.max(8, anchor.left), Math.max(8, window.innerWidth - FLOATING_WIDTH.sm - 8));
    top = anchor.bottom + height + 8 <= window.innerHeight
      ? anchor.bottom + 4
      : Math.max(8, anchor.top - height - 4);
  }

  async function openPicker(): Promise<void> {
    if (!button) return;
    anchor = button.getBoundingClientRect();
    open = true;
    await tick();
    place();
    firstOption?.focus();
  }

  function choose(next: string | null): void {
    onSelect(next);
    closePicker();
  }

  function closePicker(): void {
    open = false;
    creating = false;
    name = "";
    createError = "";
    void tick().then(() => button?.focus());
  }

  async function beginCreate(): Promise<void> {
    if (!canCreate) return;
    creating = true;
    name = "";
    createError = "";
    newColor = pickQuickNoteTagColor(tags.map((tag) => tag.color), defaultColor);
    await tick();
    nameInput?.focus();
  }

  function cancelCreate(): void {
    if (busy) return;
    creating = false;
    name = "";
    createError = "";
    void tick().then(() => newTagButton?.focus());
  }

  /** Creates the tag and assigns it, which also closes the picker. */
  async function submitCreate(): Promise<void> {
    const value = name.trim();
    if (!onCreate || !value || busy) return;
    busy = true;
    createError = "";
    let created: QuickNoteTag | null = null;
    try {
      created = await onCreate(value, newColor);
    } catch (cause: unknown) {
      createError = cause instanceof Error && cause.message ? cause.message : t("quickNotes.tag.saveFailed");
    } finally {
      busy = false;
    }
    if (created) {
      onSelect(created.id);
      if (open) closePicker();
      return;
    }
    // The input is disabled while saving, so it can take focus back only once busy clears.
    await tick();
    nameInput?.focus();
  }

  function chooseNewColor(next: EventColor): void {
    newColor = next;
    nameInput?.focus();
  }

  function handlePickerKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    if (creating) cancelCreate();
    else closePicker();
  }

  const pickerStyle = $derived(mobileLayout
    ? "left: calc(var(--visual-viewport-offset-left) + var(--safe-area-left) + 0.5rem); right: calc(var(--safe-area-right) + 0.5rem); bottom: calc(var(--keyboard-inset) + var(--safe-area-bottom) + 0.5rem); max-width: 32rem; max-height: min(60vh, 24rem, calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - 1rem)); margin-inline: auto;"
    : `left: ${left}px; top: ${top}px;`);
  const newColors = $derived(getQuickNoteColor(newColor, theme));

  $effect(() => {
    if (!open) return;
    const deactivateBack = mobileLayout
      ? mobileBackStack.activate({ handle: closePicker })
      : () => undefined;
    const deactivateFocus = picker
      ? activateModalFocus(picker, firstOption)
      : () => undefined;
    // The new tag form changes the height, so a picker above its button stays against it.
    const observer = picker && !mobileLayout ? new ResizeObserver(place) : null;
    if (picker) observer?.observe(picker);
    window.addEventListener("keydown", handlePickerKeydown, true);
    return () => {
      deactivateBack();
      deactivateFocus();
      observer?.disconnect();
      window.removeEventListener("keydown", handlePickerKeydown, true);
    };
  });

  $effect(() => {
    if (!mobileLayout || !creating) return;
    return mobileBackStack.activate({ handle: cancelCreate });
  });
</script>

<button
  bind:this={button}
  type="button"
  class={buttonClass}
  aria-label={t("quickNotes.tag.assign")}
  title={t("quickNotes.tag.assign")}
  aria-haspopup="dialog"
  aria-expanded={open}
  onclick={() => { if (open) closePicker(); else void openPicker(); }}
>
  <Tag class="size-4" strokeWidth={1.5} aria-hidden="true" />
</button>

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div use:portal class="fixed inset-0 z-90" aria-hidden="true" onclick={closePicker}></div>
  <div
    bind:this={picker}
    use:portal
    class={mobileLayout ? "surface-floating fixed z-100 flex flex-col overflow-hidden" : "surface-floating fixed z-100 flex max-h-75 w-floating-sm flex-col overflow-hidden"}
    style={pickerStyle}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    aria-label={t("quickNotes.tag.assign")}
  >
    <div use:scrollEdgeFadeAction class="surface-floating-body min-h-0 overflow-y-auto overscroll-contain">
      <button
        bind:this={firstOption}
        type="button"
        class="menu-item"
        onclick={() => choose(null)}
      >
        <span class="size-3 shrink-0 rounded-full border border-dashed border-muted-foreground" aria-hidden="true"></span>
        <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.none")}</span>
        {#if tagId === null}<Check class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />{/if}
      </button>
      {#each tags as tag}
        <button
          type="button"
          class="menu-item"
          onclick={() => choose(tag.id)}
        >
          <span class="size-3 shrink-0 rounded-full border border-border" style="background-color: {getQuickNoteColor(tag.color, theme).bg};" aria-hidden="true"></span>
          <span class="min-w-0 flex-1 truncate">{tag.name}</span>
          {#if tagId === tag.id}<Check class="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />{/if}
        </button>
      {/each}
    </div>
    {#if canCreate || creating}
      <div class="surface-floating-body flex shrink-0 flex-col border-t border-border">
        {#if creating}
          <form class="flex flex-col gap-1.5" onsubmit={(event) => { event.preventDefault(); void submitCreate(); }}>
            <div class="flex items-center gap-1">
              <label class="field flex min-w-0 flex-1 items-center gap-1.5" data-invalid={createError ? "" : undefined}>
                <span class="size-3 shrink-0 rounded-full border border-border" style="background-color: {newColors.bg};" aria-hidden="true"></span>
                <input
                  bind:this={nameInput}
                  bind:value={name}
                  type="text"
                  maxlength={QUICK_NOTE_TAG_NAME_MAX_CHARS}
                  class="field-bare"
                  placeholder={t("quickNotes.tag.namePlaceholder")}
                  aria-label={t("quickNotes.tag.namePlaceholder")}
                  aria-invalid={createError ? "true" : undefined}
                  disabled={busy}
                  oninput={() => { createError = ""; }}
                />
              </label>
              <button
                type="submit"
                class={mobileLayout ? "flex size-12 shrink-0 items-center justify-center rounded-(--floating-item-radius) active:bg-accent disabled:opacity-50" : "flex size-7 shrink-0 items-center justify-center rounded-(--floating-item-radius) hover:bg-accent disabled:opacity-50"}
                aria-label={t("quickNotes.tag.create")}
                title={t("quickNotes.tag.create")}
                disabled={busy || name.trim() === ""}
              >
                <Check class="size-4" strokeWidth={1.5} aria-hidden="true" />
              </button>
            </div>
            {#if createError}<p class="text-xs text-destructive" role="alert">{createError}</p>{/if}
            <QuickNoteColorSwatches
              color={newColor}
              {theme}
              label={t("quickNotes.tag.color")}
              onSelect={chooseNewColor}
              columns="repeat(8, minmax(0, 1fr))"
              class={mobileLayout ? "gap-1.5" : "gap-1"}
              swatchClass={mobileLayout ? "aspect-square w-full rounded-lg" : "aspect-square w-full rounded-[3px]"}
            />
          </form>
        {:else}
          <button bind:this={newTagButton} type="button" class="menu-item" onclick={() => void beginCreate()}>
            <span class="flex size-3 shrink-0 items-center justify-center" aria-hidden="true"><Plus class="size-3.5" strokeWidth={1.5} /></span>
            <span class="min-w-0 flex-1 truncate">{t("quickNotes.tag.new")}</span>
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}
