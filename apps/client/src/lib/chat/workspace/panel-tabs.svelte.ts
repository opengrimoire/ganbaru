import { tick } from "svelte";
import {
  moveWorkspacePanelTab,
  normalizeWorkspacePanelTabName,
  workspacePanelKinds,
  workspacePanelRenameGeometry,
  workspacePanelTabInsertionIndex,
  workspacePanelTabShift,
  type ChatInspectorThreadState,
  type ChatWorkspacePanelRenameGeometry,
  type ChatWorkspacePanelTabKey,
} from "./inspector-model";

export interface WorkspacePanelTabControllerOptions {
  orderedKeys: () => readonly ChatWorkspacePanelTabKey[];
  tabNames: () => Partial<Record<ChatWorkspacePanelTabKey, string>>;
  label: (key: ChatWorkspacePanelTabKey) => string;
  defaultLabel: (key: ChatWorkspacePanelTabKey) => string;
  activate: (key: ChatWorkspacePanelTabKey) => void;
  update: (update: Partial<ChatInspectorThreadState>) => void;
  closePicker: () => void;
}

interface TabDragGesture {
  key: ChatWorkspacePanelTabKey;
  pointerId: number;
  startClientX: number;
  order: ChatWorkspacePanelTabKey[];
  sourceIndex: number;
  sourceCenter: number;
  sourceSpan: number;
  active: boolean;
}

/** Owns workspace tab keyboard, rename, scrolling, and pointer-reorder behavior. */
export class WorkspacePanelTabController {
  tabbar: HTMLDivElement | undefined = $state();
  renamePanel: HTMLDivElement | undefined = $state();
  renameInput: HTMLInputElement | undefined = $state();
  draggedKey: ChatWorkspacePanelTabKey | null = $state(null);
  renameState: {
    key: ChatWorkspacePanelTabKey;
    anchorX: number;
    anchorY: number;
  } | null = $state(null);
  renameDraft = $state("");
  renameReady = $state(false);
  renameGeometry = $state<ChatWorkspacePanelRenameGeometry>({
    left: 0,
    top: 0,
    width: 240,
    maxHeight: 0,
  });

  private draggedOffsetX = $state(0);
  private dragTargetIndex = $state(0);
  private dragGesture: TabDragGesture | null = null;
  private dragListenersAttached = false;
  private renameTrigger: HTMLElement | undefined;

  constructor(private readonly options: WorkspacePanelTabControllerOptions) {}

  handleKeydown(event: KeyboardEvent, key: ChatWorkspacePanelTabKey): void {
    if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
      event.preventDefault();
      const trigger = event.currentTarget as HTMLElement;
      const triggerRect = trigger.getBoundingClientRect();
      void this.openRename(key, triggerRect.left, triggerRect.bottom, trigger);
      return;
    }
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    const tabbar = (event.currentTarget as HTMLElement).closest<HTMLElement>("[role='tablist']");
    const buttons = [...(tabbar?.querySelectorAll<HTMLButtonElement>("[role='tab']") ?? [])];
    const index = buttons.indexOf(event.currentTarget as HTMLButtonElement);
    if (index < 0 || buttons.length === 0) return;
    event.preventDefault();
    const next = event.key === "Home"
      ? 0
      : event.key === "End"
        ? buttons.length - 1
        : (index + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length;
    buttons[next]?.click();
    buttons[next]?.focus();
  }

  handleWheel(event: WheelEvent): void {
    if (!this.tabbar || this.tabbar.scrollWidth <= this.tabbar.clientWidth) return;
    const delta = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
    if (delta === 0) return;
    const previousScrollLeft = this.tabbar.scrollLeft;
    this.tabbar.scrollLeft += delta;
    if (this.tabbar.scrollLeft !== previousScrollLeft) event.preventDefault();
  }

  async reveal(key: ChatWorkspacePanelTabKey): Promise<void> {
    await tick();
    const tab = [...(this.tabbar?.querySelectorAll<HTMLElement>("[data-panel-tab-key]") ?? [])]
      .find((candidate) => candidate.dataset.panelTabKey === key);
    tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  handleContextMenu(event: MouseEvent, key: ChatWorkspacePanelTabKey): void {
    event.preventDefault();
    event.stopPropagation();
    const slot = event.currentTarget as HTMLElement;
    const trigger = slot.querySelector<HTMLElement>("[role='tab']") ?? slot;
    void this.openRename(key, event.clientX, event.clientY, trigger);
  }

  async openRename(
    key: ChatWorkspacePanelTabKey,
    anchorX: number,
    anchorY: number,
    trigger: HTMLElement,
  ): Promise<void> {
    this.cancelDrag();
    this.options.closePicker();
    this.renameTrigger = trigger;
    this.renameDraft = this.options.label(key);
    this.renameReady = false;
    this.renameState = { key, anchorX, anchorY };
    await tick();
    this.positionRename();
    this.renameInput?.focus();
    this.renameInput?.select();
  }

  positionRename(): void {
    if (!this.renameState) return;
    this.renameGeometry = workspacePanelRenameGeometry(
      this.renameState.anchorX,
      this.renameState.anchorY,
      window.innerWidth,
      window.innerHeight,
      this.renamePanel?.scrollHeight ?? 132,
    );
    this.renameReady = true;
  }

  renameStyle(): string {
    if (!this.renameReady) return "visibility:hidden;top:0;left:0;";
    return [
      "visibility:visible",
      `top:${this.renameGeometry.top}px`,
      `left:${this.renameGeometry.left}px`,
      `width:${this.renameGeometry.width}px`,
      `max-height:${this.renameGeometry.maxHeight}px`,
    ].join(";");
  }

  closeRename(restoreFocus = false): void {
    const trigger = this.renameTrigger;
    this.renameState = null;
    this.renameReady = false;
    this.renameTrigger = undefined;
    if (restoreFocus) queueMicrotask(() => trigger?.focus());
  }

  saveName(): void {
    const state = this.renameState;
    if (!state) return;
    const name = normalizeWorkspacePanelTabName(this.renameDraft);
    if (!name) return;
    const tabNames = this.currentTabNames();
    if (name === this.options.defaultLabel(state.key)) delete tabNames[state.key];
    else tabNames[state.key] = name;
    this.options.update({ tabNames });
    this.closeRename(true);
  }

  resetName(): void {
    const state = this.renameState;
    if (!state) return;
    const tabNames = this.currentTabNames();
    delete tabNames[state.key];
    this.options.update({ tabNames });
    this.closeRename(true);
  }

  handleRenameKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape") return;
    event.preventDefault();
    this.closeRename(true);
  }

