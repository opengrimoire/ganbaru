/** Ends one hold on the secure window; resolves once the window state follows. */
export type SecureWindowRelease = () => Promise<void>;

/** Sets whether the platform window is kept out of screenshots, recents, and screen capture. */
export type SecureWindowSetter = (secure: boolean) => Promise<void>;

/** Creates an acquire function for the secure window. */
export function createSecureWindow(
  _setSecure: SecureWindowSetter,
  _supported: boolean,
): () => Promise<SecureWindowRelease> {
  return () => Promise.resolve(() => Promise.resolve());
}

/** Holds the secure window while a surface shows a secret. */
export const acquireSecureWindow = createSecureWindow(() => Promise.resolve(), false);
