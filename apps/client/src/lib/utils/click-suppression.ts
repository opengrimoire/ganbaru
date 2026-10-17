function swallowClick(event: MouseEvent): void {
  event.preventDefault();
  event.stopPropagation();
}

/**
 * Swallow the click that a browser fires right after the pointer release ending a drag, so dropping an item does not
 * also activate it. A release that produces no click leaves nothing behind once the current task ends.
 */
export function suppressClickAfterRelease(): void {
  window.addEventListener("click", swallowClick, { capture: true, once: true });
  window.setTimeout(() => window.removeEventListener("click", swallowClick, { capture: true }), 0);
}
