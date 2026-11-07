import { untrack } from "svelte";
import {
  bottomSheetDragHeight,
  bottomSheetHeights,
  bottomSheetReleaseVelocity,
  higherBottomSheetDetent,
  revealScrollDelta,
  settleBottomSheet,
  type BottomSheetDetent,
  type BottomSheetHeights,
  type BottomSheetSample,
} from "$lib/mobile/bottom-sheet";
import { isTextEntryElement } from "$lib/mobile/keyboard";
import { prefersReducedMotion } from "$lib/utils/smooth-swap";
import { TOUCH_SLOP_PX } from "$lib/utils/touch-hold";

/** Space between the last row a peek or half detent shows and the bottom of the sheet, in CSS pixels. */
export const EVENT_PANEL_SHEET_PEEK_PADDING_PX = 10;
/** Release samples kept for velocity; older ones fall outside the velocity window anyway. */
const MAX_SAMPLES = 8;
/** Space kept between the revealed event and the grid or sheet edges, in CSS pixels. */
const REVEAL_MARGIN_PX = 16;
/** Space left above a fully open sheet so the page edge stays visible behind it. */
export const EVENT_PANEL_SHEET_TOP_GAP = "0.5rem";
/**
 * Shortest time the sheet stays lowered for an edit on the grid, in milliseconds, so after a quick edit the sheet lands
 * before it rises again instead of reversing midway.
 */
const MIN_GRID_EDIT_DIP_MS = 450;

/**
 * How the sheet animates to a new resting height: `dip` eases slowly down and back around an edit on the grid, `room`
 * rises in step with a section sliding open, and `settle` is the quick default.
 */
export type EventPanelSheetMotion = "dip" | "room" | "settle";

export interface EventPanelSheetOptions {
  /** Whether the panel is a mobile bottom sheet; the controller does nothing otherwise. */
  enabled: () => boolean;
  /** Whether the panel session is ready to be seen, which starts the slide in from the bottom. */
  shown: () => boolean;
  /** Where a new panel session comes to rest. */
  initialDetent: () => BottomSheetDetent;
  /** Changes whenever the panel starts a new session for another event or draft. */
  sessionKey: () => unknown;
  /** Whether a swipe down or a scroll of the grid behind lowers the sheet to its summary strip instead of closing it. */
  collapsible: () => boolean;
  /** Whether the user is moving or resizing the event on the grid behind the sheet. */
  gridEditing: () => boolean;
  /** Whether a picker or section that needs room is open inside the sheet. */
  needsRoom: () => boolean;
  panel: () => HTMLElement | undefined;
  header: () => HTMLElement | undefined;
  /** The last header element shown while the user edits on the grid; the sheet lowers to its bottom edge then. */
  dipEnd: () => HTMLElement | undefined;
  content: () => HTMLElement | undefined;
  /** The last row of the summary shown at the peek. */
  peekEnd: () => HTMLElement | undefined;
  /** The last element of the overview shown at the half detent, measured while nothing inside needs room. */
  halfEnd: () => HTMLElement | undefined;
  scroll: () => HTMLElement | undefined;
  /** Calendar event kept visible on the grid above the sheet while it rests. */
  revealEventId: () => string | undefined;
  onClose: () => void;
}

interface SheetGesture {
  startX: number;
  startY: number;
  startVisible: number;
  target: Element | null;
  claimed: "sheet" | "none" | null;
  samples: BottomSheetSample[];
}

/** Return whether `element` scrolls on its own in the direction a finger moving `deltaY` would scroll it. */
function canScrollToward(element: HTMLElement, deltaY: number): boolean {
  const { overflowY } = getComputedStyle(element);
  if (overflowY !== "auto" && overflowY !== "scroll") return false;
  if (element.scrollHeight <= element.clientHeight + 1) return false;
  return deltaY > 0
    ? element.scrollTop > 0
    : element.scrollTop + element.clientHeight < element.scrollHeight - 1;
}

