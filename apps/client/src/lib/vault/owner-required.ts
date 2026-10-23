/** Prefix of the native error a command returns when only the vault owner may run it. */
export const OWNER_REQUIRED_ERROR_PREFIX = "owner_required:";

/** Receives requests to show the vault ownership prompt. */
export type VaultOwnershipPromptListener = () => void;

const listeners = new Set<VaultOwnershipPromptListener>();

/**
 * Whether a failure is the typed owner-required refusal, given as a native error string, an
 * `Error`, or a structured error whose `message` carries it (Chat command errors).
 *
 * @param error Failure raised by a command or a store guard.
 * @returns True when the failure asks for vault ownership.
 */
export function isOwnerRequiredError(error: unknown): boolean {
  return errorText(error)?.trimStart().startsWith(OWNER_REQUIRED_ERROR_PREFIX) ?? false;
}

/**
 * Builds the owner-required refusal a store raises before sending a write this device cannot make.
 *
 * @param reason Short English description of the refused write, for logs.
 * @returns An error that {@link isOwnerRequiredError} recognizes.
 */
export function ownerRequiredError(reason: string): Error {
  return new Error(`${OWNER_REQUIRED_ERROR_PREFIX} ${reason}`);
}

/** Asks the vault ownership prompt to show again, explaining that a change needs the main device. */
export function requestVaultOwnershipPrompt(): void {
  for (const listener of listeners) {
    try {
      listener();
    } catch (cause) {
      console.warn("Failed to show the vault ownership prompt:", cause);
    }
  }
}

/**
 * Subscribes to ownership prompt requests.
 *
 * @param listener Called on each request.
 * @returns Unsubscribes the listener.
 */
export function onVaultOwnershipPromptRequest(listener: VaultOwnershipPromptListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/**
 * Runs a command and, when it fails with the owner-required refusal, asks for the ownership
 * prompt before rethrowing so callers keep their own error handling.
 *
 * @param operation Command to run.
 * @returns The command's result.
 */
export async function withOwnerRequiredPrompt<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    if (isOwnerRequiredError(error)) requestVaultOwnershipPrompt();
    throw error;
  }
}

function errorText(error: unknown): string | null {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "message" in error) {
    const { message } = error;
    return typeof message === "string" ? message : null;
  }
  return null;
}
