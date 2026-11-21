import { untrack } from "svelte";
import {
  bottomSheetDragHeight,
  bottomSheetReleaseVelocity,
  settleBottomSheet,
  type BottomSheetDetent,
  type BottomSheetHeights,
  type BottomSheetSample,
} from "$lib/mobile/bottom-sheet";
import { isTextEntryElement } from "$lib/mobile/keyboard";
import { prefersReducedMotion } from "$lib/utils/smooth-swap";
import { TOUCH_SLOP_PX } from "$lib/utils/touch-hold";
import {
  REPLY_THREAD_SHEET_PEEK_RATIO,
  REPLY_THREAD_SHEET_SETTLE_MS,
  replyThreadSheetClaimsDrag,
  replyThreadSheetHeights,
} from "./reply-thread-sheet";

/** Release samples kept for velocity; older ones fall outside the velocity window anyway. */
const MAX_SAMPLES = 8;

export interface ReplyThreadSheetOptions {
  /** The area the sheet may fill, from below the top safe area down to the keyboard or system navigation. */
  track: () => HTMLElement | undefined;
  /** The sheet, which receives the touches that move it. */
  sheet: () => HTMLElement | undefined;
  /** Runs once the sheet has slid out, so the thread closes after the motion ends. */
  onClose: () => void;
}

interface SheetGesture {
  startX: number;
  startY: number;
  target: Element | null;
  claimed: "sheet" | "none" | null;
  samples: BottomSheetSample[];
}

/** A drag that moves the sheet, measured from where the sheet took it over. */
interface SheetDrag {
  /** Rendered height when the drag began, which may sit partway through a settle. */
  startVisible: number;
  /** Track height when the drag began, so a keyboard sliding away mid-drag leaves the top edge under the finger. */
  startFull: number;
  originY: number;
  deltaY: number;
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

/**
 * Return whether text entry, a slider, a floating menu, or a scroller nested inside the thread entries sits under
 * `target` and keeps a vertical drag; the entries scroller itself is judged by its offset instead.
 */
function innerControlKeepsDrag(target: Element | null, scroll: HTMLElement | null, deltaY: number): boolean {
  for (let current = target; current && current !== scroll; current = current.parentElement) {
    if (current.matches("[data-reply-thread-sheet]")) return false;
    if (current.matches(".surface-floating")) return true;
    if (isTextEntryElement(current)) return true;
    if (current instanceof HTMLInputElement && current.type === "range") return true;
    if (current instanceof HTMLElement && canScrollToward(current, deltaY)) return true;
  }
  return false;
}

/**
 * Presents the phone reply thread as a bottom sheet that slides in nearly full, opens fully from a drag up on its
 * handle or when text entry inside it takes focus, follows drags down from its handle or the top of the entries, and
 * slides out before the thread closes. The sheet changes height instead of moving, so the composer docked at its
 * bottom stays in view at every height and floating menus inside it keep their screen positions. At rest the height is
 * a share of the track, so the sheet follows the keyboard without easing while a settle that is under way keeps going.
 */
export class ReplyThreadSheetController {
  detent = $state<BottomSheetDetent>("peek");
  entered = $state(false);
  closing = $state(false);
  private fullHeight = $state(0);
  private drag = $state<SheetDrag | null>(null);
  private gesture: SheetGesture | null = null;
  private closeTimer: ReturnType<typeof setTimeout> | null = null;

  heights: BottomSheetHeights = $derived(replyThreadSheetHeights(this.fullHeight));

  constructor(private readonly options: ReplyThreadSheetOptions) {
    $effect(() => {
      const track = options.track();
      if (!track) return;
      const measure = () => {
        this.fullHeight = track.clientHeight;
      };
      const observer = new ResizeObserver(measure);
      observer.observe(track);
      untrack(measure);
      return () => observer.disconnect();
    });

    $effect(() => {
      if (this.entered || this.closing || this.fullHeight <= 0) return;
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
      const sheet = options.sheet();
      if (!sheet) return;
      const start = (event: TouchEvent) => this.handleTouchStart(event);
      const move = (event: TouchEvent) => this.handleTouchMove(event);
      const end = () => this.handleTouchEnd();
      const focus = (event: FocusEvent) => {
        const target = event.target instanceof Element ? event.target : null;
        if (target && isTextEntryElement(target)) this.detent = "full";
      };
      sheet.addEventListener("touchstart", start, { passive: true });
      sheet.addEventListener("touchmove", move, { passive: false });
      sheet.addEventListener("touchend", end);
      sheet.addEventListener("touchcancel", end);
      sheet.addEventListener("focusin", focus);
      return () => {
        sheet.removeEventListener("touchstart", start);
        sheet.removeEventListener("touchmove", move);
        sheet.removeEventListener("touchend", end);
        sheet.removeEventListener("touchcancel", end);
        sheet.removeEventListener("focusin", focus);
      };
    });

    $effect(() => () => {
      if (this.closeTimer !== null) clearTimeout(this.closeTimer);
    });
  }

