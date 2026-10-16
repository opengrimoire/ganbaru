<script lang="ts">
  import { tick } from "svelte";
  import Palette from "@lucide/svelte/icons/palette";
  import Plus from "@lucide/svelte/icons/plus";
  import type { EventColor } from "$lib/calendar/types";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { getQuickNoteColor } from "$lib/quick-notes/colors";
  import { pickQuickNoteTagColor } from "$lib/quick-notes/tags";
  import { QUICK_NOTE_TAG_LIMIT, type QuickNoteTag } from "$lib/quick-notes/types";
  import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
  import type { Theme } from "$lib/themes";
  import QuickNoteColorPicker from "./QuickNoteColorPicker.svelte";
  import QuickNoteTagNameInput from "./QuickNoteTagNameInput.svelte";

  let {
    tagCount,
    usedColors,
    defaultColor,
    theme,
    onCreate,
    mobileLayout = false,
  }: {
    tagCount: number;
    /** Colors existing tags use, which a new tag avoids while others are free. */
    usedColors: readonly EventColor[];
    /** Color of untagged notes, which a new tag avoids. */
    defaultColor: EventColor;
    theme: Theme;
    /** Creates the tag, or rejects with an error whose message is shown. */
    onCreate: (name: string, color: EventColor) => Promise<QuickNoteTag>;
    mobileLayout?: boolean;
  } = $props();

  const { t } = getLocalization();
  const mobileBackStack = getMobileBackStack();
  let creating = $state(false);
  let input = $state<HTMLInputElement | null>(null);
  let createButton = $state<HTMLButtonElement | null>(null);
  let name = $state("");
  let color = $state<EventColor>(0);
  let paletteOpen = $state(false);
  let busy = $state(false);
  let error = $state("");
  const colors = $derived(getQuickNoteColor(color, theme));

  async function begin(): Promise<void> {
    if (tagCount >= QUICK_NOTE_TAG_LIMIT) return;
    creating = true;
    name = "";
    color = pickQuickNoteTagColor(usedColors, defaultColor);
    error = "";
    await tick();
    input?.focus();
  }

  async function create(): Promise<void> {
    const value = name.trim();
    if (!value || busy || tagCount >= QUICK_NOTE_TAG_LIMIT) return;
    busy = true;
    error = "";
    try {
      await onCreate(value, color);
      name = "";
      creating = false;
    } catch (cause: unknown) {
      error = cause instanceof Error && cause.message ? cause.message : t("quickNotes.tag.saveFailed");
    } finally {
      busy = false;
    }
    // The input is disabled while saving, so it can take focus back only once busy clears.
    if (!error) return;
    await tick();
    input?.focus();
  }

  function cancel(restoreFocus = false): void {
    if (busy) return;
    creating = false;
    name = "";
    error = "";
    if (restoreFocus) void tick().then(() => createButton?.focus());
  }

  $effect(() => {
    if (!mobileLayout || !creating) return;
    return mobileBackStack.activate({ handle: () => cancel(true) });
  });
</script>

<div class="shrink-0" data-quick-note-tag-creator>
  {#if creating}
    <QuickNoteTagNameInput
      bind:value={name}
      bind:input
      label={t("quickNotes.tag.namePlaceholder")}
      placeholder={t("quickNotes.tag.namePlaceholder")}
      {error}
      {busy}
      {mobileLayout}
      {colors}
      onInput={() => { error = ""; }}
      onSubmit={() => void create()}
      onCancel={() => cancel(true)}
      onBlur={() => { if (!busy && !paletteOpen) cancel(); }}
    >
      {#snippet leading()}
        <!-- The palette opens from inside the name input, so the user can recolor the tag before saving it. -->
        <QuickNoteColorPicker
          bind:open={paletteOpen}
          {color}
          {theme}
          {mobileLayout}
          label={t("quickNotes.tag.color")}
          buttonClass={mobileLayout ? "-ml-2 flex size-9 shrink-0 items-center justify-center rounded-lg active:bg-current/15" : "-ml-1 flex size-5 shrink-0 items-center justify-center rounded-sm hover:bg-current/15"}
          onSelect={(next) => { color = next; }}
          onClosed={() => input?.focus()}
        >
          {#snippet trigger()}<Palette class={mobileLayout ? "size-4" : "size-3.5"} strokeWidth={1.5} aria-hidden="true" />{/snippet}
        </QuickNoteColorPicker>
      {/snippet}
    </QuickNoteTagNameInput>
  {:else if tagCount < QUICK_NOTE_TAG_LIMIT}
    <button
      bind:this={createButton}
      type="button"
      class={mobileLayout ? "flex min-h-12 shrink-0 items-center gap-1 rounded-xl px-4 text-sm text-muted-foreground transition-colors active:bg-accent active:text-foreground" : "flex h-7 shrink-0 items-center gap-1 rounded-md px-2 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"}
      aria-label={t("quickNotes.tag.add")}
      onclick={() => void begin()}
    >
      <Plus class="size-3.5" strokeWidth={1.5} />
      <span class="max-[360px]:hidden">{t("quickNotes.tag.add")}</span>
    </button>
  {/if}
</div>
