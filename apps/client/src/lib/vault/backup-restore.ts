import type { Translate } from "$lib/i18n/translator.svelte";
import type {
  BackupInspection,
  BackupRefusal,
  BackupRestoreOutcome,
  BackupSaveOutcome,
} from "$lib/vault/state";

/** Native steps a backup restore runs, injected so the flow stays testable. */
export interface BackupRestoreSteps {
  /** Lets the user pick a backup file and inspects it. */
  inspect: () => Promise<BackupInspection>;
  /** Restores the inspected backup with a kit code, or with this device's keys for `null`. */
  restore: (token: string, kit: string | null) => Promise<BackupRestoreOutcome>;
  /**
   * Asks the user for the recovery kit code. `refusal` is why the previous try failed, or `null`
   * on the first request. Resolves `null` when the user gives up.
   */
  askKit: (refusal: BackupRefusal | null) => Promise<string | null>;
}

/** Refusals that another kit code can fix, so the flow asks again instead of stopping. */
const KIT_REFUSALS: ReadonlySet<BackupRefusal> = new Set(["kitRequired", "wrongKit", "invalidKit"]);

/**
 * Whether a refusal asks the user for a (different) recovery kit code.
 *
 * @param reason - Refusal from a restore.
 * @returns True for refusals a kit code can fix.
 */
export function refusalNeedsKit(reason: BackupRefusal): boolean {
  return KIT_REFUSALS.has(reason);
}

/**
 * Runs a backup restore: pick and inspect a file, ask for the kit when this device cannot open it,
 * and ask again after a wrong or mistyped kit until the restore ends or the user gives up.
 *
 * @param steps - Native steps and the kit prompt.
 * @returns How the restore ended.
 */
export async function runBackupRestore(steps: BackupRestoreSteps): Promise<BackupRestoreOutcome> {
  const inspection = await steps.inspect();
  if (inspection.kind !== "ready") return inspection;
  let kit: string | null = null;
  if (inspection.needsKit) {
    kit = await steps.askKit(null);
    if (kit === null) return { kind: "cancelled" };
  }
  for (;;) {
    const outcome = await steps.restore(inspection.token, kit);
    if (outcome.kind !== "refused" || !refusalNeedsKit(outcome.reason)) return outcome;
    kit = await steps.askKit(outcome.reason);
    if (kit === null) return { kind: "cancelled" };
  }
}

/**
 * The localized message of a refused backup or restore.
 *
 * @param reason - Refusal from the native command.
 * @param t - Translator.
 * @returns A sentence for the user.
 */
export function backupRefusalMessage(reason: BackupRefusal, t: Translate): string {
  switch (reason) {
    case "noRecoveryKit":
      return t("settings.data.backupRefusal.noRecoveryKit");
    case "notBackup":
      return t("settings.data.backupRefusal.notBackup");
    case "unsupportedVersion":
      return t("settings.data.backupRefusal.unsupportedVersion");
    case "damaged":
      return t("settings.data.backupRefusal.damaged");
    case "kitRequired":
      return t("settings.data.backupRefusal.kitRequired");
    case "wrongKit":
      return t("settings.data.backupRefusal.wrongKit");
    case "invalidKit":
      return t("settings.data.backupRefusal.invalidKit");
    case "linkedAndroid":
      return t("settings.data.backupRefusal.linkedAndroid");
    case "expired":
      return t("settings.data.backupRefusal.expired");
  }
}

/**
 * The error a refused restore of a replaced vault shows. A linked phone is asked to unlink,
 * since restoring on a computer does not apply to a copy kept on the phone.
 *
 * @param outcome - Restore outcome.
 * @param t - Translator.
 * @returns The message, or `null` when nothing was refused.
 */
export function replacedVaultRestoreError(
  outcome: BackupRestoreOutcome,
  t: Translate,
): string | null {
  if (outcome.kind !== "refused") return null;
  if (outcome.reason === "linkedAndroid") return t("dataFolderError.linkedAndroid");
  return backupRefusalMessage(outcome.reason, t);
}

/** A status line after a backup action, or `null` when there is nothing to report. */
export type BackupStatus = { kind: "success" | "error"; message: string } | null;

/**
 * The status line after writing a backup.
 *
 * @param outcome - Save outcome.
 * @param destination - Where the file went, which picks the success wording.
 * @param t - Translator.
 * @returns The status, or `null` after a cancelled dialog.
 */
export function backupSaveStatus(
  outcome: BackupSaveOutcome,
  destination: "file" | "downloads",
  t: Translate,
): BackupStatus {
  switch (outcome.kind) {
    case "cancelled":
      return null;
    case "refused":
      return { kind: "error", message: backupRefusalMessage(outcome.reason, t) };
    case "saved":
      return {
        kind: "success",
        message: destination === "downloads"
          ? t("settings.data.backupSaved", outcome.fileName)
          : t("settings.data.backupSavedFile", outcome.fileName),
      };
  }
}
