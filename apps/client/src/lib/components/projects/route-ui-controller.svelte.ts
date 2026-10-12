import { isAppShortcutBlockedTarget, isEditableKeyboardTarget } from "$lib/utils";
import { PROJECT_VIEW_IDS, type ProjectTask, type ProjectViewId } from "$lib/projects/types";
import type { ProjectToolbarPanel } from "$lib/projects/toolbar";

const PROJECT_VIEW_SHORTCUTS = new Map<string, ProjectViewId>(
  PROJECT_VIEW_IDS.map((view, index) => [String(index + 1), view]),
);

export interface ProjectRouteUiControllerInput {
  setActiveView: (view: ProjectViewId) => void;
}

/** Owns route overlays, settings close guards, selection, and global keyboard policy. */
export class ProjectRouteUiController {
  readonly #input: ProjectRouteUiControllerInput;
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

  constructor(input: ProjectRouteUiControllerInput) {
    this.#input = input;
  }

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
    if (this.handleViewShortcut(event)) return;
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

  #shortcutBlocked(event: KeyboardEvent): boolean {
    const blocked = (target: EventTarget | null) => isEditableKeyboardTarget(target)
      || isAppShortcutBlockedTarget(target)
      || (target instanceof Element && target.closest("[role='dialog']") !== null);
    return this.taskFinderOpen
      || this.toolbarPanel !== null
      || blocked(event.target)
      || blocked(document.activeElement)
      || this.rootElement?.querySelector("[role='dialog']") !== null;
  }

  #handleViewShortcut(event: KeyboardEvent): boolean {
    if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
    const view = PROJECT_VIEW_SHORTCUTS.get(event.key);
    if (!view || this.#shortcutBlocked(event)) return false;
    event.preventDefault();
    this.#input.setActiveView(view);
    return true;
  }

  handleViewShortcut(event: KeyboardEvent): boolean {
    return this.#handleViewShortcut(event);
  }
}

export function isEditableSelectionTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && Boolean(target.closest("input, textarea, [contenteditable='true'], [role='textbox']"));
}