/** Return whether a nested scroller or slider between `target` and `boundary` should keep a vertical drag. */
function nestedControlKeepsDrag(target: Element | null, boundary: HTMLElement | undefined, deltaY: number): boolean {
  for (let current = target; current && current !== boundary; current = current.parentElement) {
    if (current instanceof HTMLInputElement && current.type === "range") return true;
    if (current instanceof HTMLElement && canScrollToward(current, deltaY)) return true;
  }
  return false;
}

/**
 * Return whether a touch on `target` scrolls the calendar grid behind the sheet rather than moving the event, its
 * knobs, or the sheet itself.
 */
function scrollsGridBehind(target: EventTarget | null, panel: HTMLElement | undefined): boolean {
  if (!(target instanceof Element) || panel?.contains(target)) return false;
  if (target.closest("[data-calendar-scroll-container]") === null) return false;
  return target.closest("[data-event-id], [data-calendar-edit-close-ignore]") === null;
}

/**
 * Presents the mobile event panel as a bottom sheet that rests at a peek, half, or full height, follows vertical
 * drags on its header and content, slides in from the bottom when shown, and lowers to a summary strip while the user
 * edits the event on the grid behind it, staying there briefly after a quick edit so it never reverses midway. A
 * collapsible sheet also rests at that strip after a swipe down or a scroll of the grid instead of closing.
 */
export class EventPanelSheetController {
  detent = $state<BottomSheetDetent>("peek");
  dragHeight = $state<number | null>(null);
  entered = $state(false);
  /** Whether the sheet rests at its summary strip after a swipe down or a scroll of the grid behind it. */
  collapsed = $state(false);
  /** How the latest resting change animates; see {@link EventPanelSheetMotion}. */
  private restMotion = $state<EventPanelSheetMotion>("settle");
  /** Whether the sheet is lowered for an edit on the grid, held for {@link MIN_GRID_EDIT_DIP_MS} after a quick edit. */
  private gridEditDip = $state(false);
  private gridEditDipStartedAt = 0;
  private fullHeight = $state(0);
  private dipHeight = $state(0);
  private peekHeight = $state(0);
  private halfHeight = $state(0);
  private gesture: SheetGesture | null = null;
  /**
   * Option values held as derived primitives, so effects rerun only when a value changes, not whenever the parent
   * rebuilds the props behind it, such as after every draft edit.
   */
  private readonly enabled = $derived.by(() => this.options.enabled());
  private readonly sessionKey = $derived.by(() => this.options.sessionKey());
  private readonly collapsible = $derived.by(() => this.options.collapsible());
  private readonly needsRoom = $derived.by(() => this.options.needsRoom());

  heights: BottomSheetHeights = $derived(
    bottomSheetHeights(this.fullHeight, this.dipHeight, this.peekHeight, this.halfHeight),
  );

