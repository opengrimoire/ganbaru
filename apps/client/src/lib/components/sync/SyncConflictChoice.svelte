<script lang="ts">
  import type { Snippet } from "svelte";

  /** A quiet choice button for a sync conflict card or a stored value that needs a decision. */
  let {
    disabled = false,
    pending = false,
    onclick,
    children,
  }: {
    disabled?: boolean;
    /** Marks the choice whose action is running. */
    pending?: boolean;
    onclick: () => void;
    children: Snippet;
  } = $props();
</script>

<button
  class="rounded-md px-1.5 py-0.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
  type="button"
  disabled={disabled || pending}
  aria-busy={pending || undefined}
  {onclick}
>{@render children()}</button>
