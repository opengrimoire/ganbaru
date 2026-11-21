/** Text and tone of one confirmation the shared dialog shows. */
export interface ConfirmationRequest {
  /** Short heading; the message alone is shown as the heading when omitted. */
  title?: string;
  message: string;
  confirmLabel: string;
  /** Defaults to the localized Cancel label. */
  cancelLabel?: string;
  /** Marks the confirmed action as destructive. */
  danger?: boolean;
}

/** The confirmation a host currently shows. */
export interface ActiveConfirmation {
  readonly id: number;
  readonly request: ConfirmationRequest;
}

interface PendingConfirmation extends ActiveConfirmation {
  readonly resolve: (confirmed: boolean) => void;
}

let pending = $state<PendingConfirmation | null>(null);
let nextConfirmationId = 1;
let mountedHosts = 0;

/**
 * Asks the user to confirm an action in the shared confirmation dialog.
 *
 * A new request while one is open answers the earlier one with false, and dismissing the dialog answers false.
 * Without a mounted confirmation host the request answers false at once, so callers never wait forever.
 *
 * @param request What the dialog says.
 * @returns Whether the user chose the confirm action.
 */
export function requestConfirmation(request: ConfirmationRequest): Promise<boolean> {
  if (mountedHosts === 0) {
    console.warn("Confirmation requested without a mounted confirmation host", request.message);
    return Promise.resolve(false);
  }
  return new Promise((resolve) => {
    const previous = pending;
    pending = { id: nextConfirmationId++, request, resolve };
    previous?.resolve(false);
  });
}

/** Returns the confirmation the host should show, or null when none is open. */
export function activeConfirmation(): ActiveConfirmation | null {
  return pending;
}

/**
 * Answers the open confirmation. An answer for a request that is no longer open is ignored.
 *
 * @param id The id of the confirmation being answered.
 * @param confirmed Whether the user chose the confirm action.
 */
export function answerConfirmation(id: number, confirmed: boolean): void {
  const current = pending;
  if (!current || current.id !== id) return;
  pending = null;
  current.resolve(confirmed);
}

/**
 * Records that a confirmation host is mounted in this window.
 *
 * @returns Unregisters the host; when the last host goes, an open request answers false.
 */
export function registerConfirmationHost(): () => void {
  mountedHosts += 1;
  let registered = true;
  return () => {
    if (!registered) return;
    registered = false;
    mountedHosts -= 1;
    if (mountedHosts === 0 && pending) answerConfirmation(pending.id, false);
  };
}
