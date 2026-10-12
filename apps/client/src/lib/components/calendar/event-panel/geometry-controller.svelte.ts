import { tick, untrack } from "svelte";
import {
  EVENT_PANEL_EDGE_MARGIN,
  EVENT_PANEL_MAX_WIDTH,
  EVENT_PANEL_TITLE_BAR_HEIGHT,
  getResponsivePanelWidth,
  pickEventPanelLayout,
  type EventPanelAnchor,
  type EventPanelLayout,
} from "$lib/utils/responsive";
import {
  buildEventPanelStyle,
  clampFloatingLeft,
  clampFloatingTop,
  getAvailablePanelHeight,
  getPanelHeightLimit,
  shouldConstrainPanelHeight,
  type EventPanelGeometryInput,
} from "./geometry";

const DEFAULT_PANEL_HEIGHT = 540;
const PANEL_MAX_WIDTH = Math.round(EVENT_PANEL_MAX_WIDTH * 1.08);
const PANEL_GAP = EVENT_PANEL_EDGE_MARGIN;
const TITLE_BAR_HEIGHT = EVENT_PANEL_TITLE_BAR_HEIGHT;
const HEIGHT_TOLERANCE = 2;

export interface EventPanelGeometryControllerOptions {
  anchor: () => EventPanelAnchor;
  parked: () => boolean;
  viewport: () => { width: number; height: number };
}

export interface EventPanelDragState {
  active: boolean;
  origin: { x: number; y: number };
  offset: { x: number; y: number };
}

export function beginEventPanelDrag(
  pointer: { x: number; y: number },
  offset: { x: number; y: number },
): EventPanelDragState {
  return {
    active: true,
    origin: { x: pointer.x - offset.x, y: pointer.y - offset.y },
    offset,
  };
}

export function moveEventPanelDrag(
  state: EventPanelDragState,
  pointer: { x: number; y: number },
): EventPanelDragState {
  if (!state.active) return state;
  return {
    ...state,
    offset: { x: pointer.x - state.origin.x, y: pointer.y - state.origin.y },
  };
}

export function endEventPanelDrag(state: EventPanelDragState): EventPanelDragState {
  return { ...state, active: false };
}

/** Owns panel measurement, responsive positioning, pinning, and pointer drag. */
export class EventPanelGeometryController {
  panelEl = $state<HTMLDivElement>();
  scrollEl = $state<HTMLDivElement>();
  contentEl = $state<HTMLDivElement>();
  panelHeight = $state(0);
  positionReady = $state(false);
  pinnedBottom = $state(0);
  dragOffset = $state({ x: 0, y: 0 });
  isDragging = $state(false);
  baseLeft = $state(0);
  baseTop = $state(0);

