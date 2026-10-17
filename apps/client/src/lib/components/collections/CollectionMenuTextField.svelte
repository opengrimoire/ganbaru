<script lang="ts">
  import { untrack } from "svelte";

  /**
   * A text field inside a menu that saves on Enter, on blur, and when its menu closes, since a closing menu can remove
   * the field without blurring it first. The draft follows the saved value until someone edits it. A multiline field
   * saves on Ctrl or Cmd with Enter so Enter can start a new line.
   */
  let { label, value, placeholder, multiline = false, monospace = false, hideLabel = false, initialFocus = false, disabled = false, onCommit }: {
    label: string;
    value: string;
    placeholder?: string;
    multiline?: boolean;
    monospace?: boolean;
    /** Name the field for assistive technology only, such as when the menu title already names it. */
    hideLabel?: boolean;
    /** Focus this field when its menu opens. */
    initialFocus?: boolean;
    disabled?: boolean;
    /** Save a draft that differs from the saved value, ignoring surrounding spaces. */
    onCommit: (value: string) => void;
  } = $props();

  let draft = $state(untrack(() => value));
  let editing = $state(false);
  /** The last draft handed to `onCommit`, so closing the menu does not resend a draft that was rejected. */
  let sent: string | null = null;

  $effect(() => {
    const saved = value;
    untrack(() => { if (!editing) draft = saved; });
  });

  $effect(() => () => untrack(commit));

  function commit(): void {
    if (disabled || draft === sent || draft.trim() === value.trim()) return;
    sent = draft;
    onCommit(draft);
  }

  function keydown(event: KeyboardEvent): void {
    event.stopPropagation();
    if (event.key !== "Enter" || event.isComposing || (multiline && !event.ctrlKey && !event.metaKey)) return;
    event.preventDefault();
    commit();
  }
</script>

<label class="grid min-w-0 gap-0 pb-1">
  <span class={hideLabel ? "sr-only" : "menu-label px-1"}>{label}</span>
  {#if multiline}
    <textarea class={["field min-h-20 w-full min-w-0 resize-y", monospace && "font-mono"]} {placeholder} {disabled} data-collection-menu-initial-focus={initialFocus ? "" : undefined}
      bind:value={draft} onfocus={() => { editing = true; }} onblur={() => { editing = false; commit(); }} onkeydown={keydown}></textarea>
  {:else}
    <input class={["field w-full min-w-0", monospace && "font-mono"]} {placeholder} {disabled} data-collection-menu-initial-focus={initialFocus ? "" : undefined}
      bind:value={draft} onfocus={() => { editing = true; }} onblur={() => { editing = false; commit(); }} onkeydown={keydown} />
  {/if}
</label>
