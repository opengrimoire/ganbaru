import { getContext, setContext } from "svelte";

const MOBILE_IDENTITY_SLOT_KEY = Symbol("mobile-identity-slot");

/** The top bar element where the active phone destination shows its context switcher. */
export interface MobileIdentitySlot {
  readonly target: HTMLElement | null;
}

/** Shares the top bar's context switcher slot with the destination rendered below it. */
export function provideMobileIdentitySlot(slot: MobileIdentitySlot): void {
  setContext(MOBILE_IDENTITY_SLOT_KEY, slot);
}

/** Returns the shell's context switcher slot, or null outside the mobile shell. */
export function getMobileIdentitySlot(): MobileIdentitySlot | null {
  return getContext<MobileIdentitySlot | undefined>(MOBILE_IDENTITY_SLOT_KEY) ?? null;
}

/**
 * Svelte action that shows the host element inside the top bar identity slot while one exists and
 * keeps it in its original place otherwise, so the switcher still renders outside the mobile shell.
 */
export function mobileIdentityPortal(node: HTMLElement, target: HTMLElement | null) {
  const anchor = document.createComment("mobile-identity");
  node.before(anchor);
  function place(next: HTMLElement | null): void {
    if (next) next.appendChild(node);
    else anchor.after(node);
  }
  place(target);
  return {
    update(next: HTMLElement | null) {
      place(next);
    },
    destroy() {
      node.remove();
      anchor.remove();
    },
  };
}
