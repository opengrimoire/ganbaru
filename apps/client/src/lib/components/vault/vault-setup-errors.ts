import type { VaultErrorAction } from "$lib/vault/state";

/** A failure the vault setup screen reports, with the action that failed. */
export interface VaultSetupFailure {
  readonly raw: unknown;
  readonly action: VaultErrorAction;
}

/**
 * The failures the vault setup screen reports, one slot per source, so a failure from one source never hides the
 * other's.
 */
export interface VaultSetupErrors {
  /** Why the vault failed to open at startup, or why the latest attempt to choose a vault failed. */
  readonly attempt: VaultSetupFailure | null;
  /** Why the default vault location could not be loaded. */
  readonly location: VaultSetupFailure | null;
}

/** No failures. */
export const NO_VAULT_SETUP_ERRORS: VaultSetupErrors = { attempt: null, location: null };

/**
 * The errors with `failure` recorded in `slot`, replacing only that slot's earlier failure.
 *
 * @param errors - The current errors.
 * @param slot - Which source failed.
 * @param failure - The failure.
 */
export function recordVaultSetupFailure(
  errors: VaultSetupErrors,
  slot: keyof VaultSetupErrors,
  failure: VaultSetupFailure,
): VaultSetupErrors {
  return { ...errors, [slot]: failure };
}

/**
 * The text the setup screen shows for its errors: every recorded failure, the attempt first, or null when none is.
 *
 * @param errors - The recorded failures.
 * @param format - Turns one failure into its localized message.
 */
export function vaultSetupErrorText(
  errors: VaultSetupErrors,
  format: (failure: VaultSetupFailure) => string,
): string | null {
  const messages = [errors.attempt, errors.location].flatMap((failure) => (failure ? [format(failure)] : []));
  return messages.length > 0 ? messages.join(" ") : null;
}
