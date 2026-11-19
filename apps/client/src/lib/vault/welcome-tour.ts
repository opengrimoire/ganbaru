const WELCOME_TOUR_STORAGE_KEY_PREFIX = "ganbaru.welcome-tour.v1:";
const PENDING = "pending";

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem" | "removeItem">;

/**
 * Whether the feature tour still has to show for `vaultId`. Only a vault opened from the setup screens is marked, so
 * a vault that was already in use, or one reopened after an error, goes straight on.
 */
export function welcomeTourPending(storage: ReadableStorage | undefined, vaultId: string): boolean {
  try {
    return storage?.getItem(`${WELCOME_TOUR_STORAGE_KEY_PREFIX}${vaultId}`) === PENDING;
  } catch (error) {
    console.warn("Could not read whether the welcome tour is pending:", error);
    return false;
  }
}

/** Marks the feature tour as pending for `vaultId`, right after the setup screens open it. */
export function markWelcomeTourPending(storage: WritableStorage | undefined, vaultId: string): void {
  try {
    storage?.setItem(`${WELCOME_TOUR_STORAGE_KEY_PREFIX}${vaultId}`, PENDING);
  } catch (error) {
    console.warn("Could not remember the pending welcome tour; it will not resume after a restart:", error);
  }
}

/** Clears the pending feature tour for `vaultId` once its last screen is done. */
export function completeWelcomeTour(storage: WritableStorage | undefined, vaultId: string): void {
  try {
    storage?.removeItem(`${WELCOME_TOUR_STORAGE_KEY_PREFIX}${vaultId}`);
  } catch (error) {
    console.warn("Could not clear the pending welcome tour:", error);
  }
}

/**
 * Moves a pending feature tour from `fromVaultId` to `toVaultId` when linking replaced the vault the setup screens
 * opened with a linked or server copy, so the tour still shows, and resumes after a restart, for the vault now active.
 */
export function carryWelcomeTour(
  storage: (ReadableStorage & WritableStorage) | undefined,
  fromVaultId: string,
  toVaultId: string,
): void {
  if (fromVaultId === toVaultId || !welcomeTourPending(storage, fromVaultId)) return;
  markWelcomeTourPending(storage, toVaultId);
  completeWelcomeTour(storage, fromVaultId);
}