  constructor(private readonly options: EventPanelSheetOptions) {
    $effect(() => {
      if (!this.enabled) return;
      // Measures again once an open section or picker closes, so the overview height drops what it added.
      void this.needsRoom;
      const panel = options.panel();
      const header = options.header();
      const dipEnd = options.dipEnd();
      const content = options.content();
      const peekEnd = options.peekEnd();
      const halfEnd = options.halfEnd();
      if (!panel || !header || !dipEnd || !content || !peekEnd || !halfEnd) return;
      const measure = () => this.measure(panel, header, dipEnd, content, peekEnd, halfEnd);
      const observer = new ResizeObserver(measure);
      for (const element of [panel, header, dipEnd, content, peekEnd, halfEnd]) observer.observe(element);
      untrack(measure);
      return () => observer.disconnect();
    });

    $effect(() => {
      void this.sessionKey;
      if (!this.enabled) return;
      this.detent = untrack(() => options.initialDetent());
      this.collapsed = false;
      this.restMotion = "settle";
      this.dragHeight = null;
    });

    $effect(() => {
      if (!this.enabled || !options.shown()) this.entered = false;
    });

    $effect(() => {
      if (this.enabled && options.gridEditing()) {
        untrack(() => {
          if (!this.gridEditDip) this.gridEditDipStartedAt = performance.now();
          this.gridEditDip = true;
          this.restMotion = "dip";
        });
        return;
      }
      if (!untrack(() => this.gridEditDip)) return;
      const remaining = MIN_GRID_EDIT_DIP_MS - (performance.now() - this.gridEditDipStartedAt);
      const timer = setTimeout(() => {
        this.gridEditDip = false;
      }, Math.max(0, remaining));
      return () => clearTimeout(timer);
    });

    $effect(() => {
      if (!this.enabled || !options.shown() || this.entered || this.fullHeight <= 0) return;
      let second = 0;
      const first = requestAnimationFrame(() => {
        second = requestAnimationFrame(() => {
          this.entered = true;
        });
      });
      return () => {
        cancelAnimationFrame(first);
        cancelAnimationFrame(second);
      };
    });

    $effect(() => {
      if (this.enabled && this.needsRoom) untrack(() => this.raise("full", "room"));
    });

    $effect(() => {
      const eventId = options.revealEventId();
      const visible = this.visibleHeight;
      // A collapsed sheet leaves the grid where the user scrolled it.
      if (!this.enabled || !eventId || !this.entered || this.dragHeight !== null || this.gridEditDip
        || this.collapsed) return;
      const frame = requestAnimationFrame(() => this.revealEvent(eventId, visible));
      return () => cancelAnimationFrame(frame);
    });

    $effect(() => {
      if (!this.enabled || !this.collapsible) return;
      let start: { x: number; y: number } | null = null;
      const begin = (event: TouchEvent) => {
        start = null;
        const touch = event.touches[0];
        if (event.touches.length !== 1 || !touch || !this.entered || this.collapsed) return;
        if (!scrollsGridBehind(event.target, options.panel())) return;
        start = { x: touch.clientX, y: touch.clientY };
      };
      const move = (event: TouchEvent) => {
        const touch = event.touches[0];
        if (!start || !touch || options.gridEditing()) return;
        if (Math.hypot(touch.clientX - start.x, touch.clientY - start.y) <= TOUCH_SLOP_PX) return;
        start = null;
        this.collapse();
      };
      const end = () => {
        start = null;
      };
      const listener = { capture: true, passive: true };
      window.addEventListener("touchstart", begin, listener);
      window.addEventListener("touchmove", move, listener);
      window.addEventListener("touchend", end, listener);
      window.addEventListener("touchcancel", end, listener);
      return () => {
        window.removeEventListener("touchstart", begin, listener);
        window.removeEventListener("touchmove", move, listener);
        window.removeEventListener("touchend", end, listener);
        window.removeEventListener("touchcancel", end, listener);
      };
    });

    $effect(() => {
      if (!this.enabled) return;
      const panel = options.panel();
      if (!panel) return;
      const start = (event: TouchEvent) => this.handleTouchStart(event);
      const move = (event: TouchEvent) => this.handleTouchMove(event);
      const end = () => this.handleTouchEnd();
      const focus = (event: FocusEvent) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target && isTextEntryElement(target) && !this.shows(target)) this.raise("full");
      };
      panel.addEventListener("touchstart", start, { passive: true });
      panel.addEventListener("touchmove", move, { passive: false });
      panel.addEventListener("touchend", end);
      panel.addEventListener("touchcancel", end);
      panel.addEventListener("focusin", focus);
      return () => {
        panel.removeEventListener("touchstart", start);
        panel.removeEventListener("touchmove", move);
        panel.removeEventListener("touchend", end);
        panel.removeEventListener("touchcancel", end);
        panel.removeEventListener("focusin", focus);
      };
    });
  }

  /** Whether the sheet is measured and can be shown. */
  get ready(): boolean {
    return this.fullHeight > 0;
  }

  /** Visible height of the sheet right now, in CSS pixels. */
  get visibleHeight(): number {
    if (this.dragHeight !== null) return this.dragHeight;
    if (!this.entered) return 0;
    return this.restingHeight;
  }

  /** Whether the sheet shows only its summary strip, for an edit on the grid or after it collapsed. */
  get lowered(): boolean {
    return this.gridEditDip || this.collapsed;
  }

  /** Height the sheet rests at when no finger moves it: its summary strip while lowered. */
  private get restingHeight(): number {
    return this.lowered ? this.heights.dip : this.heights[this.detent];
  }

  /** How position changes animate, or null while a finger moves the sheet or before it slides in. */
  get motion(): EventPanelSheetMotion | null {
    if (!this.entered || this.dragHeight !== null) return null;
    return this.restMotion;
  }

  /**
   * Inline style of the frame that holds the sheet: the visible viewport down to the system navigation (or the
   * keyboard), clipping the sheet's hidden part so it never draws over the navigation buttons. The frame never moves,
   * so the clip stays still while the sheet's transform animates on the compositor; animating a clip alongside the
   * transform runs on another thread and lets the sheet's bottom edge lag above the navigation for a few frames.
   */
  get frameStyle(): string {
    return [
      "position:fixed",
      "z-index:50",
      "left:var(--visual-viewport-offset-left)",
      "top:var(--visual-viewport-offset-top)",
      "width:var(--visual-viewport-width)",
      "height:calc(var(--visual-viewport-height) - var(--safe-area-bottom))",
      "overflow:clip",
      "pointer-events:none",
      "",
    ].join("; ");
  }

  /** Inline position of the sheet inside its frame: below the top safe area, moved down by its hidden part. */
  get style(): string {
    const offset = Math.max(0, this.fullHeight - this.visibleHeight);
    return [
      "position:absolute",
      "pointer-events:auto",
      "left:var(--safe-area-left)",
      `top:calc(var(--safe-area-top) + ${EVENT_PANEL_SHEET_TOP_GAP})`,
      "width:calc(var(--visual-viewport-width) - var(--safe-area-left) - var(--safe-area-right))",
      `height:calc(var(--visual-viewport-height) - var(--safe-area-top) - var(--safe-area-bottom) - ${EVENT_PANEL_SHEET_TOP_GAP})`,
      `transform:translate3d(0, ${offset}px, 0)`,
      "",
    ].join("; ");
  }

  /** Open the sheet at least as far as `detent`, animating with `motion`. */
  raise(detent: BottomSheetDetent, motion: "room" | "settle" = "settle"): void {
    this.detent = higherBottomSheetDetent(this.collapsed ? "peek" : this.detent, detent);
    this.collapsed = false;
    this.restMotion = motion;
  }

  /** Lower the sheet to its summary strip, leaving it at the peek once it opens again. */
  collapse(): void {
    if (this.collapsed) return;
    this.collapsed = true;
    this.restMotion = "settle";
    this.detent = "peek";
    this.options.scroll()?.scrollTo({ top: 0, behavior: "smooth" });
  }

  private measure(
    panel: HTMLElement,
    header: HTMLElement,
    dipEnd: HTMLElement,
    content: HTMLElement,
    peekEnd: HTMLElement,
    halfEnd: HTMLElement,
  ): void {
    this.fullHeight = panel.clientHeight;
    this.dipHeight = dipEnd.getBoundingClientRect().bottom - panel.getBoundingClientRect().top;
    const contentTop = content.getBoundingClientRect().top;
    this.peekHeight = header.offsetHeight + peekEnd.getBoundingClientRect().bottom - contentTop
      + EVENT_PANEL_SHEET_PEEK_PADDING_PX;
    // An open section or picker grows the overview, so the half detent keeps the height of the closed one.
    if (this.needsRoom) return;
    this.halfHeight = header.offsetHeight + halfEnd.getBoundingClientRect().bottom - contentTop
      + EVENT_PANEL_SHEET_PEEK_PADDING_PX;
  }

  /** Whether `element` shows inside the sheet at the height it rests at, so focusing it needs no more room. */
  private shows(element: Element): boolean {
    const panel = this.options.panel();
    if (!panel) return false;
    return element.getBoundingClientRect().bottom - panel.getBoundingClientRect().top <= this.restingHeight;
  }

  /** Scroll the grid holding `eventId` so the event sits above a sheet resting at `visible` height. */
  private revealEvent(eventId: string, visible: number): void {
    const panel = this.options.panel();
    const target = document.querySelector(
      `[data-calendar-scroll-container] [data-event-id="${CSS.escape(eventId)}"]`,
    );
    const container = target?.closest<HTMLElement>("[data-calendar-scroll-container]");
    if (!panel || !target || !container) return;
    const panelRect = panel.getBoundingClientRect();
    const currentOffset = new DOMMatrixReadOnly(getComputedStyle(panel).transform).m42;
    const sheetTop = panelRect.top - currentOffset + Math.max(0, this.fullHeight - visible);
    const containerRect = container.getBoundingClientRect();
    const delta = revealScrollDelta(
      target.getBoundingClientRect(),
      { top: containerRect.top, bottom: Math.min(containerRect.bottom, sheetTop) },
      REVEAL_MARGIN_PX,
    );
    if (delta !== 0) container.scrollBy({ top: delta, behavior: prefersReducedMotion() ? "auto" : "smooth" });
  }

  private settle(detent: BottomSheetDetent): void {
    this.detent = detent;
    if (detent !== "full") this.options.scroll()?.scrollTo({ top: 0, behavior: "smooth" });
  }

  private claimsVerticalDrag(target: Element | null, deltaY: number): boolean {
    if (target && this.options.header()?.contains(target)) return true;
    const scroll = this.options.scroll();
    if (nestedControlKeepsDrag(target, scroll, deltaY)) return false;
    if (this.detent !== "full") return true;
    return deltaY > 0 && (scroll?.scrollTop ?? 0) <= 0;
  }

  private handleTouchStart(event: TouchEvent): void {
    const touch = event.touches[0];
    if (event.touches.length !== 1 || !touch || this.options.gridEditing()) {
      this.cancelGesture();
      return;
    }
    this.gesture = {
      startX: touch.clientX,
      startY: touch.clientY,
      startVisible: this.visibleHeight,
      target: event.target instanceof Element ? event.target : null,
      claimed: null,
      samples: [{ y: touch.clientY, time: event.timeStamp }],
    };
  }

  private handleTouchMove(event: TouchEvent): void {
    const gesture = this.gesture;
    const touch = event.touches[0];
    if (!gesture || gesture.claimed === "none" || !touch) return;
    const deltaX = touch.clientX - gesture.startX;
    const deltaY = touch.clientY - gesture.startY;
    if (gesture.claimed === null) {
      if (Math.hypot(deltaX, deltaY) <= TOUCH_SLOP_PX) return;
      gesture.claimed = Math.abs(deltaY) > Math.abs(deltaX) && this.claimsVerticalDrag(gesture.target, deltaY)
        ? "sheet"
        : "none";
      if (gesture.claimed === "none") return;
      if (document.activeElement instanceof HTMLElement && isTextEntryElement(document.activeElement)) {
        document.activeElement.blur();
      }
    }
    if (event.cancelable) event.preventDefault();
    gesture.samples.push({ y: touch.clientY, time: event.timeStamp });
    if (gesture.samples.length > MAX_SAMPLES) gesture.samples.shift();
    this.dragHeight = bottomSheetDragHeight(gesture.startVisible, deltaY, this.heights);
  }

  private handleTouchEnd(): void {
    const gesture = this.gesture;
    this.gesture = null;
    const released = this.dragHeight;
    if (!gesture || gesture.claimed !== "sheet" || released === null) return;
    const low = this.collapsible ? "collapse" : "close";
    const result = settleBottomSheet(released, bottomSheetReleaseVelocity(gesture.samples), this.heights, low);
    this.dragHeight = null;
    this.restMotion = "settle";
    if (!this.options.gridEditing()) this.gridEditDip = false;
    if (result === "close") {
      this.detent = "peek";
      this.options.onClose();
      return;
    }
    if (result === "collapse") {
      this.collapse();
      return;
    }
    this.collapsed = false;
    this.settle(result);
  }

  private cancelGesture(): void {
    this.gesture = null;
    this.dragHeight = null;
  }
}
