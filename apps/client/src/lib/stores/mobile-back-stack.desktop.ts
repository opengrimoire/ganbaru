import type { MobileBackStack } from "./mobile-back-stack-contracts";

const noop = (): void => undefined;

const desktopMobileBackStack: MobileBackStack = {
  get hasActiveLayer(): boolean {
    return false;
  },

  activate(): () => void {
    return noop;
  },

  consume(): boolean {
    return false;
  },
};

/** Return the allocation-free Android Back adapter used by desktop and iOS builds. */
export function getMobileBackStack(): MobileBackStack {
  return desktopMobileBackStack;
}