  beginDrag(event: PointerEvent, key: ChatWorkspacePanelTabKey): void {
    if (event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest(".tab-close")) return;
    if (!this.tabbar) return;
    const order = [...this.options.orderedKeys()];
    const sourceIndex = order.indexOf(key);
    if (sourceIndex < 0) return;
    const slot = event.currentTarget as HTMLElement;
    const computedGap = Number.parseFloat(getComputedStyle(this.tabbar).columnGap);
    this.options.activate(key);
    this.dragGesture = {
      key,
      pointerId: event.pointerId,
      startClientX: event.clientX,
      order,
      sourceIndex,
      sourceCenter: slot.offsetLeft + slot.offsetWidth / 2,
      sourceSpan: slot.offsetWidth + (Number.isFinite(computedGap) ? computedGap : 0),
      active: false,
    };
    this.dragTargetIndex = sourceIndex;
    this.attachDragListeners();
  }

  dragShift(key: ChatWorkspacePanelTabKey): number {
    const gesture = this.dragGesture;
    if (!gesture?.active) return 0;
    if (key === gesture.key) return this.draggedOffsetX;
    return workspacePanelTabShift(
      gesture.order.indexOf(key),
      gesture.sourceIndex,
      this.dragTargetIndex,
      gesture.sourceSpan,
    );
  }

  cancelDrag(): void {
    this.detachDragListeners();
    this.dragGesture = null;
    this.draggedKey = null;
    this.draggedOffsetX = 0;
    this.dragTargetIndex = 0;
  }

  destroy(): void {
    this.cancelDrag();
  }

  private currentTabNames(): Partial<Record<ChatWorkspacePanelTabKey, string>> {
    return { ...this.options.tabNames() };
  }

  private moveDrag(event: PointerEvent): void {
    const gesture = this.dragGesture;
    if (!gesture || gesture.pointerId !== event.pointerId || !this.tabbar) return;
    const offsetX = event.clientX - gesture.startClientX;
    if (!gesture.active) {
      if (Math.abs(offsetX) < 5) return;
      gesture.active = true;
      this.draggedKey = gesture.key;
    }
    event.preventDefault();
    this.draggedOffsetX = offsetX;
    const remainingCenters = [...this.tabbar.querySelectorAll<HTMLElement>("[data-panel-tab-key]")]
      .filter((element) => element.dataset.panelTabKey !== gesture.key)
      .map((element) => element.offsetLeft + element.offsetWidth / 2);
    this.dragTargetIndex = workspacePanelTabInsertionIndex(
      gesture.sourceCenter + offsetX,
      remainingCenters,
    );
  }

  private finishDrag(event: PointerEvent, commit: boolean): void {
    const gesture = this.dragGesture;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    const nextOrder = commit && gesture.active
      ? moveWorkspacePanelTab(gesture.order, gesture.key, this.dragTargetIndex)
      : null;
    this.cancelDrag();
    if (nextOrder) {
      this.options.update({
        tabOrder: nextOrder,
        openTabs: workspacePanelKinds(nextOrder),
      });
    }
  }

  private readonly handleWindowPointerMove = (event: PointerEvent): void => {
    this.moveDrag(event);
  };

  private readonly handleWindowPointerUp = (event: PointerEvent): void => {
    this.finishDrag(event, true);
  };

  private readonly handleWindowPointerCancel = (event: PointerEvent): void => {
    this.finishDrag(event, false);
  };

  private readonly handleWindowBlur = (): void => {
    this.cancelDrag();
  };

  private attachDragListeners(): void {
    if (this.dragListenersAttached) return;
    this.dragListenersAttached = true;
    window.addEventListener("pointermove", this.handleWindowPointerMove, true);
    window.addEventListener("pointerup", this.handleWindowPointerUp, true);
    window.addEventListener("pointercancel", this.handleWindowPointerCancel, true);
    window.addEventListener("blur", this.handleWindowBlur);
  }

  private detachDragListeners(): void {
    if (!this.dragListenersAttached) return;
    this.dragListenersAttached = false;
    window.removeEventListener("pointermove", this.handleWindowPointerMove, true);
    window.removeEventListener("pointerup", this.handleWindowPointerUp, true);
    window.removeEventListener("pointercancel", this.handleWindowPointerCancel, true);
    window.removeEventListener("blur", this.handleWindowBlur);
  }
}
