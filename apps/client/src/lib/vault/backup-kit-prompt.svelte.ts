import { runBackupRestore } from "$lib/vault/backup-restore";
import {
  inspectVaultBackup,
  restoreInspectedBackup,
  type BackupRefusal,
  type BackupRestoreOutcome,
} from "$lib/vault/state";

/** An open request for the recovery kit code. */
export interface BackupKitRequest {
  /** Why the previous code failed, or `null` on the first request. */
  refusal: BackupRefusal | null;
}

/** Reactive state of the recovery kit prompt shown during a backup restore. */
export interface BackupKitPrompt {
  /** The open request, or `null` while no prompt is shown. */
  readonly request: BackupKitRequest | null;
  /** Answers the open request with a code, or with `null` when the user gives up. */
  answer: (code: string | null) => void;
  /** Picks a backup and restores it, asking for the kit through this prompt when needed. */
  restore: () => Promise<BackupRestoreOutcome>;
}

/**
 * Creates the recovery kit prompt state a backup restore drives. Render `BackupKitPrompt` while
 * `request` is set and pass its answers to `answer`.
 *
 * @returns The prompt state.
 */
export function createBackupKitPrompt(): BackupKitPrompt {
  let request = $state<BackupKitRequest | null>(null);
  let resolveRequest: ((code: string | null) => void) | null = null;

  function ask(refusal: BackupRefusal | null): Promise<string | null> {
    resolveRequest?.(null);
    return new Promise((resolve) => {
      resolveRequest = resolve;
      request = { refusal };
    });
  }

  function answer(code: string | null): void {
    const resolve = resolveRequest;
    resolveRequest = null;
    request = null;
    resolve?.(code);
  }

  return {
    get request() {
      return request;
    },
    answer,
    restore: () =>
      runBackupRestore({
        inspect: inspectVaultBackup,
        restore: restoreInspectedBackup,
        askKit: ask,
      }),
  };
}
