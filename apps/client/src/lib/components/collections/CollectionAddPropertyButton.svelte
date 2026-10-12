<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /** The square add control after a collection table's last column; it toggles the draft property header. */
  let { open, disabled = false, element = $bindable(), onclick }: {
    /** Whether the draft it opens is showing. */
    open: boolean;
    disabled?: boolean;
    element?: HTMLButtonElement;
    onclick: () => void;
  } = $props();

  const { t } = getLocalization();
</script>

<button bind:this={element} type="button" data-collection-hover-target {disabled} {onclick}
  class={cn("inline-flex size-9 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:bg-accent focus-visible:outline-none disabled:cursor-not-allowed", open && "bg-accent text-foreground")}
  aria-label={t("collections.property.add")} aria-haspopup="dialog" aria-expanded={open}>
  <Plus class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
</button>
