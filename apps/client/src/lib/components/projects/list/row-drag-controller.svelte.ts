import {
  projectListDropPositionFromPoint,
  projectListDropSortOrder,
  projectListPointerDragGestureReady,
  projectListSectionDragAllowed,
  projectListSectionDropAllowed,
  projectListSectionDropSortOrder,
  projectListTaskDragAllowed,
  projectListTaskDropAllowed,
  type ProjectListDropPosition,
  type ProjectListPointerDragGesture,
} from "$lib/projects/list/drag";
import type {
  ProjectSection,
  ProjectTask,
  ProjectTaskGroupMode,
  ProjectTaskSortDirection,
  ProjectTaskSortMode,
} from "$lib/projects/types";

const TASK_DRAG_MIME = "application/x-ganbaru-project-list-task";
const SECTION_DRAG_MIME = "application/x-ganbaru-project-list-section";

export interface ProjectListRowDragControllerContext {
  getTasks: () => ProjectTask[];
  getAllTasks: () => ProjectTask[];
  getSections: () => ProjectSection[];
  getGroupBy: () => ProjectTaskGroupMode;
  getSortMode: () => ProjectTaskSortMode;
  getSortDirection: () => ProjectTaskSortDirection;
  updateTask: (task: ProjectTask, patch: Partial<ProjectTask>) => Promise<unknown>;
  updateSection: (section: ProjectSection, patch: Partial<ProjectSection>) => Promise<unknown>;
}

export class ProjectListRowDragController {
  draggingTaskId = $state<string | null>(null);
  dragOverSectionId = $state<string | null>(null);
  dragOverTaskId = $state<string | null>(null);
  dragOverPosition = $state<ProjectListDropPosition | "section" | null>(null);
  dropPendingTaskId = $state<string | null>(null);
  rowGesture = $state<ProjectListPointerDragGesture | null>(null);
  draggingSectionId = $state<string | null>(null);
  sectionDragOverId = $state<string | null>(null);
  sectionDragOverPosition = $state<ProjectListDropPosition | null>(null);
  sectionDropPendingId = $state<string | null>(null);
  sectionGesture = $state<ProjectListPointerDragGesture | null>(null);
  suppressedTaskOpenId = $state<string | null>(null);

  constructor(private readonly context: ProjectListRowDragControllerContext) {}

  tasksForSection(section: ProjectSection): ProjectTask[] {
    return this.context.getTasks().filter((task) => task.sectionId === section.id && !task.parentTaskId);
  }

  canStartTask = (task: ProjectTask): boolean => projectListTaskDragAllowed({
    dragEnabled: this.context.getGroupBy() === "section" && this.context.getSortMode() === "manual",
    task,
    dropPendingTaskId: this.dropPendingTaskId,
  });

  canStartSection(section: ProjectSection): boolean {
    return projectListSectionDragAllowed({
      dragEnabled: this.context.getGroupBy() === "section",
      section,
      dropPendingSectionId: this.sectionDropPendingId,
    });
  }

  handleTaskPointerDown = (event: PointerEvent, task: ProjectTask): void => {
    if (event.button !== 0 || !this.canStartTask(task) || !this.taskTargetAllowed(event.target)) {
      this.rowGesture = null;
      return;
    }
    this.rowGesture = this.gesture(task.id, event);
  };

  clearTaskGesture = (event?: PointerEvent): void => {
    if (!event || !this.rowGesture || event.pointerId === this.rowGesture.pointerId) this.rowGesture = null;
  };

  handleSectionPointerDown = (event: PointerEvent, section: ProjectSection): void => {
    if (event.button !== 0 || !this.canStartSection(section) || !this.sectionTargetAllowed(event.target)) {
      this.sectionGesture = null;
      return;
    }
    this.sectionGesture = this.gesture(section.id, event);
  };

  clearSectionGesture = (event?: PointerEvent): void => {
    if (!event || !this.sectionGesture || event.pointerId === this.sectionGesture.pointerId) {
      this.sectionGesture = null;
    }
  };

