import type { getProjects } from "$lib/stores/projects.svelte";
import {
  projectListGroupQuickAddPlan,
  projectListGroupTaskCreateTarget,
  projectListGroupTaskDraftKey,
  projectListSectionTaskCreateTarget,
  projectListSubtaskCreateTarget,
  type ProjectListGroupQuickAddPlan,
  type ProjectListTaskCreateTarget,
} from "$lib/projects/list/quick-add";
import type {
  ProjectPriorityConfig,
  ProjectStatus,
  ProjectTask,
  ProjectTaskGroupMode,
} from "$lib/projects/types";
import type { ProjectTaskListGroup } from "$lib/projects/tasks/view";

type ProjectsQuickAddCapability = Pick<
  ReturnType<typeof getProjects>,
  "addSection" | "addTask" | "taskById" | "updateTask"
>;

export interface ProjectListQuickAddControllerContext {
  projects: ProjectsQuickAddCapability;
  getSelectedProjectId: () => string | null;
  getGroupBy: () => ProjectTaskGroupMode;
  getStatuses: () => ProjectStatus[];
  getPriorities: () => ProjectPriorityConfig[];
  revealTask: (task: ProjectTask | undefined) => void;
  selectProjectFirstMessage: () => string;
  createFailedMessage: (error: unknown) => string;
}

/**
 * The add-subtask row under an expanded task. Drafts and errors are keyed by the parent task, and one row is active at
 * a time.
 */
export interface ProjectListSubtaskQuickAdd {
  readonly pending: boolean;
  subtaskDraft(parent: ProjectTask): string;
  subtaskActive(parent: ProjectTask): boolean;
  subtaskError(parent: ProjectTask): string | null;
  setSubtaskDraft(parent: ProjectTask, draft: string): void;
  setSubtaskActive(parent: ProjectTask, active: boolean): void;
  submitSubtask(parent: ProjectTask): Promise<void>;
}

export class ProjectListQuickAddController implements ProjectListSubtaskQuickAdd {
  pendingTarget = $state<ProjectListTaskCreateTarget | null>(null);
  errorTarget = $state<ProjectListTaskCreateTarget | null>(null);
  errorMessage = $state<string | null>(null);
  sectionDraft = $state("");
  sectionTaskDrafts = $state<Record<string, string>>({});
  groupTaskDrafts = $state<Record<string, string>>({});
  subtaskDrafts = $state<Record<string, string>>({});
  activeSectionTaskDraftInputId = $state<string | null>(null);
  activeGroupTaskDraftInputId = $state<string | null>(null);
  activeSubtaskDraftParentId = $state<string | null>(null);
  sectionDraftInputActive = $state(false);

  constructor(private readonly context: ProjectListQuickAddControllerContext) {}

  hasActiveDraft(): boolean {
    return this.activeSectionTaskDraftInputId !== null
      || this.activeGroupTaskDraftInputId !== null
      || this.activeSubtaskDraftParentId !== null
      || this.sectionDraftInputActive
      || this.sectionDraft.trim().length > 0
      || Object.values(this.groupTaskDrafts).some((draft) => draft.trim().length > 0)
      || Object.values(this.subtaskDrafts).some((draft) => draft.trim().length > 0);
  }

  clearError(target: ProjectListTaskCreateTarget): void {
    if (this.errorTarget !== target) return;
    this.errorTarget = null;
    this.errorMessage = null;
  }

  errorFor(target: ProjectListTaskCreateTarget): string | null {
    return this.errorTarget === target ? this.errorMessage : null;
  }

  cancelActiveDrafts(): void {
    if (this.activeSectionTaskDraftInputId) {
      const id = this.activeSectionTaskDraftInputId;
      this.sectionTaskDrafts = { ...this.sectionTaskDrafts, [id]: "" };
      this.activeSectionTaskDraftInputId = null;
      this.clearError(projectListSectionTaskCreateTarget(id));
    }
    if (this.activeGroupTaskDraftInputId) {
      const id = this.activeGroupTaskDraftInputId;
      this.groupTaskDrafts = { ...this.groupTaskDrafts, [id]: "" };
      this.activeGroupTaskDraftInputId = null;
      this.clearError(projectListGroupTaskCreateTarget(id));
    }
    this.cancelSubtaskDraft();
    if (this.sectionDraftInputActive || this.sectionDraft.trim()) {
      this.sectionDraft = "";
      this.sectionDraftInputActive = false;
    }
  }

  private cancelSubtaskDraft(): void {
    const id = this.activeSubtaskDraftParentId;
    if (!id) return;
    this.subtaskDrafts = { ...this.subtaskDrafts, [id]: "" };
    this.activeSubtaskDraftParentId = null;
    this.clearError(projectListSubtaskCreateTarget(id));
  }

