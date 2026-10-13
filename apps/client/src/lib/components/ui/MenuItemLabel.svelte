<script lang="ts">
  /**
   * Truncating label for a picker or menu row. A starred item shows a small favorite mark right after its name, so a
   * list that puts favorites first still reads as ordered.
   */
  import Star from "@lucide/svelte/icons/star";
  import { cn } from "$lib/utils";

  let {
    label,
    starred = false,
    class: className,
  }: {
    label: string;
    starred?: boolean;
    class?: string;
  } = $props();
</script>

<span class={cn("flex min-w-0 flex-1 items-center gap-1", className)}>
  <span class="min-w-0 truncate">{label}</span>
  {#if starred}<Star class="menu-item-star size-2.5 shrink-0 fill-current text-primary" aria-hidden="true" />{/if}
</span>
