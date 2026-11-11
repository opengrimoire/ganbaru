import { scrollEdgeFadeAction, SCROLL_FADE_OWNER_ATTRIBUTE } from "./scroll-edge-fade";

/**
 * Subtrees marked with this attribute get no edge fade on the mobile shell, though their scrollbars stay hidden like
 * everywhere else. The Calendar uses it so the fade does not mask its time grid.
 */
export const MOBILE_SCROLL_FADE_OFF_ATTRIBUTE = "data-mobile-scroll-fade-off";

/** Marks a vertical scroll container styled in a component stylesheet, which the class selector cannot see. */
export const MOBILE_SCROLL_FADE_ATTRIBUTE = "data-mobile-scroll-fade";

const FADE_OFF_SELECTOR = `[${MOBILE_SCROLL_FADE_OFF_ATTRIBUTE}]`;

/** Vertical scroll containers the mobile shell fades, recognized by their scroll utilities or the explicit marker. */
export const MOBILE_SCROLL_CONTAINER_SELECTOR = [
  ".overflow-y-auto",
  ".overflow-y-scroll",
  ".overflow-auto",
  `[${MOBILE_SCROLL_FADE_ATTRIBUTE}]`,
].join(", ");

/**
 * Containers the manager leaves alone: text entry keeps its edges crisp while the caret moves, and a floating surface or
 * dialog that scrolls itself would lose its shadow and border to the mask, which clips to its box.
 */
const EXCLUDED_SELECTOR = [
  "input",
  "textarea",
  "[contenteditable]:not([contenteditable='false'])",
  ".surface-floating",
  ".surface-dialog",
].join(", ");

/** Whether `element` sits in a subtree that opts out of the mobile edge fade. */
export function optsOutOfMobileScrollFade(element: Element): boolean {
  return element.closest(FADE_OFF_SELECTOR) !== null;
}

/** Whether the mobile manager may fade `element`: a recognized scroll container outside opted-out subtrees, text entry, and floating surfaces. */
export function isMobileScrollFadeCandidate(element: Element): element is HTMLElement {
  return (
    element instanceof HTMLElement &&
    element.matches(MOBILE_SCROLL_CONTAINER_SELECTOR) &&
    !element.matches(EXCLUDED_SELECTOR) &&
    !optsOutOfMobileScrollFade(element)
  );
}

/** Whether a scrolled element styled elsewhere scrolls vertically, so it can join the manager on first scroll. */
function scrollsVertically(element: HTMLElement): boolean {
  if (element.matches(EXCLUDED_SELECTOR) || optsOutOfMobileScrollFade(element)) return false;
  const overflowY = getComputedStyle(element).overflowY;
  return overflowY === "auto" || overflowY === "scroll";
}

/**
 * Applies the scroll edge fade to every vertical scroll container under `root` on the mobile shell, where scrollbars
 * are hidden and the fade alone signals that more content lies above or below. Containers that attach the fade
 * themselves keep their own settings. Containers appear and leave with DOM changes, and a scroll container the selector
 * misses joins when it first scrolls. Returns a function that removes every fade the manager applied.
 */
export function installMobileScrollFades(root: HTMLElement = document.body): () => void {
  const managed = new Map<HTMLElement, { destroy(): void }>();
  let pending: Element[] = [];
  let removalsPending = false;
  let frame: number | undefined;

  const ownsFadeElsewhere = (element: HTMLElement) =>
    element.hasAttribute(SCROLL_FADE_OWNER_ATTRIBUTE) && !managed.has(element);

  const attach = (element: HTMLElement) => {
    if (managed.has(element) || ownsFadeElsewhere(element)) return;
    managed.set(element, scrollEdgeFadeAction(element));
  };

  const detach = (element: HTMLElement) => {
    managed.get(element)?.destroy();
    managed.delete(element);
  };

  const visit = (element: Element) => {
    if (isMobileScrollFadeCandidate(element)) attach(element);
    else if (element instanceof HTMLElement && managed.has(element)) detach(element);
    for (const descendant of element.querySelectorAll(MOBILE_SCROLL_CONTAINER_SELECTOR)) {
      if (isMobileScrollFadeCandidate(descendant)) attach(descendant);
    }
  };

  const flush = () => {
    frame = undefined;
    const elements = pending;
    pending = [];
    for (const element of elements) if (element.isConnected) visit(element);
    if (removalsPending) {
      removalsPending = false;
      for (const element of [...managed.keys()]) if (!element.isConnected) detach(element);
    }
  };

  const schedule = () => {
    frame ??= requestAnimationFrame(flush);
  };

  const mutations = new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === "attributes") {
        if (record.target instanceof Element) pending.push(record.target);
        continue;
      }
      for (const added of record.addedNodes) if (added instanceof Element) pending.push(added);
      if (record.removedNodes.length > 0) removalsPending = true;
    }
    schedule();
  });
  mutations.observe(root, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["class", MOBILE_SCROLL_FADE_ATTRIBUTE],
  });

  const handleScroll = (event: Event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement) || managed.has(target) || ownsFadeElsewhere(target)) return;
    if (!root.contains(target) || !scrollsVertically(target)) return;
    attach(target);
  };
  root.addEventListener("scroll", handleScroll, { capture: true, passive: true });

  visit(root);

  return () => {
    mutations.disconnect();
    root.removeEventListener("scroll", handleScroll, { capture: true });
    if (frame !== undefined) cancelAnimationFrame(frame);
    for (const element of [...managed.keys()]) detach(element);
  };
}
