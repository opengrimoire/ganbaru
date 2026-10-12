<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";

  let { class: className, children, onclick, ...attributes }: HTMLAttributes<HTMLDivElement> & { children: Snippet } = $props();

  /**
   * Marks the control that owns the whole cell. Buttons and labels stretch their hit area over the cell in CSS;
   * text fields cannot, so clicks on the remaining cell space focus them here.
   */
  const PRIMARY_SELECTOR = "[data-collection-cell-primary]";
  const TEXT_ENTRY_SELECTOR = "textarea, input:not([type='checkbox']):not([type='radio']):not([type='button']):not([type='submit'])";
  const INTERACTIVE_SELECTOR = "a, button, input, label, select, textarea, [contenteditable='true'], [role='dialog'], [role='menu'], [role='listbox']";

  /** Focus the primary text field when a click lands on cell space outside any control. */
  function focusPrimaryField(event: MouseEvent & { currentTarget: EventTarget & HTMLDivElement }): void {
    onclick?.(event);
    if (event.defaultPrevented || event.button !== 0 || !(event.target instanceof Element)) return;
    if (event.target.closest(INTERACTIVE_SELECTOR)) return;
    const primary = event.currentTarget.querySelector<HTMLElement>(PRIMARY_SELECTOR);
    if (primary?.matches(TEXT_ENTRY_SELECTOR) && !primary.matches(":disabled")) primary.focus();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div {...attributes} class={cn("collection-cell relative flex min-h-(--collection-table-row-height) min-w-0 items-center self-stretch px-2 py-0.5", className)} onclick={focusPrimaryField}>
  {@render children()}
</div>

<style>
  /* Controls disabled during a save keep full opacity; controls revealed on row hover keep their own visibility. */
  .collection-cell :global(:disabled:not([data-collection-hover-reveal])) { opacity: 1; }
  .collection-cell :global(input:disabled) { -webkit-text-fill-color: currentColor; }
  /* The cell is the only highlighted surface (see the collection rules in app.css); the primary control and its wrappers stay unpositioned so its hit area resolves against the cell. */
  .collection-cell :global(:has([data-collection-cell-primary])),
  .collection-cell :global([data-collection-cell-primary]) {
    position: static;
  }
  .collection-cell :global([data-collection-cell-primary]) { background-color: transparent; }
  .collection-cell :global(:is(button, label)[data-collection-cell-primary])::after {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    content: "";
  }
  .collection-cell:has(:global(:is(button, label)[data-collection-cell-primary]:not(:disabled))) { cursor: pointer; }
  .collection-cell :global(input:not([type="checkbox"]):not([type="radio"]):focus) {
    outline: none;
    box-shadow: none;
    border-color: transparent;
  }
  .collection-cell :global(button:focus-visible) { outline: none; box-shadow: none; }
</style>