  cancelForOutsideTarget(target: Element): void {
    if (this.activeSectionTaskDraftInputId) {
      const id = this.activeSectionTaskDraftInputId;
      if (target.closest("[data-section-task-add-row]")?.getAttribute("data-section-task-add-row") !== id) {
        this.sectionTaskDrafts = { ...this.sectionTaskDrafts, [id]: "" };
        this.activeSectionTaskDraftInputId = null;
        this.clearError(projectListSectionTaskCreateTarget(id));
      }
    }
    if (this.activeGroupTaskDraftInputId) {
      const id = this.activeGroupTaskDraftInputId;
      if (target.closest("[data-group-task-add-row]")?.getAttribute("data-group-task-add-row") !== id) {
        this.groupTaskDrafts = { ...this.groupTaskDrafts, [id]: "" };
        this.activeGroupTaskDraftInputId = null;
        this.clearError(projectListGroupTaskCreateTarget(id));
      }
    }
    if (
      this.activeSubtaskDraftParentId
      && target.closest("[data-subtask-add-row]")?.getAttribute("data-subtask-add-row") !== this.activeSubtaskDraftParentId
    ) {
      this.cancelSubtaskDraft();
    }
    if ((this.sectionDraftInputActive || this.sectionDraft.trim()) && !target.closest("[data-add-section-row='true']")) {
      this.sectionDraft = "";
      this.sectionDraftInputActive = false;
    }
  }

  groupPlan(group: ProjectTaskListGroup): ProjectListGroupQuickAddPlan {
    return projectListGroupQuickAddPlan({
      groupBy: this.context.getGroupBy(),
      group,
      statuses: this.context.getStatuses(),
      priorities: this.context.getPriorities(),
    });
  }

  private setError(target: ProjectListTaskCreateTarget, message: string): void {
    this.errorTarget = target;
    this.errorMessage = message;
  }

  private async createTask(
    target: ProjectListTaskCreateTarget,
    title: string,
    options: {
      sectionId?: string;
      statusId?: string;
      parentTaskId?: string;
      patch?: Partial<Pick<ProjectTask, "priority" | "dueDate">>;
    } = {},
  ): Promise<ProjectTask | undefined> {
    const projectId = this.context.getSelectedProjectId();
    if (!projectId) {
      this.setError(target, this.context.selectProjectFirstMessage());
      return undefined;
    }
    const displayTitle = title.trim();
    if (!displayTitle) return undefined;
    this.pendingTarget = target;
    this.clearError(target);
    try {
      const created = await this.context.projects.addTask(
        projectId,
        displayTitle,
        options.sectionId,
        options.statusId,
        options.parentTaskId ?? null,
      );
      if (!created || !options.patch || Object.keys(options.patch).length === 0) return created;
      await this.context.projects.updateTask(created, options.patch);
      return this.context.projects.taskById(created.id) ?? created;
    } catch (error) {
      console.error("create project task failed", error);
      this.setError(target, this.context.createFailedMessage(error));
      return undefined;
    } finally {
      if (this.pendingTarget === target) this.pendingTarget = null;
    }
  }

  async submitSectionTask(sectionId: string): Promise<void> {
    const target = projectListSectionTaskCreateTarget(sectionId);
    const created = await this.createTask(target, this.sectionTaskDrafts[sectionId] ?? "", { sectionId });
    if (!created) return;
    this.sectionTaskDrafts = { ...this.sectionTaskDrafts, [sectionId]: "" };
    this.context.revealTask(created);
  }

  async submitGroupTask(group: ProjectTaskListGroup): Promise<void> {
    const plan = this.groupPlan(group);
    if (!plan.enabled) return;
    const key = projectListGroupTaskDraftKey(this.context.getGroupBy(), group);
    const target = projectListGroupTaskCreateTarget(key);
    const created = await this.createTask(target, this.groupTaskDrafts[key] ?? "", {
      statusId: plan.statusId,
      patch: plan.patch,
    });
    if (!created) return;
    this.groupTaskDrafts = { ...this.groupTaskDrafts, [key]: "" };
    this.context.revealTask(created);
  }

  get pending(): boolean {
    return this.pendingTarget !== null;
  }

  subtaskDraft(parent: ProjectTask): string {
    return this.subtaskDrafts[parent.id] ?? "";
  }

  subtaskActive(parent: ProjectTask): boolean {
    return this.activeSubtaskDraftParentId === parent.id;
  }

  subtaskError(parent: ProjectTask): string | null {
    return this.errorFor(projectListSubtaskCreateTarget(parent.id));
  }

  setSubtaskDraft(parent: ProjectTask, draft: string): void {
    this.subtaskDrafts = { ...this.subtaskDrafts, [parent.id]: draft };
  }

  setSubtaskActive(parent: ProjectTask, active: boolean): void {
    if (active) this.activeSubtaskDraftParentId = parent.id;
    else if (this.activeSubtaskDraftParentId === parent.id) this.activeSubtaskDraftParentId = null;
  }

  /** Creates a subtask in the parent's section with the project's default status. */
  async submitSubtask(parent: ProjectTask): Promise<void> {
    const created = await this.createTask(projectListSubtaskCreateTarget(parent.id), this.subtaskDraft(parent), {
      sectionId: parent.sectionId,
      parentTaskId: parent.id,
    });
    if (!created) return;
    this.setSubtaskDraft(parent, "");
  }

  async submitSection(): Promise<void> {
    const projectId = this.context.getSelectedProjectId();
    const name = this.sectionDraft.trim();
    if (!projectId || !name) return;
    await this.context.projects.addSection(projectId, name);
    this.sectionDraft = "";
  }
}
