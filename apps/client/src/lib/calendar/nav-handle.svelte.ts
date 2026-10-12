/**
 * Headless handle for driving calendar navigation from outside the component
 * tree. The benchmark harness calls `navigate("forward")` and
 * `setViewMode("week")` programmatically; without this indirection the
 * scenario module would have to reach into a Svelte component's internal
 * state, which Svelte 5 does not expose stably.
 *
 * `CalendarView.svelte` registers its in-component `navigate` and view-setter
 * on mount and clears them on unmount. Until a registration lands, the
 * methods are no-ops, which is also the right behavior when the calendar
 * view is not the current navigation target.
 */
import type { CalendarViewMode } from "./types";
import type { HeldNavigationDirection, HeldNavigationEvent } from "./held-navigation";

type NavigateDirection = "today" | "back" | "forward";
type NavigateSource = "programmatic" | "wheel" | "key" | "hold-repeat";
type NavigateFn = (direction: NavigateDirection, source?: NavigateSource) => void;
type SetViewModeFn = (mode: CalendarViewMode) => void;
type SetAnchorDateFn = (date: Date) => void;
type OpenVisibleEventFn = (index: number) => Promise<boolean>;
type GetVisibleEventCountFn = () => number;
type OpenCreatePanelFn = (start: string, end: string, allDay?: boolean) => Promise<boolean>;
type ClosePanelFn = () => Promise<void>;
type CanRepeatHeldNavigationFn = (direction: HeldNavigationDirection) => boolean;
type HeldNavigationObserver = (event: HeldNavigationEvent) => void;

class CalendarNavHandle {
  #navigate: NavigateFn | null = null;
  #setViewMode: SetViewModeFn | null = null;
  #setAnchorDate: SetAnchorDateFn | null = null;
  #openVisibleEvent: OpenVisibleEventFn | null = null;
  #getVisibleEventCount: GetVisibleEventCountFn | null = null;
  #openCreatePanel: OpenCreatePanelFn | null = null;
  #closePanel: ClosePanelFn | null = null;
  #canRepeatHeldNavigation: CanRepeatHeldNavigationFn | null = null;
  #viewMode: CalendarViewMode = "week";
  readonly #heldNavigationObservers = new Set<HeldNavigationObserver>();

  /**
   * Called by `CalendarView.svelte` on mount. Returns a disposer that the
   * component runs from its onMount cleanup.
   */
  register(opts: {
    navigate: NavigateFn;
    setViewMode: SetViewModeFn;
    setAnchorDate: SetAnchorDateFn;
    openVisibleEvent: OpenVisibleEventFn;
    getVisibleEventCount: GetVisibleEventCountFn;
    openCreatePanel: OpenCreatePanelFn;
    closePanel: ClosePanelFn;
    canRepeatHeldNavigation: CanRepeatHeldNavigationFn;
    getViewMode: () => CalendarViewMode;
  }): () => void {
    this.#navigate = opts.navigate;
    this.#setViewMode = opts.setViewMode;
    this.#setAnchorDate = opts.setAnchorDate;
    this.#openVisibleEvent = opts.openVisibleEvent;
    this.#getVisibleEventCount = opts.getVisibleEventCount;
    this.#openCreatePanel = opts.openCreatePanel;
    this.#closePanel = opts.closePanel;
    this.#canRepeatHeldNavigation = opts.canRepeatHeldNavigation;
    this.#viewMode = opts.getViewMode();
    return () => {
      this.#navigate = null;
      this.#setViewMode = null;
      this.#setAnchorDate = null;
      this.#openVisibleEvent = null;
      this.#getVisibleEventCount = null;
      this.#openCreatePanel = null;
      this.#closePanel = null;
      this.#canRepeatHeldNavigation = null;
    };
  }

  /** Called from the view's $effect when viewMode changes so callers can read it. */
  reportViewMode(mode: CalendarViewMode) {
    this.#viewMode = mode;
  }

  /** Whether a CalendarView is currently mounted. */
  get available(): boolean {
    return this.#navigate !== null;
  }

  get viewMode(): CalendarViewMode {
    return this.#viewMode;
  }

  navigate(direction: NavigateDirection, source?: NavigateSource) {
    this.#navigate?.(direction, source);
  }

  setViewMode(mode: CalendarViewMode) {
    this.#setViewMode?.(mode);
  }

  setAnchorDate(date: Date) {
    this.#setAnchorDate?.(date);
  }

  openVisibleEvent(index: number): Promise<boolean> {
    return this.#openVisibleEvent?.(index) ?? Promise.resolve(false);
  }

  getVisibleEventCount(): number {
    return this.#getVisibleEventCount?.() ?? 0;
  }

  openCreatePanel(start: string, end: string, allDay?: boolean): Promise<boolean> {
    return this.#openCreatePanel?.(start, end, allDay) ?? Promise.resolve(false);
  }

  closePanel(): Promise<void> {
    return this.#closePanel?.() ?? Promise.resolve();
  }

  canRepeatHeldNavigation(direction: HeldNavigationDirection): boolean {
    return this.#canRepeatHeldNavigation?.(direction) ?? false;
  }

  observeHeldNavigation(observer: HeldNavigationObserver): () => void {
    this.#heldNavigationObservers.add(observer);
    return () => {
      this.#heldNavigationObservers.delete(observer);
    };
  }

  reportHeldNavigation(event: HeldNavigationEvent): void {
    for (const observer of this.#heldNavigationObservers) observer(event);
  }
}

let handle: CalendarNavHandle | null = null;

export function getCalendarNavHandle(): CalendarNavHandle {
  if (!handle) handle = new CalendarNavHandle();
  return handle;
}
