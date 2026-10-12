import { isAppShortcutBlockedTarget, isEditableKeyboardTarget } from "$lib/utils";
import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
import {
  HeldNavigationController,
  NAV_HOLD_DELAY_MS,
  NAV_REPEAT_MS,
  type HeldNavigationEvent,
  type HeldNavigationKey,
} from "$lib/calendar/held-navigation";
import type { CalendarViewMode } from "$lib/calendar/types";

export interface CalendarViewNavigationControllerOptions {
  getViewMode: () => CalendarViewMode;
  getSessionClosed: () => boolean;
  getConfirmOpen: () => boolean;
  getViewWrapper: () => HTMLElement | undefined;
  getVisibleEventCount: () => number;
  closePanel: () => void;
  navigate: (direction: "back" | "forward", source: "key" | "hold-repeat") => void;
  canRepeat: (direction: "back" | "forward") => boolean;
  waitForSettled: () => Promise<void>;
  mark: (name: string, detail?: Record<string, string | number>) => void;
  now?: () => number;
  requestFrame?: (callback: FrameRequestCallback) => number;
  cancelFrame?: (id: number) => void;
}

export interface CalendarBenchmarkNavigationRegistration {
  navigate: (direction: "today" | "back" | "forward", source?: "programmatic" | "wheel" | "key" | "hold-repeat") => void;
  setViewMode: (mode: CalendarViewMode) => void;
  setAnchorDate: (date: Date) => void;
  openVisibleEvent: (index: number) => Promise<boolean>;
  getVisibleEventCount: () => number;
  openCreatePanel: (start: string, end: string, allDay?: boolean) => Promise<boolean>;
  closePanel: () => Promise<void>;
  canRepeatHeldNavigation: (direction: "back" | "forward") => boolean;
  getViewMode: () => CalendarViewMode;
}

const SCROLL_PX_PER_SEC = 600;

export type CalendarNavigationKeyAction =
  | { type: "close" }
  | { type: "hold"; key: HeldNavigationKey; direction: "back" | "forward" }
  | { type: "scroll"; direction: -1 | 1 }
  | null;

export function calendarNavigationKeyAction(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "metaKey">,
  state: {
    blocked: boolean;
    sessionClosed: boolean;
    confirmOpen: boolean;
    viewMode: CalendarViewMode;
  },
): CalendarNavigationKeyAction {
  if (event.key === "Escape" && !state.sessionClosed) return { type: "close" };
  if (state.blocked || event.ctrlKey || event.altKey || event.metaKey
    || !state.sessionClosed || state.confirmOpen) return null;
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    return {
      type: "hold",
      key: event.key,
      direction: event.key === "ArrowLeft" ? "back" : "forward",
    };
  }
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return null;
  if (state.viewMode === "month") {
    return {
      type: "hold",
      key: event.key,
      direction: event.key === "ArrowUp" ? "back" : "forward",
    };
  }
  return { type: "scroll", direction: event.key === "ArrowUp" ? -1 : 1 };
}

/** Owns held navigation, vertical scrolling, global keys, and benchmark registration. */
export class CalendarViewNavigationController {
  private arrowScrollDirection = 0;
  private arrowScrollFrame = 0;
  private arrowScrollPrevious = 0;
  private releaseSequence = 0;
  private readonly held: HeldNavigationController;

  constructor(private readonly options: CalendarViewNavigationControllerOptions) {
    this.held = new HeldNavigationController({
      holdDelayMs: NAV_HOLD_DELAY_MS,
      repeatMs: NAV_REPEAT_MS,
      navigate: options.navigate,
      canRepeat: options.canRepeat,
      mark: (event) => this.markHeldNavigation(event),
    });
  }

  installWindowListeners(): () => void {
    const keydown = (event: KeyboardEvent) => this.handleKeydown(event);
    const keyup = (event: KeyboardEvent) => this.handleKeyup(event);
    const blur = () => this.stopAll();
    window.addEventListener("keydown", keydown);
    window.addEventListener("keyup", keyup);
    window.addEventListener("blur", blur);
    return () => {
      this.stopAll();
      window.removeEventListener("keydown", keydown);
      window.removeEventListener("keyup", keyup);
      window.removeEventListener("blur", blur);
    };
  }

  registerBenchmark(registration: CalendarBenchmarkNavigationRegistration): () => void {
    return getCalendarNavHandle().register(registration);
  }

