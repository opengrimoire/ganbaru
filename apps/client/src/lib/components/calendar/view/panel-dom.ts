import type { PanelAnchor } from "$lib/components/calendar/edit-session.svelte";

interface OutsideCloseOptions {
  closeGuardMs: number;
  getConfirmOpen: () => boolean;
  getLastDragEndTime: () => number;
  isPanelCommitHidden: () => boolean;
  isSessionClosed: () => boolean;
  onClose: () => void;
}

export function fallbackPanelAnchor(): PanelAnchor {
  return { x: window.innerWidth / 2, y: window.innerHeight / 3, width: 0, height: 0 };
}

export function panelAnchorFromRect(rect: DOMRect | undefined): PanelAnchor {
  return rect
    ? { x: rect.right, y: rect.top, width: rect.width, height: rect.height }
    : fallbackPanelAnchor();
}

export function findRenderedEventElement(
  containerEl: HTMLElement | undefined,
  eventId: string,
): HTMLElement | undefined {
  if (!containerEl) return undefined;
  for (const el of containerEl.querySelectorAll<HTMLElement>("[data-event-id]")) {
    if (el.dataset.eventId === eventId) return el;
  }
  return undefined;
}

export function panelAnchorFromRenderedEvent(
  containerEl: HTMLElement | undefined,
  eventId: string,
): PanelAnchor {
  const el = findRenderedEventElement(containerEl, eventId);
  if (!el) return fallbackPanelAnchor();

  const eventRect = el.getBoundingClientRect();
  const columnRect = el.closest("[data-day-column]")?.getBoundingClientRect();
  if (columnRect) {
    return {
      x: columnRect.right,
      y: eventRect.top,
      width: columnRect.width,
      height: eventRect.height,
    };
  }

  return {
    x: eventRect.right,
    y: eventRect.top,
    width: eventRect.width,
    height: eventRect.height,
  };
}

function isPanelOrEventTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && (
      target.closest("[data-event-id]") !== null
      || target.closest("[data-create-preview]") !== null
      || target.closest(".panel-root") !== null
    );
}

function isConfirmDialogPanelTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(".confirm-dialog") !== null;
}

function isInteractiveControlTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && target.closest(
      "button, a[href], input, textarea, select, [contenteditable='true'], [role='button'], [role='menuitem'], [role='checkbox'], [role='switch'], [role='combobox']",
    ) !== null;
}

function isCalendarEditCloseTarget(target: EventTarget | null): boolean {
  return target instanceof Element
    && target.closest("[data-calendar-edit-close-ignore]") === null
    && target.closest("[data-calendar-edit-close-zone]") !== null;
}

export function createCalendarOutsideCloseAction({
  closeGuardMs,
  getConfirmOpen,
  getLastDragEndTime,
  isPanelCommitHidden,
  isSessionClosed,
  onClose,
}: OutsideCloseOptions) {
  let suppressOutsideClickUntil = 0;

  function handleOutsidePointerDown(e: PointerEvent): void {
    if (getConfirmOpen()) return;
    if (isPanelCommitHidden()) return;
    if (isSessionClosed()) return;
    if (isPanelOrEventTarget(e.target)) return;
    if (isInteractiveControlTarget(e.target)) return;
    if (!isCalendarEditCloseTarget(e.target)) return;

    suppressOutsideClickUntil = performance.now() + 750;
    e.preventDefault();
    e.stopPropagation();

    if (Date.now() - getLastDragEndTime() < closeGuardMs) return;
    onClose();
  }

  function handleOutsideClick(e: MouseEvent): void {
    if (getConfirmOpen() && isConfirmDialogPanelTarget(e.target)) return;
    if (performance.now() > suppressOutsideClickUntil) {
      suppressOutsideClickUntil = 0;
      return;
    }
    suppressOutsideClickUntil = 0;
    e.preventDefault();
    e.stopPropagation();
  }

  return function outsideClose(node: HTMLElement) {
    node.addEventListener("pointerdown", handleOutsidePointerDown, true);
    node.addEventListener("click", handleOutsideClick, true);
    return {
      destroy() {
        node.removeEventListener("pointerdown", handleOutsidePointerDown, true);
        node.removeEventListener("click", handleOutsideClick, true);
      },
    };
  };
}
