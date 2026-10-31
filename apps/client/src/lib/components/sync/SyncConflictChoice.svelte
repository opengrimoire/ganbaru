<script lang="ts">
  import type { Snippet } from "svelte";
  import Loading from "$lib/components/ui/Loading.svelte";

  /**
   * A quiet choice button for a sync conflict card or a stored value that needs a decision. It keeps the shared
   * panel row height as its touch target, and a pending choice keeps its label with the shared spinner before it.
   */
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
  class="inline-flex min-h-(--panel-row-height) items-center gap-1 rounded-md px-1.5 font-medium underline hover:bg-black/10 disabled:opacity-40 dark:hover:bg-white/10"
  type="button"
  disabled={disabled || pending}
  aria-busy={pending || undefined}
  {onclick}
>{#if pending}<Loading layout="inline" />{/if}{@render children()}</button>
