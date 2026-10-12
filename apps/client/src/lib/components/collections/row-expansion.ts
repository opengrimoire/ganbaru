/** Which rows of a nested collection are explicitly collapsed or opened. */
export interface CollectionRowExpansion {
  /** Rows with children the user closed. */
  collapsedIds: ReadonlySet<string>;
  /** Rows the user opened, kept so a row without children can show its add row. */
  openedIds: ReadonlySet<string>;
}

/**
 * Whether a row shows its nested rows. A row with children is open unless collapsed; a row without children is closed
 * unless opened, so an add row appears only under rows the user is working in.
 */
export function collectionRowExpanded(id: string, hasChildren: boolean, expansion: CollectionRowExpansion): boolean {
  return hasChildren ? !expansion.collapsedIds.has(id) : expansion.openedIds.has(id);
}

/** The expansion after the user toggles one row. Only rows with children are recorded as collapsed. */
export function toggledCollectionRowExpansion(
  id: string,
  hasChildren: boolean,
  expansion: CollectionRowExpansion,
): CollectionRowExpansion {
  const collapsedIds = new Set(expansion.collapsedIds);
  const openedIds = new Set(expansion.openedIds);
  if (collectionRowExpanded(id, hasChildren, expansion)) {
    openedIds.delete(id);
    if (hasChildren) collapsedIds.add(id);
  } else {
    collapsedIds.delete(id);
    openedIds.add(id);
  }
  return { collapsedIds, openedIds };
}

/** Start inset added to a nested row's title for each level of depth. */
export const COLLECTION_ROW_INDENT_REM = 0.75;

/** The title inset of a row at the given depth, as a CSS length. */
export function collectionRowIndent(depth: number): string {
  return `${Math.max(0, depth) * COLLECTION_ROW_INDENT_REM}rem`;
}