  private pinnedDragY = 0;
  private userDragged = false;
  private dragState: EventPanelDragState = {
    active: false,
    origin: { x: 0, y: 0 },
    offset: { x: 0, y: 0 },
  };
  private releasePinTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly options: EventPanelGeometryControllerOptions) {
    $effect(() => {
      const panel = this.panelEl;
      if (!panel) return;
      const observer = new ResizeObserver(() => this.measureNaturalHeight());
      observer.observe(panel);
      if (this.scrollEl) observer.observe(this.scrollEl);
      if (this.contentEl) observer.observe(this.contentEl);
      void tick().then(() => this.measureNaturalHeight());
      return () => observer.disconnect();
    });

    $effect(() => {
      const anchor = this.options.anchor();
      const layout = this.layout;
      const width = this.width;
      const viewport = this.options.viewport();
      const panel = this.panelEl;
      const height = this.positionReady ? untrack(() => this.panelHeight) : this.panelHeight;
      if (!panel || height <= 0) {
        if (!this.options.parked()) this.positionReady = false;
        return;
      }
      if (!this.canDrag) {
        this.dragOffset = { x: 0, y: 0 };
        this.pinnedBottom = 0;
        this.userDragged = false;
        this.positionReady = true;
        return;
      }
      if (this.userDragged) return;
      const availableHeight = getAvailablePanelHeight(viewport.height, TITLE_BAR_HEIGHT, PANEL_GAP);
      const visibleHeight = Math.min(height, availableHeight);
      let left: number;
      if (layout === "centered") {
        left = Math.round((viewport.width - width) / 2);
      } else {
        const anchorLeft = anchor.x - anchor.width;
        const requiredSideSpace = width + PANEL_GAP * 2;
        const rightSpace = viewport.width - anchor.x;
        if (anchorLeft >= requiredSideSpace) left = anchorLeft - PANEL_GAP - width;
        else if (rightSpace >= requiredSideSpace) left = anchor.x + PANEL_GAP;
        else left = Math.round((viewport.width - width) / 2);
      }
      const top = layout === "centered"
        ? Math.round((viewport.height - visibleHeight) / 2)
        : anchor.y;
      this.baseLeft = clampFloatingLeft(left, viewport.width, width, PANEL_GAP);
      this.baseTop = clampFloatingTop(top, viewport.height, visibleHeight, this.minTop, PANEL_GAP);
      this.dragOffset = { x: 0, y: 0 };
      this.positionReady = true;
    });

    $effect(() => () => {
      if (this.releasePinTimer) clearTimeout(this.releasePinTimer);
    });
  }

  get width(): number {
    return getResponsivePanelWidth(this.options.viewport().width, PANEL_MAX_WIDTH, PANEL_GAP);
  }

  get layout(): EventPanelLayout {
    const viewport = this.options.viewport();
    return pickEventPanelLayout({
      viewport,
      anchor: this.options.anchor(),
      panelWidth: PANEL_MAX_WIDTH,
      edgeMargin: PANEL_GAP,
      titleBarHeight: TITLE_BAR_HEIGHT,
    });
  }

  get canDrag(): boolean {
    return this.layout === "anchored" || this.layout === "centered";
  }

  get isDateTimeStacked(): boolean {
    return this.width < 300;
  }

  get minTop(): number {
    return TITLE_BAR_HEIGHT + PANEL_GAP;
  }

  get style(): string {
    return buildEventPanelStyle(this.currentInput());
  }

  get parkedStyle(): string {
    return `position:fixed; left:-10000px; top:-10000px; width:${Math.round(this.width)}px; z-index:-1; pointer-events:none;`;
  }

  get isBodyConstrained(): boolean {
    return this.layout === "fullscreen"
      || shouldConstrainPanelHeight(
        this.panelHeight,
        getPanelHeightLimit(this.currentInput()),
        HEIGHT_TOLERANCE,
      );
  }

  resetForSession(parked: boolean): void {
    if (this.releasePinTimer) clearTimeout(this.releasePinTimer);
    this.releasePinTimer = undefined;
    this.panelHeight = 0;
    this.positionReady = parked;
    this.pinnedBottom = 0;
    this.dragOffset = { x: 0, y: 0 };
    this.userDragged = false;
    this.isDragging = false;
    this.dragState = {
      active: false,
      origin: { x: 0, y: 0 },
      offset: { x: 0, y: 0 },
    };
  }

  resetExitAnimation(): void {
    for (const animation of this.panelEl?.getAnimations() ?? []) animation.cancel();
  }

  measureNaturalHeight(): void {
    const panel = this.panelEl;
    if (!panel) return;
    const renderedHeight = panel.offsetHeight;
    let naturalHeight = panel.scrollHeight;
    if (this.scrollEl) {
      const panelRect = panel.getBoundingClientRect();
      const scrollRect = this.scrollEl.getBoundingClientRect();
      const chromeHeight = Math.max(0, panelRect.height - scrollRect.height);
      naturalHeight = Math.max(naturalHeight, chromeHeight + this.scrollEl.scrollHeight);
    }
    this.panelHeight = Math.ceil(Math.max(renderedHeight, naturalHeight));
  }

  measureAfterRender(expected: () => boolean): void {
    void tick().then(() => {
      if (expected()) this.measureNaturalHeight();
    });
  }

  updateSectionPin(opening: boolean): void {
    if (this.releasePinTimer) clearTimeout(this.releasePinTimer);
    this.releasePinTimer = undefined;
    if (opening && this.panelEl && this.canDrag) {
      const rect = this.panelEl.getBoundingClientRect();
      const viewport = this.options.viewport();
      const roomBelow = viewport.height - PANEL_GAP - rect.bottom;
      const roomAbove = rect.top - this.minTop;
      if (roomBelow < roomAbove) {
        this.pinnedBottom = rect.bottom;
        this.pinnedDragY = this.dragOffset.y;
      } else {
        this.pinnedBottom = 0;
      }
      return;
    }
    if (opening) {
      this.pinnedBottom = 0;
      return;
    }
    if (this.pinnedBottom <= 0) return;
    this.releasePinTimer = setTimeout(() => {
      if (this.pinnedBottom > 0 && this.panelEl) {
        this.baseTop = this.panelEl.getBoundingClientRect().top - this.dragOffset.y;
      }
      this.pinnedBottom = 0;
      this.releasePinTimer = undefined;
    }, 200);
  }

  handleDragStart(event: PointerEvent): void {
    if (!this.canDrag || this.options.parked()) return;
    this.isDragging = true;
    this.userDragged = true;
    this.dragState = beginEventPanelDrag(
      { x: event.clientX, y: event.clientY },
      this.dragOffset,
    );
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  handleDragMove(event: PointerEvent): void {
    if (!this.isDragging || !this.canDrag) return;
    this.dragState = moveEventPanelDrag(this.dragState, { x: event.clientX, y: event.clientY });
    this.dragOffset = this.dragState.offset;
  }

  handleDragEnd(): void {
    this.dragState = endEventPanelDrag(this.dragState);
    this.isDragging = this.dragState.active;
  }

  private currentInput(): EventPanelGeometryInput {
    const viewport = this.options.viewport();
    return {
      baseLeft: this.baseLeft,
      baseTop: this.baseTop,
      defaultPanelHeight: DEFAULT_PANEL_HEIGHT,
      dragOffset: this.dragOffset,
      gap: PANEL_GAP,
      heightConstraintTolerance: HEIGHT_TOLERANCE,
      layout: this.layout,
      minTop: this.minTop,
      panelHeight: this.panelHeight,
      pinnedBottom: this.pinnedBottom,
      pinnedDragY: this.pinnedDragY,
      titleBarHeight: TITLE_BAR_HEIGHT,
      viewportHeight: viewport.height,
      viewportWidth: viewport.width,
      width: this.width,
    };
  }
}
