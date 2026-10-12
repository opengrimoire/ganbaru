export interface MusicDialogFocusOptions {
  onEscape: () => void;
  escapeDisabled?: boolean;
  onEnter?: () => void;
  enterDisabled?: boolean;
}

const FOCUSABLE_SELECTOR = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function focusableElements(node: HTMLElement): HTMLElement[] {
  return Array.from(node.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
    (element) => !element.hidden && element.getAttribute("aria-hidden") !== "true",
  );
}

/**
 * Contains keyboard focus in a Music modal and restores it when the modal closes.
 */
export function containMusicDialogFocus(
  node: HTMLElement,
  initialOptions: MusicDialogFocusOptions,
): { update: (options: MusicDialogFocusOptions) => void; destroy: () => void } {
  let options = initialOptions;
  const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;

  const focusInitial = (): void => {
    const preferred = node.querySelector<HTMLElement>("[data-dialog-autofocus]");
    (preferred ?? focusableElements(node)[0] ?? node).focus();
  };

  const handleKeydown = (event: KeyboardEvent): void => {
    if (event.key === "Escape" && !options.escapeDisabled) {
      event.preventDefault();
      event.stopPropagation();
      options.onEscape();
      return;
    }
    if (event.key === "Enter" && options.onEnter && !options.enterDisabled) {
      event.preventDefault();
      event.stopPropagation();
      options.onEnter();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = focusableElements(node);
    if (focusable.length === 0) {
      event.preventDefault();
      node.focus();
      return;
    }
    const activeIndex = focusable.indexOf(document.activeElement as HTMLElement);
    const nextIndex = event.shiftKey
      ? activeIndex <= 0 ? focusable.length - 1 : activeIndex - 1
      : activeIndex < 0 || activeIndex === focusable.length - 1 ? 0 : activeIndex + 1;
    if (activeIndex < 0 || (event.shiftKey && activeIndex === 0) || (!event.shiftKey && activeIndex === focusable.length - 1)) {
      event.preventDefault();
      focusable[nextIndex]?.focus();
    }
  };

  node.addEventListener("keydown", handleKeydown);
  queueMicrotask(focusInitial);

  return {
    update(nextOptions) {
      options = nextOptions;
    },
    destroy() {
      node.removeEventListener("keydown", handleKeydown);
      if (previousFocus?.isConnected) queueMicrotask(() => previousFocus.focus());
    },
  };
}
