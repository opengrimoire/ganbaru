<script lang="ts">
  import type { Snippet } from "svelte";
  import X from "@lucide/svelte/icons/x";
  import { getLocalization } from "$lib/i18n/translator.svelte";

  /**
   * The action bar a collection shows while rows are selected: the count, the collection's actions, and a way to clear
   * the selection. A footer holds anything that spans the bar below its actions, such as a form or an error.
   */
  let { count, onClear, children, footer }: {
    count: number;
    onClear: () => void;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  const { t } = getLocalization();
</script>

<div class="mx-3 my-2 flex min-w-0 flex-wrap items-center gap-1 rounded-md border border-border bg-card px-2 py-1 text-[0.766667rem]"
  role="toolbar" aria-label={t("collections.selection.selected", count)}>
  <span class="mr-1 shrink-0 font-medium">{t("collections.selection.selected", count)}</span>
  {@render children()}
  <button type="button"
    class="ml-auto flex h-7 shrink-0 items-center gap-1 rounded-md border border-border bg-card px-2 font-medium text-muted-foreground hover:bg-accent hover:text-foreground"
    onclick={onClear}>
    <X size={13} strokeWidth={1.75} />
    <span>{t("collections.selection.clear")}</span>
  </button>
  {@render footer?.()}
</div>
