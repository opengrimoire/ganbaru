const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

type ModalKeyboardLayer =
  | { id: symbol; kind: "exclusive"; handle: (event: KeyboardEvent) => void }
  | { id: symbol; kind: "isolating"; handle: (event: KeyboardEvent) => boolean };

let modalKeyboardLayers: ModalKeyboardLayer[] = [];
let modalKeyboardRouterTarget: Window | null = null;

function routeModalKeyboardEvent(event: KeyboardEvent): void {
  const layer = modalKeyboardLayers.at(-1);
  if (!layer) return;
  if (layer.kind === "isolating") {
    if (layer.handle(event)) event.stopImmediatePropagation();
    return;
  }
  try {
    layer.handle(event);
  } finally {
    event.stopImmediatePropagation();
  }
}

/** Install the modal keyboard router before application-level shortcut listeners. */
export function installModalKeyboardRouter(target: Window = window): () => void {
  if (modalKeyboardRouterTarget === target) return () => undefined;
  if (modalKeyboardRouterTarget) {
    modalKeyboardRouterTarget.removeEventListener("keydown", routeModalKeyboardEvent, true);
  }
  modalKeyboardRouterTarget = target;
  target.addEventListener("keydown", routeModalKeyboardEvent, true);
  return () => {
    if (modalKeyboardRouterTarget !== target) return;
    target.removeEventListener("keydown", routeModalKeyboardEvent, true);
    modalKeyboardRouterTarget = null;
  };
}

/** Give the most recently opened modal first ownership of keyboard input. */
export function activateModalKeyboardLayer(
  handle: (event: KeyboardEvent) => void,
): () => void {
  return pushModalKeyboardLayer({ id: Symbol("modal-keyboard-layer"), kind: "exclusive", handle });
}

/**
 * Give a modal panel first look at keyboard input while the controls inside it keep receiving keys. `handle` returns
 * whether it claimed the event, which then stops there. While the layer is active, window-wide app and view shortcuts
 * stay inactive (see `isAppKeyboardIsolated`), so keys the panel leaves alone do not reach the app behind it.
 */
export function activateIsolatingKeyboardLayer(
  handle: (event: KeyboardEvent) => boolean,
): () => void {
  return pushModalKeyboardLayer({ id: Symbol("isolating-keyboard-layer"), kind: "isolating", handle });
}

/** Whether a modal panel owns the keyboard, so window-wide app and view shortcuts must not act. */
export function isAppKeyboardIsolated(): boolean {
  return modalKeyboardLayers.some((layer) => layer.kind === "isolating");
}

function pushModalKeyboardLayer(layer: ModalKeyboardLayer): () => void {
  if (!modalKeyboardRouterTarget && typeof window !== "undefined") {
    installModalKeyboardRouter(window);
  }
  modalKeyboardLayers = [...modalKeyboardLayers, layer];
  return () => {
    modalKeyboardLayers = modalKeyboardLayers.filter((candidate) => candidate.id !== layer.id);
  };
}

function visibleFocusableElements(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)]
    .filter((element) => !element.hidden && element.getAttribute("aria-hidden") !== "true");
}

/** Keep a Tab key event inside a modal container. */
export function trapModalTabKey(container: HTMLElement, event: KeyboardEvent): boolean {
  if (event.key !== "Tab" || container.closest("[inert]")) return false;
  const focusable = visibleFocusableElements(container);
  if (focusable.length === 0) {
    event.preventDefault();
    container.focus();
    return true;
  }

  const first = focusable[0];
  const last = focusable.at(-1);
  if (!first || !last) return false;
  const active = document.activeElement;
  if (event.shiftKey && (active === first || !container.contains(active))) {
    event.preventDefault();
    last.focus();
    return true;
  }
  if (!event.shiftKey && (active === last || !container.contains(active))) {
    event.preventDefault();
    first.focus();
    return true;
  }
  return false;
}

/**
 * Capture the control that opens a surface so closing the surface can return focus.
 *
 * The returned function returns focus to the opener only when the opener had keyboard focus
 * (`:focus-visible`). A pointer-opened surface does not leave a focused trigger behind, so Space
 * or Enter cannot silently reopen it or activate a control in a hidden surface. Focus the user
 * already moved outside the surface is left alone, and focus stranded in a hidden surface is
 * released to the nearest dialog that contained the opener, or to the document.
 *
 * @returns A function to call when the surface closes, given the closing surface element.
 */
export function captureFocusReturn(): (surface?: HTMLElement | null) => void {
  const active = document.activeElement;
  const opener = active instanceof HTMLElement && active !== document.body ? active : null;
  const restoreOpener = opener?.matches(":focus-visible") ?? false;
  const openerDialog = opener?.parentElement?.closest<HTMLElement>("[role='dialog']") ?? null;

  return (surface) => {
    queueMicrotask(() => {
      const current = document.activeElement;
      const focusMovedElsewhere = current instanceof HTMLElement
        && current !== document.body
        && !(surface?.contains(current) ?? false);
      if (focusMovedElsewhere) return;
      if (restoreOpener && opener?.isConnected) {
        opener.focus({ preventScroll: true });
        return;
      }
      if (openerDialog?.isConnected) {
        openerDialog.focus({ preventScroll: true });
        if (document.activeElement === openerDialog) return;
      }
      if (current instanceof HTMLElement && current !== document.body) current.blur();
    });
  };
}

/** Focus a modal, contain Tab navigation, and return focus when it closes. */
export function activateModalFocus(
  container: HTMLElement,
  initialFocus: HTMLElement | null = null,
): () => void {
  const returnFocus = captureFocusReturn();
  const handleKeydown = (event: KeyboardEvent): void => {
    trapModalTabKey(container, event);
  };
  document.addEventListener("keydown", handleKeydown, true);
  queueMicrotask(() => {
    const target = initialFocus ?? visibleFocusableElements(container)[0] ?? container;
    target.focus();
  });

  return () => {
    document.removeEventListener("keydown", handleKeydown, true);
    returnFocus(container);
  };
}
