import type { ProjectTask } from "$lib/projects/types";
import type { ProjectToolbarPanel } from "$lib/projects/toolbar";

/** Owns route overlays, settings close guards, selection, and global keyboard policy. */
export class ProjectRouteUiController {
  taskFinderOpen = $state(false);
  taskFinderFocusRequestId = $state(0);
  selectedTaskId = $state<string | null>(null);
  /** Project that receives the task draft open in task detail, if any. */
  taskDraftProjectId = $state<string | null>(null);
  selectedTaskIds = $state<string[]>([]);
  toolbarPanel = $state<ProjectToolbarPanel | null>(null);
  settingsDirty = $state(false);
  discardConfirmOpen = $state(false);
  rootElement = $state<HTMLDivElement | null>(null);
  #pendingSettingsAction: (() => void) | null = null;

  runAfterSettingsClose(action: () => void): void {
    if (this.toolbarPanel === "settings" && this.settingsDirty) {
      this.#pendingSettingsAction = action;
      this.discardConfirmOpen = true;
      return;
    }
    action();
  }

  setToolbarPanel(panel: ProjectToolbarPanel | null): void {
    this.toolbarPanel = panel;
    if (panel !== "settings") this.settingsDirty = false;
  }

  toggleToolbarPanel(panel: ProjectToolbarPanel): void {
    const next = this.toolbarPanel === panel ? null : panel;
    this.runAfterSettingsClose(() => this.setToolbarPanel(next));
  }

  requestToolbarClose(): void {
    this.runAfterSettingsClose(() => this.setToolbarPanel(null));
  }

  closeToolbarImmediately(): void {
    this.#pendingSettingsAction = null;
    this.discardConfirmOpen = false;
    this.setToolbarPanel(null);
  }

  confirmDiscard(): void {
    const action = this.#pendingSettingsAction;
    this.#pendingSettingsAction = null;
    this.discardConfirmOpen = false;
    this.settingsDirty = false;
    action?.();
  }

  cancelDiscard(): void {
    this.#pendingSettingsAction = null;
    this.discardConfirmOpen = false;
  }

  openFinder(): void {
    this.runAfterSettingsClose(() => {
      this.taskFinderOpen = true;
      this.setToolbarPanel(null);
      this.taskFinderFocusRequestId += 1;
    });
  }

  closeFinder(): void {
    this.taskFinderOpen = false;
  }

  closeOrClearFinder(search: string, clearSearch: () => void): void {
    if (search.trim()) clearSearch();
    this.closeFinder();
  }

  openTask(task: ProjectTask): void {
    this.runAfterSettingsClose(() => {
      this.setToolbarPanel(null);
      this.taskDraftProjectId = null;
      this.selectedTaskId = task.id;
    });
  }

  /** Opens task detail on an unsaved draft that becomes a task only when submitted. */
  openTaskDraft(projectId: string): void {
    this.runAfterSettingsClose(() => {
      this.setToolbarPanel(null);
      this.selectedTaskId = null;
      this.taskDraftProjectId = projectId;
    });
  }

  /** Replaces the open draft with the task created from it. */
  completeTaskDraft(task: ProjectTask): void {
    this.taskDraftProjectId = null;
    this.selectedTaskId = task.id;
  }

  closeTaskDetail(): void {
    this.taskDraftProjectId = null;
    this.selectedTaskId = null;
  }

  toggleTaskSelection(task: ProjectTask): void {
    this.selectedTaskIds = this.selectedTaskIds.includes(task.id)
      ? this.selectedTaskIds.filter((taskId) => taskId !== task.id)
      : [...this.selectedTaskIds, task.id];
  }

  clearTaskSelection(): void {
    this.selectedTaskIds = [];
  }

  handleWindowKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || this.selectedTaskId || this.taskDraftProjectId) return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      this.openFinder();
    } else if (event.key === "Escape" && this.taskFinderOpen) {
      event.preventDefault();
      this.closeFinder();
    }
  }

  handleDocumentSelectStart(event: Event): void {
    const target = event.target;
    if (!(target instanceof Node) || !this.rootElement?.contains(target)) return;
    if (isEditableSelectionTarget(target)) return;
    event.preventDefault();
  }

  handleDocumentSelectionChange(): void {
    const selection = document.getSelection();
    if (!selection || selection.isCollapsed) return;
    if (document.activeElement && isEditableSelectionTarget(document.activeElement)) return;
    const inside = (node: Node | null) => Boolean(this.rootElement && node && this.rootElement.contains(node));
    if (!inside(selection.anchorNode) && !inside(selection.focusNode)) return;
    selection.removeAllRanges();
  }

}

export function isEditableSelectionTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && Boolean(target.closest("input, textarea, [contenteditable='true'], [role='textbox']"));
}
