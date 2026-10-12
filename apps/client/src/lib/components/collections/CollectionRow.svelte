<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLAttributes } from "svelte/elements";
  import { cn } from "$lib/utils";
  import { COLLECTION_ROW_INSET, collectionRowContentStart } from "./row-expansion";

  let { template, compactTemplate, minWidth, gutter, header = false, selected = false, divider = true, dividerTop = false, class: className, style: styleText, children, ...attributes }: HTMLAttributes<HTMLDivElement> & {
    template: string;
    compactTemplate?: string;
    minWidth?: string;
    /**
     * Width of the leading tracks that sit outside the table, such as the selection checkbox and expand chevron.
     * Dividers, the selected fill, and the hover highlight start after it.
     */
    gutter?: string;
    header?: boolean;
    selected?: boolean;
    divider?: boolean;
    /** Draws a divider above the row too, for the first row of a table. */
    dividerTop?: boolean;
    children: Snippet;
  } = $props();
</script>

<div
  {...attributes}
  class={cn("collection-row group/row grid min-h-(--collection-table-row-height) items-center text-collection", header && "font-normal text-muted-foreground", className)}
  data-collection-hoverable={header ? undefined : ""}
  data-collection-selected={selected ? "" : undefined}
  data-collection-divider={divider ? "" : undefined}
  data-collection-divider-top={dividerTop ? "" : undefined}
  data-compact={compactTemplate ? "true" : undefined}
  style={`--collection-columns: ${template}; --collection-compact-columns: ${compactTemplate ?? template}; --collection-row-content-start: ${gutter ? collectionRowContentStart(gutter) : "0px"}; padding-inline: ${COLLECTION_ROW_INSET}; ${minWidth ? `min-width: ${minWidth};` : ""} ${styleText ?? ""}`}
>
  {@render children()}
</div>

<style>
  /* Fills and dividers start at the content start, so a gutter reads as outside the table; dividers take no layout space, so every row keeps the shared row height. */
  .collection-row {
    position: relative;
    grid-template-columns: var(--collection-columns);
    background-image:
      linear-gradient(to right, transparent var(--collection-row-content-start), var(--collection-row-hover, transparent) var(--collection-row-content-start)),
      linear-gradient(to right, transparent var(--collection-row-content-start), var(--collection-row-fill, transparent) var(--collection-row-content-start));
  }
  .collection-row[data-collection-selected] { --collection-row-fill: color-mix(in srgb, var(--accent) 40%, transparent); }
  .collection-row[data-collection-divider]::after,
  .collection-row[data-collection-divider-top]::before {
    position: absolute;
    z-index: var(--collection-divider-layer, auto);
    right: 0;
    left: var(--collection-row-content-start);
    border-top: 1px solid var(--cal-gridline);
    content: "";
    pointer-events: none;
  }
  .collection-row[data-collection-divider]::after { bottom: 0; }
  .collection-row[data-collection-divider-top]::before { top: 0; }
  @container (max-width: 32rem) {
    .collection-row[data-compact="true"] { grid-template-columns: var(--collection-compact-columns); }
    .collection-row[data-compact="true"] > :global([data-collection-secondary]) { display: none; }
  }
</style>
