import { untrack } from "svelte";
import {
  bottomSheetDragHeight,
  bottomSheetHeights,
  bottomSheetReleaseVelocity,
  higherBottomSheetDetent,
  settleBottomSheet,
  type BottomSheetDetent,
  type BottomSheetHeights,
  type BottomSheetSample,
} from "$lib/mobile/bottom-sheet";
import { isTextEntryElement } from "$lib/mobile/keyboard";
import { TOUCH_SLOP_PX } from "$lib/utils/touch-hold";

/** Space between the summary rows and the bottom of a peeking sheet, in CSS pixels. */
const PEEK_PADDING_PX = 14;
/** Release samples kept for velocity; older ones fall outside the velocity window anyway. */
const MAX_SAMPLES = 8;
/** Space left above a fully open sheet so the page edge stays visible behind it. */
export const EVENT_PANEL_SHEET_TOP_GAP = "0.5rem";

export interface EventPanelSheetOptions {
  /** Whether the panel is a mobile bottom sheet; the controller does nothing otherwise. */
  enabled: () => boolean;
  /** Where a new panel session comes to rest. */
  initialDetent: () => BottomSheetDetent;
  /** Changes whenever the panel starts a new session for another event or draft. */
  sessionKey: () => unknown;
  /** Whether the user is moving or resizing the event on the grid behind the sheet. */
  gridEditing: () => boolean;
  /** Whether a picker or section that needs room is open inside the sheet. */
  needsRoom: () => boolean;
  panel: () => HTMLElement | undefined;
  header: () => HTMLElement | undefined;
  content: () => HTMLElement | undefined;
  /** The last row of the summary shown at the peek. */
  peekEnd: () => HTMLElement | undefined;
  scroll: () => HTMLElement | undefined;
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
 * Presents the mobile event panel as a bottom sheet that rests at a peek, half, or full height, follows vertical
 * drags on its header and content, and drops to its header while the user edits the event on the grid behind it.
 */
export class EventPanelSheetController {
  detent = $state<BottomSheetDetent>("peek");
  dragHeight = $state<number | null>(null);
  entered = $state(false);
  private fullHeight = $state(0);
  private headerHeight = $state(0);
  private peekHeight = $state(0);
  private gesture: SheetGesture | null = null;

  heights: BottomSheetHeights = $derived(
    bottomSheetHeights(this.fullHeight, this.headerHeight, this.peekHeight),
  );

  constructor(private readonly options: EventPanelSheetOptions) {
    $effect(() => {
      if (!options.enabled()) return;
      const panel = options.panel();
      const header = options.header();
      const content = options.content();
      const peekEnd = options.peekEnd();
      if (!panel || !header || !content || !peekEnd) return;
      const measure = () => this.measure(panel, header, content, peekEnd);
      const observer = new ResizeObserver(measure);
      for (const element of [panel, header, content, peekEnd]) observer.observe(element);
      measure();
      return () => observer.disconnect();
    });

    $effect(() => {
      void options.sessionKey();
      if (!options.enabled()) return;
      this.detent = options.initialDetent();
      this.dragHeight = null;
    });

    $effect(() => {
      if (!options.enabled() || this.entered || this.fullHeight <= 0) return;
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
      if (options.enabled() && options.needsRoom()) untrack(() => this.raise("full"));
    });

    $effect(() => {
      if (!options.enabled()) return;
      const panel = options.panel();
      if (!panel) return;
      const start = (event: TouchEvent) => this.handleTouchStart(event);
      const move = (event: TouchEvent) => this.handleTouchMove(event);
      const end = () => this.handleTouchEnd();
      const focus = (event: FocusEvent) => {
        if (isTextEntryElement(event.target instanceof Element ? event.target : null)) this.raise("full");
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
    if (this.options.gridEditing()) return this.heights.dip;
    return this.heights[this.detent];
  }

  /** Whether position changes animate, which they do except while a finger moves the sheet. */
  get animating(): boolean {
    return this.entered && this.dragHeight === null;
  }

  /** Inline position of the sheet: fixed to the bottom of the visible viewport and moved down by its hidden part. */
  get style(): string {
    const offset = Math.max(0, this.fullHeight - this.visibleHeight);
    return [
      "position:fixed",
      "z-index:50",
      "left:calc(var(--visual-viewport-offset-left) + var(--safe-area-left))",
      `top:calc(var(--visual-viewport-offset-top) + var(--safe-area-top) + ${EVENT_PANEL_SHEET_TOP_GAP})`,
      "width:calc(var(--visual-viewport-width) - var(--safe-area-left) - var(--safe-area-right))",
      `height:calc(var(--visual-viewport-height) - var(--safe-area-top) - ${EVENT_PANEL_SHEET_TOP_GAP})`,
      "padding-bottom:var(--safe-area-bottom)",
      `transform:translate3d(0, ${offset}px, 0)`,
      "",
    ].join("; ");
  }

  /** Open the sheet at least as far as `detent`. */
  raise(detent: BottomSheetDetent): void {
    this.detent = higherBottomSheetDetent(this.detent, detent);
  }

  private measure(panel: HTMLElement, header: HTMLElement, content: HTMLElement, peekEnd: HTMLElement): void {
    const paddingBottom = Number.parseFloat(getComputedStyle(panel).paddingBottom) || 0;
    this.fullHeight = panel.clientHeight - paddingBottom;
    this.headerHeight = header.offsetHeight;
    const summaryBottom = peekEnd.getBoundingClientRect().bottom - content.getBoundingClientRect().top;
    this.peekHeight = header.offsetHeight + summaryBottom + PEEK_PADDING_PX;
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
    const result = settleBottomSheet(released, bottomSheetReleaseVelocity(gesture.samples), this.heights, true);
    this.dragHeight = null;
    if (result === "close") {
      this.detent = "peek";
      this.options.onClose();
      return;
    }
    this.settle(result);
  }

  private cancelGesture(): void {
    this.gesture = null;
    this.dragHeight = null;
  }
}
