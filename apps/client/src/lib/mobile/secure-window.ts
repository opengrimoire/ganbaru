import { setSyncSecureWindow } from "$lib/api/sync";
import { BUILD_PLATFORM } from "$lib/platform";

/** Ends one hold on the secure window; resolves once the window state follows the holders. */
export type SecureWindowRelease = () => Promise<void>;

/** Sets whether the platform window is kept out of screenshots, recents, and screen capture. */
export type SecureWindowSetter = (secure: boolean) => Promise<void>;

/**
 * Creates an acquire function for the secure window. The window is secure while at least one
 * holder has not released. Every change runs through one queue that applies the state the
 * holder count asks for at that moment, so overlapping releases and acquires end in that state.
 * A refused change logs a warning and resolves: losing the shield must not keep the person from a
 * recovery kit. Where `supported` is false, acquiring changes nothing.
 */
export function createSecureWindow(
  setSecure: SecureWindowSetter,
  supported: boolean,
): () => Promise<SecureWindowRelease> {
  let holders = 0;
  let applied = false;
  let queue: Promise<void> = Promise.resolve();

  function follow(): Promise<void> {
    queue = queue.then(async () => {
      const wanted = holders > 0;
      if (wanted === applied) return;
      try {
        await setSecure(wanted);
        applied = wanted;
      } catch (cause: unknown) {
        console.warn(`the secure window could not be ${wanted ? "set" : "cleared"}`, cause);
      }
    });
    return queue;
  }

  return async () => {
    if (!supported) return () => Promise.resolve();
    holders += 1;
    await follow();
    let released = false;
    return () => {
      if (released) return queue;
      released = true;
      holders -= 1;
      return follow();
    };
  };
}

/** Holds the Android secure window while a surface shows or takes a recovery kit code. */
export const acquireSecureWindow = createSecureWindow(setSyncSecureWindow, BUILD_PLATFORM === "android");
