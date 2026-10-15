import type { Action } from "svelte/action";
import { getMobileBackStack } from "$lib/stores/mobile-back-stack.svelte";
import { TouchHoldArbiter } from "$lib/utils/touch-hold";

const INTERACTIVE_SELECTOR = "button, a, input, textarea, select, [contenteditable], [role='button']";
const KEEP_OPEN_SELECTOR = ".message-action-controls, [role='dialog'], [role='menu'], [role='listbox'], .surface-floating, .surface-dialog";

let heldKey = $state<string | null>(null);
let heldNode: HTMLElement | null = null;
let releaseDismissal: (() => void) | null = null;

/** Return the key of the message whose actions a touch hold revealed, or null when none is held. */
export function heldMessageActionsKey(): string | null {
  return heldKey;
}

/** Hide the actions revealed by a touch hold. */
export function dismissHeldMessageActions(): void {
  releaseDismissal?.();
  releaseDismissal = null;
  heldNode = null;
  heldKey = null;
}

/**
 * Reveal a message's action toolbar when a finger holds the message still. Touch has no hover, so
 * the toolbar stays hidden until a hold, and a tap outside the toolbar, a scroll of the message
 * list, or Android Back hides it again. Holds that start on a control inside the message leave that
 * control in charge.
 */
export const messageTouchActions: Action<HTMLElement, string | null> = (node, initialKey) => {
  let key = initialKey;
  const arbiter = new TouchHoldArbiter();

  function handlePointerDown(event: PointerEvent): void {
    if (key === null || event.pointerType !== "touch" || !event.isPrimary) return;
    const heldKeyAtStart = key;
    if (event.target instanceof Element && event.target.closest(INTERACTIVE_SELECTOR)) return;
    arbiter.begin(event, () => {
      window.getSelection()?.removeAllRanges();
      hold(node, heldKeyAtStart);
      window.addEventListener("pointerup", finishHold, { once: true });
      window.addEventListener("pointercancel", finishHold, { once: true });
    });
  }

  function finishHold(): void {
    window.removeEventListener("pointerup", finishHold);
    window.removeEventListener("pointercancel", finishHold);
    arbiter.finish();
  }

  node.addEventListener("pointerdown", handlePointerDown);
  return {
    update(nextKey: string | null) {
      if (heldNode === node && heldKey !== nextKey) dismissHeldMessageActions();
      key = nextKey;
    },
    destroy() {
      node.removeEventListener("pointerdown", handlePointerDown);
      finishHold();
      if (heldNode === node) dismissHeldMessageActions();
    },
  };
};

function hold(node: HTMLElement, key: string): void {
  dismissHeldMessageActions();
  heldNode = node;
  heldKey = key;
  releaseDismissal = activateDismissal(node);
}

function activateDismissal(node: HTMLElement): () => void {
  const handlePointerDown = (event: PointerEvent): void => {
    if (event.target instanceof Element && event.target.closest(KEEP_OPEN_SELECTOR)) return;
    dismissHeldMessageActions();
  };
  const handleScroll = (event: Event): void => {
    if (event.target instanceof Node && event.target.contains(node)) dismissHeldMessageActions();
  };
  const releaseBack = getMobileBackStack().activate({ handle: dismissHeldMessageActions });
  window.addEventListener("pointerdown", handlePointerDown, { capture: true });
  document.addEventListener("scroll", handleScroll, { capture: true, passive: true });
  return () => {
    window.removeEventListener("pointerdown", handlePointerDown, { capture: true });
    document.removeEventListener("scroll", handleScroll, { capture: true });
    releaseBack();
  };
}
