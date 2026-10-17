<script lang="ts">
  import type { Snippet } from "svelte";
  import CollectionCell from "./CollectionCell.svelte";
  import { COLLECTION_COLUMN_ID_ATTRIBUTE, COLLECTION_COLUMN_MOVABLE_ATTRIBUTE, type CollectionColumnReorder } from "./column-reorder.svelte";

  let { label, resizeLabel, disabled = false, icon, actions, class: className = "", style, columnId, movable = true, reorder, reorderRow = "header", onpointerdown, ondblclick, onkeydown }: {
    label: string;
    resizeLabel: string;
    disabled?: boolean;
    icon?: Snippet;
    actions?: Snippet;
    class?: string;
    style?: string;
    /** Column identity used to reorder by dragging this header; required with `reorder`. */
    columnId?: string;
    /** Whether this column can be dragged; fixed columns still count as places the others move around. */
    movable?: boolean;
    reorder?: CollectionColumnReorder;
    /** Distinguishes header rows that share one reorder controller, such as one header per section. */
    reorderRow?: string;
    onpointerdown: (event: PointerEvent) => void;
    ondblclick?: (event: MouseEvent) => void;
    onkeydown: (event: KeyboardEvent) => void;
  } = $props();

  const reorderId = $derived(reorder && columnId !== undefined ? columnId : null);
  const dragged = $derived(reorderId !== null && Boolean(reorder?.dragged(reorderRow, reorderId)));
  const shifting = $derived(reorderId !== null && Boolean(reorder?.shifting(reorderRow, reorderId)));
  const cellStyle = $derived([style, reorderId !== null ? reorder?.cellStyle(reorderRow, reorderId) : ""].filter(Boolean).join("; ") || undefined);
  const reorderAttributes = $derived(reorderId === null ? {} : {
    [COLLECTION_COLUMN_ID_ATTRIBUTE]: reorderId,
    [COLLECTION_COLUMN_MOVABLE_ATTRIBUTE]: movable ? "" : undefined,
    "data-collection-column-dragged": dragged ? "" : undefined,
    "data-collection-column-shifting": shifting ? "" : undefined,
  });

  function startReorder(event: PointerEvent): void {
    if (reorder && reorderId !== null && movable) reorder.pointerDown(event, reorderRow, reorderId);
  }
</script>

<CollectionCell role="columnheader" class={`collection-column-header ${actions ? "items-stretch gap-0 p-0" : "gap-1.5"} ${className}`} style={cellStyle}
  {...reorderAttributes} onpointerdown={startReorder}>
  {#if actions}
    {@render actions()}
  {:else}
    {@render icon?.()}
    <span class="relative z-10 truncate">{label}</span>
  {/if}
  <button type="button" class="collection-resize" aria-label={resizeLabel} data-app-tooltip-disabled="true" data-collection-resize="" {disabled} {onpointerdown} {ondblclick} {onkeydown}></button>
</CollectionCell>

<style>
  :global(.collection-column-header[data-collection-column-shifting]) { transition: transform 150ms ease; }
  :global(.collection-column-header[data-collection-column-dragged]) {
    background-color: var(--collection-surface, var(--background));
    background-image: linear-gradient(var(--collection-highlight), var(--collection-highlight));
    cursor: grabbing;
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.collection-column-header[data-collection-column-shifting]) { transition: none; }
  }
  .collection-resize {
    position: absolute;
    top: 0.375rem;
    right: -0.375rem;
    bottom: 0.375rem;
    z-index: 20;
    width: 0.75rem;
    border: 0;
    background: transparent;
    cursor: col-resize;
    touch-action: none;
    padding: 0;
  }
  .collection-resize::after {
    position: absolute;
    inset-block: 0;
    left: 50%;
    width: 0.25rem;
    transform: translateX(-50%);
    border-radius: 9999px;
    background: var(--primary);
    content: "";
    opacity: 0;
  }
  .collection-resize:hover::after,
  .collection-resize:focus-visible::after,
  .collection-resize:active::after { opacity: 1; }
  :global([data-collection-column-dragging]) .collection-resize::after { opacity: 0; }
</style>
