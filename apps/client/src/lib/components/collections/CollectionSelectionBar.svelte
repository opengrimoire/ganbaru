<script lang="ts">
  import type { Snippet } from "svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";

  /**
   * The floating toolbar a collection shows while rows are selected: the count, then one segment per action, such as a
   * property to set on every selected row, the trash action, and a menu of further actions. The owner places it over the
   * area above its header so selecting rows never shifts the rows. Escape clears the selection unless a text field or a
   * floating surface owns the key. The bar stays one row tall; an editor that needs more room opens from a segment as a menu.
   */
  let { count, onClear, class: className = "", children }: {
    count: number;
    onClear: () => void;
    /** Placement classes from the owner, such as an absolute position over its toolbar. */
    class?: string;
    children: Snippet;
  } = $props();

  const { t } = getLocalization();

  /** Whether a key typed into this element edits text instead of acting on the selection. */
  function editsText(target: EventTarget | null): boolean {
    return target instanceof Element
      && target.matches("textarea, select, [contenteditable]:not([contenteditable='false']), input:not([type='checkbox']):not([type='radio']):not([type='button'])");
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.defaultPrevented || editsText(event.target)) return;
    if (event.target instanceof Element && event.target.closest("[data-app-floating-surface]")) return;
    event.preventDefault();
    onClear();
  }
</script>

<svelte:document onkeydown={keydown} />

<!-- The inline width cap keeps the bar inside its collection, which can be narrower than the viewport cap of floating surfaces. -->
<div class={cn("surface-floating flex min-w-0 overflow-hidden", className)} style="max-width: 100%;"
  role="toolbar" aria-label={t("collections.selection.selected", count)}>
  <div class="flex min-w-0 items-stretch divide-x divide-border overflow-x-auto scrollbar-none">
    <span class="flex min-h-8 shrink-0 items-center px-2.5 font-medium whitespace-nowrap text-primary">{t("collections.selection.selected", count)}</span>
    {@render children()}
  </div>
</div>