  handleTaskDragStart = (event: DragEvent, task: ProjectTask): void => {
    if (!this.canStartTask(task) || !this.gestureReady(event, task.id, this.rowGesture)) {
      event.preventDefault();
      return;
    }
    event.stopPropagation();
    this.draggingTaskId = task.id;
    this.rowGesture = null;
    this.suppressNextOpen(task.id);
    event.dataTransfer?.setData(TASK_DRAG_MIME, task.id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  };

  handleSectionDragStart = (event: DragEvent, section: ProjectSection): void => {
    if (!this.canStartSection(section) || !this.gestureReady(event, section.id, this.sectionGesture)) {
      event.preventDefault();
      return;
    }
    event.stopPropagation();
    this.draggingSectionId = section.id;
    this.sectionGesture = null;
    this.resetTaskTarget();
    event.dataTransfer?.setData(SECTION_DRAG_MIME, section.id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  };

  handleTaskDragEnd = (): void => {
    this.draggingTaskId = null;
    this.dropPendingTaskId = null;
    this.rowGesture = null;
    this.resetTaskTarget();
  };

  handleSectionDragEnd = (): void => {
    this.draggingSectionId = null;
    this.sectionDropPendingId = null;
    this.sectionGesture = null;
    this.resetSectionTarget();
  };

  handleTaskDragOver = (event: DragEvent, section: ProjectSection, task: ProjectTask): void => {
    const dragged = this.taskById(this.taskDragId(event));
    if (!dragged || !this.canDropTask(dragged, section) || dragged.id === task.id) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    this.dragOverSectionId = section.id;
    this.dragOverTaskId = task.id;
    this.dragOverPosition = this.dropPosition(event);
  };

  handleSectionGroupDragOver = (event: DragEvent, section: ProjectSection): void => {
    const draggedSection = this.sectionById(this.sectionDragId(event));
    if (draggedSection && this.canDropSection(draggedSection, section) && draggedSection.id !== section.id) {
      event.preventDefault();
      event.stopPropagation();
      if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
      this.resetTaskTarget();
      this.sectionDragOverId = section.id;
      this.sectionDragOverPosition = this.dropPosition(event);
      return;
    }
    const draggedTask = this.taskById(this.taskDragId(event));
    if (!draggedTask || !this.canDropTask(draggedTask, section)) return;
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    this.dragOverSectionId = section.id;
    this.dragOverTaskId = null;
    this.dragOverPosition = "section";
  };

  dropTask = async (
    event: DragEvent,
    section: ProjectSection,
    targetTask?: ProjectTask,
    position?: ProjectListDropPosition,
  ): Promise<void> => {
    event.preventDefault();
    event.stopPropagation();
    const dragged = this.taskById(this.taskDragId(event));
    if (!dragged || !this.canDropTask(dragged, section) || dragged.id === targetTask?.id) {
      this.resetTaskTarget();
      return;
    }
    const nextOrder = projectListDropSortOrder({
      orderedTasks: this.tasksForSection(section),
      draggedTaskId: dragged.id,
      overTaskId: targetTask?.id,
      position: position ?? (targetTask ? this.dropPosition(event) : undefined),
      sortDirection: this.context.getSortDirection(),
    });
    if (dragged.sectionId === section.id && dragged.sectionSortOrder === nextOrder) {
      this.resetTaskTarget();
      return;
    }
    this.dropPendingTaskId = dragged.id;
    this.resetTaskTarget();
    try {
      await this.context.updateTask(dragged, { sectionId: section.id, sectionSortOrder: nextOrder });
    } finally {
      this.dropPendingTaskId = null;
      this.draggingTaskId = null;
    }
  };

  dropSectionOrTask = async (event: DragEvent, section: ProjectSection): Promise<void> => {
    const draggedSection = this.sectionById(this.sectionDragId(event));
    if (!draggedSection || !this.canDropSection(draggedSection, section)) {
      await this.dropTask(event, section);
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (draggedSection.id === section.id) {
      this.resetSectionTarget();
      return;
    }
    const nextOrder = projectListSectionDropSortOrder({
      orderedSections: this.context.getSections().filter((item) => !item.archivedAt && !item.hiddenAt),
      draggedSectionId: draggedSection.id,
      overSectionId: section.id,
      position: this.dropPosition(event),
    });
    if (draggedSection.sortOrder === nextOrder) {
      this.resetSectionTarget();
      return;
    }
    this.sectionDropPendingId = draggedSection.id;
    this.resetSectionTarget();
    try {
      await this.context.updateSection(draggedSection, { sortOrder: nextOrder });
    } finally {
      this.sectionDropPendingId = null;
      this.draggingSectionId = null;
    }
  };

  taskMarkerVisible(section: ProjectSection, task: ProjectTask, position: ProjectListDropPosition): boolean {
    return this.dragOverSectionId === section.id
      && this.dragOverTaskId === task.id
      && this.dragOverPosition === position;
  }

  sectionMarkerVisible(section: ProjectSection, position: ProjectListDropPosition): boolean {
    return this.sectionDragOverId === section.id && this.sectionDragOverPosition === position;
  }

  openTask(task: ProjectTask, open: (task: ProjectTask) => void): void {
    if (this.suppressedTaskOpenId === task.id) {
      this.suppressedTaskOpenId = null;
      return;
    }
    open(task);
  }

  private gesture(itemId: string, event: PointerEvent): ProjectListPointerDragGesture {
    return {
      itemId,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startedAt: Date.now(),
    };
  }

  private gestureReady(event: DragEvent, itemId: string, gesture: ProjectListPointerDragGesture | null): boolean {
    return projectListPointerDragGestureReady({
      gesture,
      itemId,
      clientX: event.clientX,
      clientY: event.clientY,
      now: Date.now(),
    });
  }

  private taskTargetAllowed(target: EventTarget | null): boolean {
    if (!(target instanceof Element) || target.closest("[data-list-row-drag-source='true']")) return true;
    return !target.closest("button, input, textarea, select, a, [role='button']");
  }

  private sectionTargetAllowed(target: EventTarget | null): boolean {
    if (!(target instanceof Element) || target.closest("[data-list-section-drag-source='true']")) return true;
    return !target.closest("button, textarea, select, a, [role='button']");
  }

  private taskById(id: string | null): ProjectTask | undefined {
    return id ? this.context.getAllTasks().find((task) => task.id === id) : undefined;
  }

  private sectionById(id: string | null): ProjectSection | undefined {
    return id ? this.context.getSections().find((section) => section.id === id) : undefined;
  }

  private taskDragId(event: DragEvent): string | null {
    return event.dataTransfer?.getData(TASK_DRAG_MIME) || this.draggingTaskId;
  }

  private sectionDragId(event: DragEvent): string | null {
    return event.dataTransfer?.getData(SECTION_DRAG_MIME) || this.draggingSectionId;
  }

  private canDropTask(task: ProjectTask | undefined, section: ProjectSection): boolean {
    return projectListTaskDropAllowed({
      dragEnabled: this.context.getGroupBy() === "section" && this.context.getSortMode() === "manual",
      task,
      targetSection: section,
    });
  }

  private canDropSection(dragged: ProjectSection | undefined, target: ProjectSection): boolean {
    return projectListSectionDropAllowed({
      dragEnabled: this.context.getGroupBy() === "section",
      draggedSection: dragged,
      targetSection: target,
    });
  }

  private dropPosition(event: DragEvent): ProjectListDropPosition {
    return projectListDropPositionFromPoint(
      event.clientY,
      (event.currentTarget as HTMLElement).getBoundingClientRect(),
    );
  }

  private suppressNextOpen(taskId: string): void {
    this.suppressedTaskOpenId = taskId;
    window.setTimeout(() => {
      if (this.suppressedTaskOpenId === taskId) this.suppressedTaskOpenId = null;
    }, 0);
  }

  private resetTaskTarget(): void {
    this.dragOverSectionId = null;
    this.dragOverTaskId = null;
    this.dragOverPosition = null;
  }

  private resetSectionTarget(): void {
    this.sectionDragOverId = null;
    this.sectionDragOverPosition = null;
  }
}
