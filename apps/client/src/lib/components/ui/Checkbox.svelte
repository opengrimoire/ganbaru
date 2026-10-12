<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Minus from "@lucide/svelte/icons/minus";
  import type { HTMLInputAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";

  /**
   * The app's one checkbox: a native checkbox input, so labels, forms, and keyboard behave natively, drawn with theme tokens.
   * Bind `checked` for local state, or pass `onChange` to keep the value owned by the parent.
   */
  let {
    checked = $bindable(false),
    indeterminate = false,
    disabled = false,
    label,
    onChange,
    class: className,
    ...attributes
  }: Omit<HTMLInputAttributes, "type" | "checked" | "class" | "onchange"> & {
    checked?: boolean;
    /** Shows a mixed state, for a parent whose children are partly selected. */
    indeterminate?: boolean;
    disabled?: boolean;
    /** Accessible name; omit it when a wrapping `<label>` names the checkbox. */
    label?: string;
    /** Receives the next state; the checkbox then shows the parent's value instead of toggling itself. */
    onChange?: (checked: boolean) => void;
    class?: string;
  } = $props();

  let input: HTMLInputElement | null = $state(null);
  const marked = $derived(checked || indeterminate);

  $effect(() => {
    if (input) input.indeterminate = indeterminate;
  });
</script>

<span class={cn("relative inline-grid size-4 shrink-0 place-items-center pointer-coarse:size-5", className)}>
  <input
    {...attributes}
    bind:this={input}
    type="checkbox"
    class="peer absolute inset-0 m-0 size-full cursor-pointer appearance-none opacity-0 disabled:cursor-not-allowed"
    {checked}
    {disabled}
    aria-label={label}
    data-app-tooltip-disabled="true"
    onchange={(event) => {
      const next = event.currentTarget.checked;
      if (onChange) {
        event.currentTarget.checked = checked;
        onChange(next);
      } else {
        checked = next;
      }
    }}
  />
  <span
    aria-hidden="true"
    class={cn(
      "pointer-events-none grid size-full place-items-center rounded-sm border transition-colors peer-focus-visible:outline-2 peer-focus-visible:outline-offset-2 peer-focus-visible:outline-ring peer-disabled:opacity-50",
      marked ? "border-primary bg-primary text-primary-foreground" : "border-foreground/25 text-transparent peer-hover:border-foreground/45",
    )}
  >
    {#if indeterminate}
      <Minus size={11} strokeWidth={2.5} />
    {:else if checked}
      <Check size={11} strokeWidth={2.5} />
    {/if}
  </span>
</span>
