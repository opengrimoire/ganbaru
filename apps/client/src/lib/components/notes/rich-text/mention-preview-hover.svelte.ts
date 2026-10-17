const PREVIEW_OPEN_DELAY_MS = 300;
const PREVIEW_CLOSE_DELAY_MS = 120;

interface NotesMentionPreviewHoverOptions {
  /** The chip whose document selection decides whether hover intent is text selection instead. */
  anchor: () => HTMLElement | null;
  /** Whether the chip has anything to preview. */
  canOpen: () => boolean;
  /** Start loading preview content once the card opens. */
  onOpen: () => void;
}

/**
 * Hover intent for a mention preview card: opens after a short dwell with a mouse or pen, stays open while the
 * pointer crosses to the card, and never opens for touch, drags, or ranged text selections.
 */
export class NotesMentionPreviewHover {
  open = $state(false);
  #options: NotesMentionPreviewHoverOptions;
  #openTimer: ReturnType<typeof setTimeout> | null = null;
  #closeTimer: ReturnType<typeof setTimeout> | null = null;
  #disposed = false;

  constructor(options: NotesMentionPreviewHoverOptions) {
    this.#options = options;
  }

  /** Whether the card is open or about to open, so Escape belongs to it. */
  get pending(): boolean {
    return this.open || this.#openTimer !== null;
  }

  schedule(event: PointerEvent): void {
    if (this.#disposed || !this.#options.canOpen() || event.pointerType === "touch" || event.buttons) return;
    this.keepOpen();
    if (this.pending) return;
    this.#openTimer = setTimeout(() => {
      this.#openTimer = null;
      const selection = this.#options.anchor()?.ownerDocument.getSelection();
      if (this.#disposed || (selection && !selection.isCollapsed)) return;
      this.open = true;
      this.#options.onOpen();
    }, PREVIEW_OPEN_DELAY_MS);
  }

  scheduleClose(): void {
    this.#clearOpenTimer();
    this.keepOpen();
    this.#closeTimer = setTimeout(() => this.close(), PREVIEW_CLOSE_DELAY_MS);
  }

  keepOpen(): void {
    if (this.#closeTimer !== null) clearTimeout(this.#closeTimer);
    this.#closeTimer = null;
  }

  /** Open immediately, for example to explain why activating the chip failed. */
  show(): void {
    if (this.#disposed) return;
    this.#clearOpenTimer();
    this.keepOpen();
    this.open = true;
  }

  /** Close without changing native text selection or the active editor. */
  close(): void {
    this.#clearOpenTimer();
    this.keepOpen();
    this.open = false;
  }

  dispose(): void {
    this.#disposed = true;
    this.close();
  }

  #clearOpenTimer(): void {
    if (this.#openTimer !== null) clearTimeout(this.#openTimer);
    this.#openTimer = null;
  }
}
