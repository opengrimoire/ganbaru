import type { Action } from "svelte/action";

const INTERACTIVE_SELECTOR = [
  "button",
  "a[href]",
  "input",
  "textarea",
  "select",
  "[contenteditable]:not([contenteditable='false'])",
  "[role='button']",
  "[role='textbox']",
  "[role='combobox']",
  "[role='listbox']",
  "[role='menu']",
  "[role='dialog']",
  "[data-app-shortcuts='ignore']",
].join(",");

function isInteractiveTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest(INTERACTIVE_SELECTOR) !== null;
}

/** Activate the current onboarding action with Enter while preserving focused controls. */
export const onboardingPrimaryAction: Action<HTMLButtonElement> = (button) => {
  const document = button.ownerDocument;
  const window = document.defaultView;
  if (!window) return;

  function handleKeydown(event: KeyboardEvent): void {
    if (
      event.key !== "Enter"
      || event.defaultPrevented
      || event.repeat
      || event.isComposing
      || event.altKey
      || event.ctrlKey
      || event.metaKey
      || event.shiftKey
      || !button.isConnected
      || button.matches(":disabled")
      || button.closest("[inert]")
      || isInteractiveTarget(event.target)
      || isInteractiveTarget(document.activeElement)
    ) return;

    event.preventDefault();
    button.click();
  }

  window.addEventListener("keydown", handleKeydown);
  return { destroy: () => window.removeEventListener("keydown", handleKeydown) };
};
