<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ColorEntry } from "$lib/calendar/utils";
  import { QUICK_NOTE_TAG_NAME_MAX_CHARS } from "$lib/quick-notes/types";
  import { cn } from "$lib/utils";

  /**
   * Edits a tag name in place of its chip: same height, radius, text size, and color, growing with the name, so the tag
   * row keeps its shape while the user types.
   */
  let {
    value = $bindable(""),
    input = $bindable(null),
    label,
    placeholder = "",
    error = "",
    busy = false,
    mobileLayout = false,
    colors,
    leading,
    onInput,
    onSubmit,
    onCancel,
    onBlur,
  }: {
    value?: string;
    input?: HTMLInputElement | null;
    label: string;
    placeholder?: string;
    error?: string;
    busy?: boolean;
    mobileLayout?: boolean;
    /** The tag color; without it the input takes the neutral chip fill. */
    colors?: ColorEntry;
    leading?: Snippet;
    onInput?: () => void;
    onSubmit: () => void;
    onCancel: () => void;
    /** Runs when focus leaves the input for somewhere outside it and its leading controls. */
    onBlur: () => void;
  } = $props();

  let form = $state<HTMLFormElement | null>(null);
  const colored = $derived(colors !== undefined && !error);
</script>

<form
  bind:this={form}
  class={cn(
    "flex shrink-0 items-center gap-1",
    mobileLayout ? "min-h-12 max-w-40 rounded-xl px-4 text-sm" : "h-7 max-w-32 rounded-md px-2.5 text-xs",
    error ? "bg-destructive/15 text-destructive" : !colored && "bg-accent/60 text-foreground",
  )}
  style={colored && colors ? `background-color: ${colors.bg}; color: ${colors.text};` : undefined}
  title={error || undefined}
  data-quick-note-tag-creator
  onsubmit={(event) => { event.preventDefault(); onSubmit(); }}
  onfocusout={(event) => {
    if (event.relatedTarget instanceof Node && form?.contains(event.relatedTarget)) return;
    onBlur();
  }}
>
  {@render leading?.()}
  <!-- The hidden copy sizes the grid cell to the text, so the input grows with the name up to the chip's maximum. -->
  <span class="grid min-w-0">
    <span class="invisible col-start-1 row-start-1 min-w-6 overflow-hidden whitespace-pre pr-0.5" aria-hidden="true">{value || placeholder}</span>
    <input
      bind:this={input}
      bind:value
      type="text"
      maxlength={QUICK_NOTE_TAG_NAME_MAX_CHARS}
      class={cn(
        "col-start-1 row-start-1 w-full min-w-0 border-0 bg-transparent p-0 outline-none",
        colored ? "caret-current placeholder:text-current/60" : "caret-primary placeholder:text-muted-foreground",
      )}
      {placeholder}
      aria-label={label}
      aria-invalid={error ? "true" : undefined}
      disabled={busy}
      oninput={() => onInput?.()}
      onkeydown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onCancel();
      }}
    />
  </span>
  {#if error}<span class="sr-only" role="alert">{error}</span>{/if}
</form>
