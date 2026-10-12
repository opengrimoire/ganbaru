/** Where a property draft header sits relative to its anchor: over its start, or just before or after it. */
export type CollectionPropertyDraftPlacement = "start" | "before" | "after";

export interface CollectionPropertyDraftHeaderInput {
  /** The header cell the draft is drawn from, such as the add cell or the column an insert starts from. */
  anchorRect: { top: number; bottom: number; left: number; right: number };
  placement: CollectionPropertyDraftPlacement;
  viewportWidth: number;
  preferredWidth: number;
  margin?: number;
}

export interface CollectionPropertyDraftHeaderBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Place a draft column header on the anchor's header row at the column boundary where the property will appear.
 * It keeps the row's height and shifts horizontally only as far as needed to stay inside the viewport.
 */
export function collectionPropertyDraftHeaderBox(input: CollectionPropertyDraftHeaderInput): CollectionPropertyDraftHeaderBox {
  const margin = input.margin ?? 8;
  const width = Math.max(0, Math.min(input.preferredWidth, input.viewportWidth - margin * 2));
  const left = input.placement === "before"
    ? input.anchorRect.left - width
    : input.placement === "after" ? input.anchorRect.right : input.anchorRect.left;
  const maxLeft = Math.max(margin, input.viewportWidth - margin - width);
  return {
    left: Math.round(Math.min(Math.max(left, margin), maxLeft)),
    top: Math.round(input.anchorRect.top),
    width: Math.round(width),
    height: Math.round(Math.max(0, input.anchorRect.bottom - input.anchorRect.top)),
  };
}
