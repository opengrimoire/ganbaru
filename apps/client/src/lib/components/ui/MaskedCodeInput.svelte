<script lang="ts">
  import X from "@lucide/svelte/icons/x";
  import type { HTMLInputAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";
  import { maskCode } from "$lib/utils/masked-code";

  /**
   * A text field for a secret code that shows a pasted code only by its ends (see {@link maskCode}),
   * so the user can compare it with its source without the whole code on screen. A pasted code
   * replaces the field's text. While masked, deleting clears the code and typing starts over with
   * the typed text; a typed code stays visible, so it can be checked character by character.
   */
  let {
    value = $bindable(""),
    input = $bindable(null),
    clearLabel,
    oninput,
    class: className,
    ...attributes
  }: Omit<HTMLInputAttributes, "type" | "value" | "class" | "oninput" | "onpaste" | "onbeforeinput"> & {
    value?: string;
    /** The input element, for focusing it. */
    input?: HTMLInputElement | null;
    /** Accessible name of the button that clears a masked code. */
    clearLabel: string;
    /** Runs after the code changes by typing, pasting, or clearing. */
    oninput?: () => void;
    class?: string;
  } = $props();

  /** The last pasted code; the field is masked while it still holds exactly that code. */
  let pastedValue = $state<string | null>(null);
  const masked = $derived(value !== "" && value === pastedValue);

  function replace(next: string, pasted: boolean): void {
    value = next;
    pastedValue = pasted ? next : null;
    oninput?.();
  }

  function paste(event: ClipboardEvent): void {
    const text = event.clipboardData?.getData("text/plain").trim() ?? "";
    if (text === "") return;
    event.preventDefault();
    replace(text, true);
  }

  function beforeInput(event: InputEvent): void {
    if (!masked) return;
    event.preventDefault();
    if (event.inputType.startsWith("delete")) replace("", false);
    else if (event.inputType === "insertText") replace(event.data ?? "", false);
    else if (event.inputType === "insertFromDrop") {
      const text = event.dataTransfer?.getData("text/plain").trim() ?? "";
      if (text !== "") replace(text, true);
    }
  }

  function typed(event: Event & { currentTarget: HTMLInputElement }): void {
    // An input that `beforeinput` could not cancel, such as IME composition, edits the mask itself,
    // so it starts the code over instead of keeping mask characters.
    replace(masked ? "" : event.currentTarget.value, false);
  }

  function clear(): void {
    replace("", false);
    input?.focus();
  }
</script>

<div class="relative">
  <input
    {...attributes}
    bind:this={input}
    type="text"
    value={masked ? maskCode(value) : value}
    class={cn(className, masked && "pe-9 pointer-coarse:pe-11")}
    data-masked={masked ? "" : undefined}
    onpaste={paste}
    onbeforeinput={beforeInput}
    oninput={typed}
  />
  {#if masked}
    <button
      type="button"
      class="absolute inset-y-0 end-1 my-auto flex size-6 pointer-coarse:size-9 items-center justify-center rounded-sm text-muted-foreground hover:bg-muted hover:text-foreground"
      title={clearLabel}
      aria-label={clearLabel}
      disabled={attributes.disabled}
      onclick={clear}
    >
      <X size={14} aria-hidden="true" />
    </button>
  {/if}
</div>
