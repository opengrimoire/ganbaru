import {
  collectionRowExpanded,
  toggledCollectionRowExpansion,
  type CollectionRowExpansion,
} from "$lib/components/collections/row-expansion";
import type { ProjectTask } from "$lib/projects/types";

export type ProjectListTaskMenu = "status" | "priority" | "start" | "due";

export interface ProjectListInteractionControllerContext {
  getSelectedTaskIds: () => string[];
  selectedTaskIdsChanged: (taskIds: string[]) => void;
}

export class ProjectListInteractionController {
  sectionOptionsMenuId = $state<string | null>(null);
  statusMenuTaskId = $state<string | null>(null);
  priorityMenuTaskId = $state<string | null>(null);
  startDateMenuTaskId = $state<string | null>(null);
  dueDateMenuTaskId = $state<string | null>(null);
  private expansion = $state<CollectionRowExpansion>({ collapsedIds: new Set(), openedIds: new Set() });

  constructor(private readonly context: ProjectListInteractionControllerContext) {}

  /** Whether a task shows its subtasks and the add-subtask row. */
  taskExpanded(task: ProjectTask, hasSubtasks: boolean): boolean {
    return collectionRowExpanded(task.id, hasSubtasks, this.expansion);
  }

  toggleTaskExpanded(task: ProjectTask, hasSubtasks: boolean): void {
    this.expansion = toggledCollectionRowExpansion(task.id, hasSubtasks, this.expansion);
  }

  private selectedIds(): Set<string> {
    return new Set(this.context.getSelectedTaskIds());
  }

  taskSelected(task: ProjectTask): boolean {
    return this.selectedIds().has(task.id);
  }

  allTasksSelected(tasks: ProjectTask[]): boolean {
    const selected = this.selectedIds();
    return tasks.length > 0 && tasks.every((task) => selected.has(task.id));
  }

  someTasksSelected(tasks: ProjectTask[]): boolean {
    const selected = this.selectedIds();
    return tasks.some((task) => selected.has(task.id));
  }

  toggleTaskSelection = (task: ProjectTask): void => {
    const current = this.context.getSelectedTaskIds();
    this.context.selectedTaskIdsChanged(
      current.includes(task.id)
        ? current.filter((id) => id !== task.id)
        : [...current, task.id],
    );
  };

  toggleTaskGroupSelection(tasks: ProjectTask[]): void {
    if (tasks.length === 0) return;
    const next = this.selectedIds();
    if (this.allTasksSelected(tasks)) {
      for (const task of tasks) next.delete(task.id);
    } else {
      for (const task of tasks) next.add(task.id);
    }
    this.context.selectedTaskIdsChanged(Array.from(next));
  }

  closeTaskMenu(menu: ProjectListTaskMenu): void {
    this.setTaskMenuId(menu, null);
  }

  toggleTaskMenu(menu: ProjectListTaskMenu, taskId: string): void {
    const nextId = this.taskMenuId(menu) === taskId ? null : taskId;
    this.statusMenuTaskId = menu === "status" ? nextId : null;
    this.priorityMenuTaskId = menu === "priority" ? nextId : null;
    this.startDateMenuTaskId = menu === "start" ? nextId : null;
    this.dueDateMenuTaskId = menu === "due" ? nextId : null;
  }

  handleOutsidePointerTarget(target: Element): void {
    if (this.sectionOptionsMenuId && !target.closest("[data-section-options-root='true']")) {
      this.sectionOptionsMenuId = null;
    }
    if (this.statusMenuTaskId && !target.closest("[data-list-status-menu-root='true']")) {
      this.statusMenuTaskId = null;
    }
    if (this.priorityMenuTaskId && !target.closest("[data-list-priority-menu-root='true']")) {
      this.priorityMenuTaskId = null;
    }
    if ((this.startDateMenuTaskId || this.dueDateMenuTaskId) && !target.closest("[data-list-date-menu-root='true']")) {
      this.startDateMenuTaskId = null;
      this.dueDateMenuTaskId = null;
    }
  }

  private taskMenuId(menu: ProjectListTaskMenu): string | null {
    if (menu === "status") return this.statusMenuTaskId;
    if (menu === "priority") return this.priorityMenuTaskId;
    if (menu === "start") return this.startDateMenuTaskId;
    return this.dueDateMenuTaskId;
  }

  private setTaskMenuId(menu: ProjectListTaskMenu, taskId: string | null): void {
    if (menu === "status") this.statusMenuTaskId = taskId;
    if (menu === "priority") this.priorityMenuTaskId = taskId;
    if (menu === "start") this.startDateMenuTaskId = taskId;
    if (menu === "due") this.dueDateMenuTaskId = taskId;
  }
}
