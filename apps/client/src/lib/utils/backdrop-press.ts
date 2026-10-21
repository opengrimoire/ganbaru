/** Tracks where a modal backdrop press began so only a press that starts and ends on the same element counts. */
export interface BackdropPress {
  /** Records the element a press starts on; bind it to the backdrop's `pointerdown`. */
  start: (event: Event) => void;
  /**
   * Whether this click is a real backdrop click. A press that starts inside the dialog, such as dragging a slider
   * thumb, and ends over the backdrop produces a click on their common ancestor, which this rejects.
   */
  isClick: (event: Event) => boolean;
}

/** Creates the press tracker for one modal backdrop. */
export function createBackdropPress(): BackdropPress {
  let pressed: EventTarget | null = null;
  return {
    start(event) {
      pressed = event.target;
    },
    isClick(event) {
      const same = pressed !== null && pressed === event.target;
      pressed = null;
      return same;
    },
  };
}