  /** Height under the finger while a drag moves the sheet, in CSS pixels, or null at rest. */
  get dragHeight(): number | null {
    const drag = this.drag;
    if (!drag) return null;
    return bottomSheetDragHeight(drag.startVisible, drag.deltaY, this.heights, this.fullHeight - drag.startFull);
  }

  /** Visible height of the sheet once its motion ends, in CSS pixels. */
  get visibleHeight(): number {
    const dragHeight = this.dragHeight;
    if (dragHeight !== null) return dragHeight;
    if (!this.entered || this.closing) return 0;
    return this.heights[this.detent];
  }

  /**
   * CSS height of the sheet: pixels under a finger, and otherwise a share of the track, which tracks a resizing track
   * on every frame without restarting the settle easing toward it.
   */
  get cssHeight(): string {
    const dragHeight = this.dragHeight;
    if (dragHeight !== null) return `${dragHeight}px`;
    if (!this.entered || this.closing) return "0%";
    return this.detent === "full" ? "100%" : `${REPLY_THREAD_SHEET_PEEK_RATIO * 100}%`;
  }

  /** How height changes animate, or null while a finger moves the sheet. */
  get motion(): "settle" | null {
    return this.drag === null ? "settle" : null;
  }

  /** Slide the sheet out, then close the thread. */
  close(): void {
    if (this.closing) return;
    this.closing = true;
    this.drag = null;
    this.gesture = null;
    this.closeTimer = setTimeout(() => {
      this.closeTimer = null;
      this.options.onClose();
    }, prefersReducedMotion() ? 0 : REPLY_THREAD_SHEET_SETTLE_MS);
  }

  private claimsVerticalDrag(target: Element | null, deltaY: number): boolean {
    const sheet = this.options.sheet();
    const scroll = sheet?.querySelector<HTMLElement>("[data-reply-thread-scroll]") ?? null;
    const onHandle = Boolean(target?.closest("[data-reply-thread-sheet-handle], [data-reply-thread-header]"));
    const inEntries = target !== null && scroll?.contains(target) === true;
    return replyThreadSheetClaimsDrag({
      onHandle,
      deltaY,
      scrollTop: inEntries ? scroll.scrollTop : 0,
      innerControlKeeps: innerControlKeepsDrag(target, inEntries ? scroll : null, deltaY),
    });
  }

  private handleTouchStart(event: TouchEvent): void {
    const touch = event.touches[0];
    if (event.touches.length !== 1 || !touch || this.closing) {
      this.gesture = null;
      this.drag = null;
      return;
    }
    this.gesture = {
      startX: touch.clientX,
      startY: touch.clientY,
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
      // The drag starts where the finger is now and from the height on screen, so the sheet neither jumps by the
      // slop nor snaps out of a settle that is still under way.
      this.drag = {
        startVisible: this.options.sheet()?.getBoundingClientRect().height ?? this.visibleHeight,
        startFull: this.fullHeight,
        originY: touch.clientY,
        deltaY: 0,
      };
      if (document.activeElement instanceof HTMLElement && isTextEntryElement(document.activeElement)) {
        document.activeElement.blur();
      }
    }
    if (event.cancelable) event.preventDefault();
    gesture.samples.push({ y: touch.clientY, time: event.timeStamp });
    if (gesture.samples.length > MAX_SAMPLES) gesture.samples.shift();
    if (this.drag) this.drag = { ...this.drag, deltaY: touch.clientY - this.drag.originY };
  }

  private handleTouchEnd(): void {
    const gesture = this.gesture;
    this.gesture = null;
    const released = this.dragHeight;
    if (!gesture || gesture.claimed !== "sheet" || released === null) return;
    const result = settleBottomSheet(released, bottomSheetReleaseVelocity(gesture.samples), this.heights, "close");
    if (result === "close") {
      this.close();
      return;
    }
    this.drag = null;
    if (result !== "collapse") this.detent = result;
  }
}