  private handleKeydown(event: KeyboardEvent): void {
    const blocked = event.key !== "Escape" && (
      isEditableKeyboardTarget(event.target)
      || isEditableKeyboardTarget(document.activeElement)
      || isAppShortcutBlockedTarget(event.target)
      || isAppShortcutBlockedTarget(document.activeElement)
    );
    const action = calendarNavigationKeyAction(event, {
      blocked,
      sessionClosed: this.options.getSessionClosed(),
      confirmOpen: this.options.getConfirmOpen(),
      viewMode: this.options.getViewMode(),
    });
    if (!action) return;
    event.preventDefault();
    if (action.type === "close") this.options.closePanel();
    else if (action.type === "hold") {
      this.startHeld(action.key, action.direction, event.repeat);
    } else this.startArrowScroll(action.direction);
  }

  private handleKeyup(event: KeyboardEvent): void {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight"
      && event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    this.stopHeld(event.key);
    if (event.key === "ArrowUp" || event.key === "ArrowDown") this.stopArrowScroll();
  }

  private startHeld(
    key: HeldNavigationKey,
    direction: "forward" | "back",
    repeated: boolean,
  ): void {
    if (this.held.activeKey === key) {
      if (repeated) this.held.armRepeatsFromKeydown(key);
      return;
    }
    if (!repeated) this.held.start(key, direction);
  }

  private stopHeld(key?: HeldNavigationKey): void {
    const stopped = this.held.stop(key);
    if (stopped) this.markReleaseTail(stopped);
  }

  private markReleaseTail(key: string): void {
    const sequence = ++this.releaseSequence;
    const releasedAt = (this.options.now ?? performance.now.bind(performance))();
    void this.options.waitForSettled().then(() => {
      if (sequence !== this.releaseSequence) return;
      this.options.mark("nav.release-tail", {
        key,
        ms: Math.round((this.options.now ?? performance.now.bind(performance))() - releasedAt),
        count: this.options.getVisibleEventCount(),
      });
    });
  }

  private markHeldNavigation(event: HeldNavigationEvent): void {
    getCalendarNavHandle().reportHeldNavigation(event);
    if (event.type === "hold-start") {
      this.options.mark("nav.hold-start", { key: event.key, dir: event.direction });
    } else if (event.type === "hold-stop") {
      this.options.mark("nav.hold-stop", { key: event.key, repeats: event.repeats });
    } else if (event.type === "repeat-skip") {
      this.options.mark("nav.repeat-skip", {
        key: event.key,
        repeats: event.repeats,
        reason: event.reason,
      });
    } else if (event.type === "repeat") {
      this.options.mark("nav.repeat", {
        key: event.key,
        dir: event.direction,
        repeats: event.repeats,
      });
    } else {
      this.options.mark("nav.repeat-cancelled", { key: event.key, stage: event.stage });
    }
  }

  private startArrowScroll(direction: -1 | 1): void {
    if (this.arrowScrollDirection === direction) return;
    this.arrowScrollDirection = direction;
    this.arrowScrollPrevious = 0;
    if (!this.arrowScrollFrame) this.arrowScrollFrame = this.requestFrame(this.arrowScrollStep);
  }

  private readonly arrowScrollStep = (timestamp: number): void => {
    if (this.arrowScrollDirection === 0) return;
    if (!this.arrowScrollPrevious) this.arrowScrollPrevious = timestamp;
    const elapsed = timestamp - this.arrowScrollPrevious;
    this.arrowScrollPrevious = timestamp;
    const element = this.options.getViewWrapper()?.querySelector<HTMLElement>(".hide-scrollbar");
    if (element) element.scrollTop += this.arrowScrollDirection * SCROLL_PX_PER_SEC * (elapsed / 1000);
    this.arrowScrollFrame = this.requestFrame(this.arrowScrollStep);
  };

  private stopArrowScroll(): void {
    this.arrowScrollDirection = 0;
    if (!this.arrowScrollFrame) return;
    this.cancelFrame(this.arrowScrollFrame);
    this.arrowScrollFrame = 0;
  }

  private stopAll(): void {
    this.stopArrowScroll();
    this.stopHeld();
  }

  private requestFrame(callback: FrameRequestCallback): number {
    return (this.options.requestFrame ?? requestAnimationFrame)(callback);
  }

  private cancelFrame(id: number): void {
    (this.options.cancelFrame ?? cancelAnimationFrame)(id);
  }
}
