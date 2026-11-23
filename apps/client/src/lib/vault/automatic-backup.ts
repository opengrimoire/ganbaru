import { invoke } from "@tauri-apps/api/core";

/** Event Rust emits whenever the automatic backup status changes; it carries no payload. */
export const AUTOMATIC_BACKUP_CHANGED_EVENT = "vault-automatic-backup-changed";

/** How often automatic backups are written. */
export type AutomaticBackupInterval = "sixHours" | "daily" | "weekly";

/** Intervals in the order Settings offers them. */
export const AUTOMATIC_BACKUP_INTERVALS: readonly AutomaticBackupInterval[] = ["sixHours", "daily", "weekly"];

/** Why the last automatic backup attempt failed. */
export type AutomaticBackupFailureKind = "noRecoveryKit" | "folderUnavailable" | "failed";

const FAILURE_KINDS: readonly AutomaticBackupFailureKind[] = ["noRecoveryKit", "folderUnavailable", "failed"];

/** Why a change to automatic backups was refused. */
export type AutomaticBackupRefusal = "noRecoveryKit" | "insideVault";

const REFUSALS: readonly AutomaticBackupRefusal[] = ["noRecoveryKit", "insideVault"];

/** The automatic backup settings and state of the active vault on this device. */
export interface AutomaticBackupStatus {
  enabled: boolean;
  interval: AutomaticBackupInterval;
  /** Where backups go: a path or folder name, and whether the user picked it. */
  folder: { label: string; custom: boolean };
  /** RFC 3339 time of the last automatic backup, if any. */
  lastBackupAt: string | null;
  /** RFC 3339 time the next scheduled backup is due, `null` while automatic backups are off. */
  nextBackupAt: string | null;
  failure: { kind: AutomaticBackupFailureKind; at: string } | null;
  /** Whether a backup is being written now. */
  running: boolean;
}

/** What a change to automatic backups did. */
export type AutomaticBackupUpdate =
  | { kind: "saved"; status: AutomaticBackupStatus }
  | { kind: "refused"; reason: AutomaticBackupRefusal };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readMember<T extends string>(values: readonly T[], value: unknown, what: string): T {
  const member = values.find((candidate) => candidate === value);
  if (member === undefined) throw new Error(`automatic backup ${what} is unknown`);
  return member;
}

function readTime(value: unknown, what: string): string | null {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string" || Number.isNaN(Date.parse(value))) {
    throw new Error(`automatic backup ${what} is not a time`);
  }
  return value;
}

/**
 * Validates the native automatic backup status.
 *
 * @param value - Untrusted `vault_automatic_backup_*` status.
 * @returns The typed status.
 */
export function parseAutomaticBackupStatus(value: unknown): AutomaticBackupStatus {
  if (!isRecord(value) || !isRecord(value.folder)) throw new Error("automatic backup status is not an object");
  const { enabled, running, folder } = value;
  if (typeof enabled !== "boolean" || typeof running !== "boolean") {
    throw new Error("automatic backup status is incomplete");
  }
  if (typeof folder.label !== "string" || folder.label === "" || typeof folder.custom !== "boolean") {
    throw new Error("automatic backup folder is incomplete");
  }
  let failure: AutomaticBackupStatus["failure"] = null;
  if (value.failure !== null && value.failure !== undefined) {
    if (!isRecord(value.failure)) throw new Error("automatic backup failure is not an object");
    const at = readTime(value.failure.at, "failure time");
    if (at === null) throw new Error("automatic backup failure has no time");
    failure = { kind: readMember(FAILURE_KINDS, value.failure.kind, "failure"), at };
  }
  return {
    enabled,
    interval: readMember(AUTOMATIC_BACKUP_INTERVALS, value.interval, "interval"),
    folder: { label: folder.label, custom: folder.custom },
    lastBackupAt: readTime(value.lastBackupAt, "last backup time"),
    nextBackupAt: readTime(value.nextBackupAt, "next backup time"),
    failure,
    running,
  };
}

/**
 * Validates the native response of a change to automatic backups.
 *
 * @param value - Untrusted `vault_automatic_backup_configure` or `vault_automatic_backup_choose_folder` response.
 * @returns The typed update.
 */
export function parseAutomaticBackupUpdate(value: unknown): AutomaticBackupUpdate {
  if (!isRecord(value)) throw new Error("automatic backup update is not an object");
  switch (value.kind) {
    case "saved":
      return { kind: "saved", status: parseAutomaticBackupStatus(value.status) };
    case "refused":
      return { kind: "refused", reason: readMember(REFUSALS, value.reason, "refusal") };
    default:
      throw new Error("automatic backup update kind is unknown");
  }
}

/** Reads the automatic backup settings and state of the active vault on this device. */
export async function getAutomaticBackupStatus(): Promise<AutomaticBackupStatus> {
  return parseAutomaticBackupStatus(await invoke<unknown>("vault_automatic_backup_status"));
}

/**
 * Turns automatic backups on or off and sets how often they run; turning them on is refused while the vault has no
 * recovery kit.
 */
export async function configureAutomaticBackup(
  enabled: boolean,
  interval: AutomaticBackupInterval,
): Promise<AutomaticBackupUpdate> {
  return parseAutomaticBackupUpdate(await invoke<unknown>("vault_automatic_backup_configure", { enabled, interval }));
}

/** Lets the user pick the folder automatic backups go to; a folder inside the vault is refused. */
export async function chooseAutomaticBackupFolder(): Promise<AutomaticBackupUpdate> {
  return parseAutomaticBackupUpdate(await invoke<unknown>("vault_automatic_backup_choose_folder"));
}

/** Sends automatic backups to the default folder again. */
export async function resetAutomaticBackupFolder(): Promise<AutomaticBackupStatus> {
  return parseAutomaticBackupStatus(await invoke<unknown>("vault_automatic_backup_reset_folder"));
}

/** Writes an automatic backup now, whether or not one is due, and returns the status after it. */
export async function runAutomaticBackupNow(): Promise<AutomaticBackupStatus> {
  return parseAutomaticBackupStatus(await invoke<unknown>("vault_automatic_backup_run_now"));
}

/** The line under the folder row that says when backups ran and will run. */
export type AutomaticBackupSchedule =
  | { kind: "none" }
  | { kind: "running" }
  | { kind: "last"; last: Date }
  | { kind: "soon"; last: Date | null }
  | { kind: "next"; last: Date | null; next: Date };

/**
 * Picks what the schedule line shows: the running backup, or the last and next backups. A next backup already due
 * shows as soon, since the scheduler writes it within minutes.
 *
 * @param status - Current status.
 * @param now - Current time.
 * @returns What to show.
 */
export function automaticBackupSchedule(status: AutomaticBackupStatus, now: Date): AutomaticBackupSchedule {
  if (status.running) return { kind: "running" };
  const last = status.lastBackupAt === null ? null : new Date(status.lastBackupAt);
  if (status.nextBackupAt === null) return last === null ? { kind: "none" } : { kind: "last", last };
  const next = new Date(status.nextBackupAt);
  if (next.getTime() <= now.getTime()) return { kind: "soon", last };
  return { kind: "next", last, next };
}
