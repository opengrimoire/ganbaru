import { CollectionColumnReorder, type CollectionColumnReorderOptions } from "$lib/components/collections/column-reorder.svelte";
import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";

/** Header cells before the list columns: the fixed name column leads every Projects header row. */
const LEADING_HEADER_COLUMNS = 1;

/**
 * Column header drag for a Projects table. It saves the list column order through the query controller, which
 * shows the new order right away and rolls it back when the save fails.
 */
export function createProjectListColumnReorder(
  query: ProjectTaskQueryController,
  options: { resizing?: () => boolean; scroller?: CollectionColumnReorderOptions["scroller"] } = {},
): CollectionColumnReorder {
  return new CollectionColumnReorder({
    canStart: () => !query.listColumnsSaving && !query.propertySaving && !options.resizing?.(),
    onReorder: (columnId, targetIndex) => {
      const column = query.listColumns.find((candidate) => candidate === columnId);
      if (column) void query.placeColumn(column, targetIndex - LEADING_HEADER_COLUMNS);
    },
    scroller: options.scroller,
  });
}
