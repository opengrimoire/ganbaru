/** Axis along which a separator drag resizes its panel. */
export type PanelResizeAxis = "x" | "y";

/** How a panel takes part in a separator drag. */
export interface PanelPointerResizeOptions {
  /** `x` for a side column whose separator is on its start edge, `y` for a bottom panel whose separator is on top. */
  axis: PanelResizeAxis;
  /** Size of the panel when the drag starts. */
  currentSize: () => number;
  /** Maps the raw size the pointer asks for to a shown size, or zero when the drag reached the collapse zone. */
  sizeFromPointer: (requested: number) => number;
  /** Shows a size while dragging and when the drag ends. */
  applySize: (size: number) => void;
  /** Marks the drag active, so the shell can suspend transitions and text selection. */
  setResizing: (resizing: boolean) => void;
  /** Runs before the drag starts, for example to measure the shell. */
  beforeStart?: () => void;
  /** Receives every change of the pending size, before the next frame shows it. */
  onPendingSize?: (current: number, next: number) => void;
  /** Closes the panel when the drag ends in the collapse zone. */
  onCollapse: () => void;
  /** Runs after a drag that ends on another open size. */
  onCommit?: () => void;
}

/** Starts separator drags for one panel. */
export interface PanelPointerResize {
  /** Starts a drag from a primary-button pointer press on the separator. */
  begin: (event: PointerEvent) => void;
  /** Cancels the frames a drag scheduled, for when the separator unmounts. */
  dispose: () => void;
}

/**
 * Creates the pointer drag behavior of a panel separator. The drag captures the pointer on the separator, shows one
 * pending size per animation frame, restores the start size on cancel, closes the panel in the collapse zone, and keeps
 * the panel marked as resizing for two more frames so the final size lays out before transitions return.
 *
 * @param options How the panel reads, shows, and commits its size.
 * @returns The drag starter for the separator's `pointerdown` handler.
 */
export function createPanelPointerResize(options: PanelPointerResizeOptions): PanelPointerResize {
  let resizeFrame: number | null = null;
  let endFrame: number | null = null;

  function begin(event: PointerEvent): void {
    if (event.button !== 0 || !(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    options.beforeStart?.();
    if (endFrame !== null) window.cancelAnimationFrame(endFrame);
    endFrame = null;
    options.setResizing(true);
    const target = event.currentTarget;
    const startPosition = pointerPosition(event, options.axis);
    const startSize = options.currentSize();
    let pendingSize = startSize;
    target.focus();
    target.setPointerCapture(event.pointerId);
    const move = (moveEvent: PointerEvent) => {
      const nextSize = options.sizeFromPointer(startSize + startPosition - pointerPosition(moveEvent, options.axis));
      options.onPendingSize?.(pendingSize, nextSize);
      pendingSize = nextSize;
      if (resizeFrame !== null) return;
      resizeFrame = window.requestAnimationFrame(() => {
        options.applySize(pendingSize);
        resizeFrame = null;
      });
    };
    const end = (endEvent: PointerEvent) => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", end);
      target.removeEventListener("pointercancel", end);
      if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
      resizeFrame = null;
      if (endEvent.type === "pointercancel") {
        options.applySize(startSize);
      } else if (pendingSize === 0) {
        options.applySize(startSize);
        options.onCollapse();
      } else {
        options.applySize(pendingSize);
        if (pendingSize !== startSize) options.onCommit?.();
      }
      endFrame = window.requestAnimationFrame(() => {
        endFrame = window.requestAnimationFrame(() => {
          options.setResizing(false);
          endFrame = null;
        });
      });
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", end);
    target.addEventListener("pointercancel", end);
  }

  function dispose(): void {
    if (resizeFrame !== null) window.cancelAnimationFrame(resizeFrame);
    if (endFrame !== null) window.cancelAnimationFrame(endFrame);
    resizeFrame = null;
    endFrame = null;
  }

  return { begin, dispose };
}

function pointerPosition(event: PointerEvent, axis: PanelResizeAxis): number {
  return axis === "x" ? event.clientX : event.clientY;
}
