import { getContext, setContext } from "svelte";
import type { ProjectTask } from "$lib/projects/types";
import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
import type { CollectionColumnReorder } from "$lib/components/collections/column-reorder.svelte";

export interface ProjectListTableContext {
  query: ProjectTaskQueryController;
  /** Width of the selection and expand tracks outside the table, passed to rows so fills skip it. */
  gutter: string;
  cellClass: (column: ProjectTaskListResizableColumn | "selection" | "expand") => string;
  cellStyle: (column: ProjectTaskListResizableColumn | "selection" | "expand") => string;
  rowStyle: (task: ProjectTask, selected: boolean) => string;
  /** Drags column headers to reorder the list columns; every header row of the table shares it. */
  columnReorder?: CollectionColumnReorder;
}
const PROJECT_LIST_TABLE_CONTEXT = Symbol("project-list-table");

/** Share table presentation without coupling task editing or section composition to it. */
export function setProjectListTableContext(context: ProjectListTableContext): void {
  setContext(PROJECT_LIST_TABLE_CONTEXT, context);
}

/** Resolve presentation only when a component belongs to a configured Projects table. */
export function getProjectListTableContext(): ProjectListTableContext | undefined {
  return getContext<ProjectListTableContext | undefined>(PROJECT_LIST_TABLE_CONTEXT);
}
