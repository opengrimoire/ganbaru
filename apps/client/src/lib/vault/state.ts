import { invoke } from "@tauri-apps/api/core";
import { translate, type Translate } from "$lib/i18n/translator.svelte";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";

export interface VaultInfo {
  path: string;
  databasePath: string;
  vaultId: string;
  displayName: string;
}

let cachedActiveVaultInfo: VaultInfo | null | undefined;

export interface VaultDefaultLocation {
  path: string;
  parentPath: string;
  folderName: string;
  developmentBuild: boolean;
}

export interface VaultBackupOutcome {
  fileName: string;
  destination: "downloads";
}

export type VaultTransferPhase =
  | { kind: "stable" }
  | {
    kind: "preparingOutgoing";
    transferId: string;
    receiverDeviceId: string;
    nextGeneration: number;
  }
  | {
    kind: "outgoingCommitted";
    transferId: string;
    receiverDeviceId: string;
    committedGeneration: number;
  }
  | {
    kind: "incomingCommitted";
    transferId: string;
    sourceDeviceId: string;
    committedGeneration: number;
  }
  | { kind: "recoveryRequired"; transferId: string; reason: string };

export interface VaultOwnershipStatus {
  vaultId: string;
  deviceId: string;
  ownerDeviceId: string;
  generation: number;
  role: "owner" | "read-only" | "recovery";
  canWrite: boolean;
  /** A stable replica linked to the vault's owner, whose database opens guarded and syncs. */
  linkedReplica: boolean;
  /** Sync domains whose replicated tables this linked replica may write while the rest of the vault is read-only. Empty unless `linkedReplica`. */
  replicaWritableDomains: string[];
  transferPhase: VaultTransferPhase;
}

/**
 * How this app holds its active vault against other Ganbaru apps: `exclusive` when the vault lock is held, `unsupported` when the file system cannot lock so another app cannot be detected, and `notHeld` before any vault opens.
 */
export type VaultLockStatus = "notHeld" | "exclusive" | "unsupported";

const VAULT_LOCK_STATUSES: readonly VaultLockStatus[] = ["notHeld", "exclusive", "unsupported"];

/** Lowercase text of the native error raised when another Ganbaru app holds the vault lock. */
const VAULT_OPEN_ELSEWHERE_MESSAGE = "this vault is open in another ganbaru app";

/**
 * Validates the native vault lock status.
 *
 * @param value Untrusted command response.
 * @returns The lock status.
 * @throws When the response is not a known status.
 */
export function parseVaultLockStatus(value: unknown): VaultLockStatus {
  const status = VAULT_LOCK_STATUSES.find((candidate) => candidate === value);
  if (!status) throw new Error("vault lock status response is invalid");
  return status;
}

/** Sync domain name of Quick notes, as the vault manifest registers it. */
export const QUICK_NOTES_SYNC_DOMAIN = "quick_notes";

/** Sync domain name of Projects, as the vault manifest registers it. */
export const PROJECTS_SYNC_DOMAIN = "projects";

/**
 * Whether this device may write the replicated tables of a sync domain, as the owner or as a linked replica the domain lets write.
 *
 * @param status Ownership status of the active vault.
 * @param domain Sync domain name from the vault manifest.
 * @returns True when writes to the domain are accepted on this device.
 */
export function canWriteSyncDomain(status: VaultOwnershipStatus, domain: string): boolean {
  return status.canWrite || (status.linkedReplica && status.replicaWritableDomains.includes(domain));
}

export type VaultErrorAction =
  | "startup"
  | "default"
  | "change"
  | "import"
  | "backup"
  | "restore"
  | "general";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readString(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

function readBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function readStringList(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const items = value.map(readString);
  return items.every((item): item is string => item !== null) ? items : null;
}

function readNonNegativeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function parseVaultInfo(value: unknown): VaultInfo {
  if (!isRecord(value)) throw new Error("vault info response is not an object");
  const path = readString(value.path);
  const databasePath = readString(value.databasePath);
  const vaultId = readString(value.vaultId);
  const displayName = readString(value.displayName);
  if (!path || !databasePath || !vaultId || !displayName) {
    throw new Error("vault info response is incomplete");
  }
  return { path, databasePath, vaultId, displayName };
}

function parseVaultDefaultLocation(value: unknown): VaultDefaultLocation {
  if (!isRecord(value)) throw new Error("default folder response is not an object");
  const path = readString(value.path);
  const parentPath = readString(value.parentPath);
  const folderName = readString(value.folderName);
  const developmentBuild = readBoolean(value.developmentBuild);
  if (!path || !parentPath || !folderName || developmentBuild === null) {
    throw new Error("default folder response is incomplete");
  }
  return { path, parentPath, folderName, developmentBuild };
}

function parseOptionalVaultInfo(value: unknown): VaultInfo | null {
  return value === null ? null : parseVaultInfo(value);
}

function parseVaultBackupOutcome(value: unknown): VaultBackupOutcome {
  if (!isRecord(value)) throw new Error("backup response is not an object");
  const fileName = readString(value.fileName);
  if (!fileName || value.destination !== "downloads") {
    throw new Error("backup response is incomplete");
  }
  return { fileName, destination: "downloads" };
}

function parseTransferPhase(value: unknown): VaultTransferPhase {
  if (!isRecord(value) || typeof value.kind !== "string") {
    throw new Error("vault ownership transfer phase is invalid");
  }
  if (value.kind === "stable") return { kind: "stable" };
  const transferId = readString(value.transferId);
  if (!transferId) throw new Error("vault ownership transfer phase is incomplete");
  if (value.kind === "recoveryRequired") {
    const reason = readString(value.reason);
    if (!reason) throw new Error("vault ownership transfer phase is incomplete");
    return { kind: value.kind, transferId, reason };
  }
  const committedGeneration = readNonNegativeInteger(value.committedGeneration);
  if (value.kind === "incomingCommitted") {
    const sourceDeviceId = readString(value.sourceDeviceId);
    if (!sourceDeviceId || committedGeneration === null) {
      throw new Error("vault ownership transfer phase is incomplete");
    }
    return { kind: value.kind, transferId, sourceDeviceId, committedGeneration };
  }
  const receiverDeviceId = readString(value.receiverDeviceId);
  if (!receiverDeviceId) throw new Error("vault ownership transfer phase is incomplete");
  if (value.kind === "preparingOutgoing") {
    const nextGeneration = readNonNegativeInteger(value.nextGeneration);
    if (nextGeneration === null) {
      throw new Error("vault ownership transfer phase is incomplete");
    }
    return { kind: value.kind, transferId, receiverDeviceId, nextGeneration };
  }
  if (value.kind === "outgoingCommitted" && committedGeneration !== null) {
    return { kind: value.kind, transferId, receiverDeviceId, committedGeneration };
  }
  throw new Error("vault ownership transfer phase is invalid");
}

function parseVaultOwnershipStatus(value: unknown): VaultOwnershipStatus {
  if (!isRecord(value)) throw new Error("vault ownership response is not an object");
  const vaultId = readString(value.vaultId);
  const deviceId = readString(value.deviceId);
  const ownerDeviceId = readString(value.ownerDeviceId);
  const generation = readNonNegativeInteger(value.generation);
  const role = value.role;
  const canWrite = readBoolean(value.canWrite);
  const linkedReplica = readBoolean(value.linkedReplica);
  const replicaWritableDomains = readStringList(value.replicaWritableDomains);
  if (
    !vaultId
    || !deviceId
    || !ownerDeviceId
    || generation === null
    || (role !== "owner" && role !== "read-only" && role !== "recovery")
    || canWrite === null
    || linkedReplica === null
    || replicaWritableDomains === null
  ) {
    throw new Error("vault ownership response is incomplete");
  }
  if (canWrite !== (role === "owner")) {
    throw new Error("vault ownership response has an inconsistent role");
  }
  if (linkedReplica && role !== "read-only") {
    throw new Error("vault ownership response has an inconsistent linked replica");
  }
  if (!linkedReplica && replicaWritableDomains.length > 0) {
    throw new Error("vault ownership response has inconsistent replica writes");
  }
  return {
    vaultId,
    deviceId,
    ownerDeviceId,
    generation,
    role,
    canWrite,
    linkedReplica,
    replicaWritableDomains,
    transferPhase: parseTransferPhase(value.transferPhase),
  };
}

function activateVaultInfo(info: VaultInfo): VaultInfo;
function activateVaultInfo(info: VaultInfo | null): VaultInfo | null;
function activateVaultInfo(info: VaultInfo | null): VaultInfo | null {
  cachedActiveVaultInfo = info;
  setActiveVaultIdentity(info?.vaultId ?? null);
  return info;
}

function errorMessage(value: unknown, t: Translate): string {
  const message = value instanceof Error ? value.message : String(value);
  const trimmed = message.trim();
  return trimmed === "" ? t("dataFolderError.unknown") : trimmed;
}

function containsAny(value: string, needles: readonly string[]): boolean {
  return needles.some((needle) => value.includes(needle));
}

function fallbackForAction(
  action: VaultErrorAction,
  t: Translate,
): string {
  switch (action) {
    case "startup":
      return t("dataFolderError.startup");
    case "default":
      return t("dataFolderError.default");
    case "change":
      return t("dataFolderError.change");
    case "import":
      return t("dataFolderError.import");
    case "backup":
      return t("dataFolderError.backup");
    case "restore":
      return t("dataFolderError.restore");
    case "general":
      return t("dataFolderError.general");
  }
}

export function formatVaultError(
  error: unknown,
  action: VaultErrorAction = "general",
  t: Translate = translate,
): string {
  const raw = errorMessage(error, t);
  const lower = raw.toLowerCase();

  if (lower.includes(VAULT_OPEN_ELSEWHERE_MESSAGE)) {
    return t("dataFolderError.openElsewhere");
  }

  if (
    containsAny(lower, [
      "permission denied",
      "access is denied",
      "operation not permitted",
      "os error 13",
    ])
  ) {
    return t("dataFolderError.permission");
  }

  if (
    containsAny(lower, [
      "file is not a database",
      "database disk image is malformed",
      "run database migrations",
      "pragma foreign_keys",
      "pragma journal_mode",
      "pragma busy_timeout",
      "pragma synchronous",
      "pragma optimize",
    ])
  ) {
    return t("dataFolderError.database");
  }

  if (lower.includes("selected folder is not empty and is not a ganbaru folder")) {
    if (action === "default") {
      return t("dataFolderError.defaultNotValid");
    }
    return t("dataFolderError.folderNotEmpty");
  }

  if (lower.includes("read ganbaru folder marker")) {
    return t("dataFolderError.missingMarker");
  }

  if (lower.includes("parse ganbaru folder marker")) {
    return t("dataFolderError.damagedMarker");
  }

  if (lower.includes("unsupported ganbaru folder schema version")) {
    return t("dataFolderError.newerSchema");
  }

  if (lower.includes("selected folder is not a ganbaru folder")) {
    return t("dataFolderError.notGanbaruFolder");
  }

  if (
    containsAny(lower, [
      "failed to inspect ganbaru folder path",
      "canonicalize ganbaru folder path",
      "no such file",
      "not found",
    ])
  ) {
    return t("dataFolderError.notFound");
  }

  return fallbackForAction(action, t);
}

export async function getActiveVaultInfo(): Promise<VaultInfo | null> {
  return activateVaultInfo(parseOptionalVaultInfo(await invoke<unknown>("vault_active_info")));
}

/** Returns the active vault information most recently validated by the native runtime. */
export function getCachedActiveVaultInfo(): VaultInfo | null | undefined {
  return cachedActiveVaultInfo;
}

export async function getVaultOwnershipStatus(): Promise<VaultOwnershipStatus> {
  return parseVaultOwnershipStatus(await invoke<unknown>("vault_ownership_status"));
}

/**
 * Reads how this app holds its active vault against other Ganbaru apps.
 *
 * @returns The validated lock status.
 */
export async function getVaultLockStatus(): Promise<VaultLockStatus> {
  return parseVaultLockStatus(await invoke<unknown>("vault_lock_status"));
}

export async function getDefaultVaultLocation(): Promise<VaultDefaultLocation> {
  return parseVaultDefaultLocation(await invoke<unknown>("vault_default_location"));
}

export async function useDefaultVault(): Promise<VaultInfo> {
  return activateVaultInfo(parseVaultInfo(await invoke<unknown>("vault_use_default")));
}

export async function pickCreateVault(): Promise<VaultInfo | null> {
  const info = parseOptionalVaultInfo(await invoke<unknown>("vault_pick_create"));
  return info ? activateVaultInfo(info) : null;
}

export async function pickOpenVault(): Promise<VaultInfo | null> {
  const info = parseOptionalVaultInfo(await invoke<unknown>("vault_pick_open"));
  return info ? activateVaultInfo(info) : null;
}

export async function revealActiveVault(): Promise<void> {
  await invoke("vault_reveal_active");
}

/** Why a rollback was kept: a data reset or restore, or a handoff that replaced the vault. */
export type DatabaseRollbackOrigin = "reset" | "replacement";

/** A device-local copy of earlier vault data that can be restored. */
export interface DatabaseRollback {
  id: string;
  createdAt: string;
  sizeBytes: number;
  origin: DatabaseRollbackOrigin;
  /** The handoff that replaced this copy held drastically less data. */
  flagged: boolean;
  /** The user dismissed the flagged notice; the copy stays restorable. */
  dismissed: boolean;
}

function readRollbackOrigin(value: unknown): DatabaseRollbackOrigin | null {
  return value === "reset" || value === "replacement" ? value : null;
}

/**
 * Validates the native rollback listing.
 *
 * @param value - Untrusted `vault_database_rollbacks` response.
 * @returns Rollbacks newest first, as Rust ordered them.
 */
export function parseDatabaseRollbacks(value: unknown): DatabaseRollback[] {
  if (!Array.isArray(value)) throw new Error("database rollback response is not a list");
  return value.map((entry: unknown) => {
    if (!isRecord(entry)) throw new Error("database rollback entry is not an object");
    const id = readString(entry.id);
    const createdAt = readString(entry.createdAt);
    const sizeBytes = readNonNegativeInteger(entry.sizeBytes);
    const origin = readRollbackOrigin(entry.origin);
    const flagged = readBoolean(entry.flagged);
    const dismissed = readBoolean(entry.dismissed);
    if (
      !id ||
      !createdAt ||
      Number.isNaN(Date.parse(createdAt)) ||
      sizeBytes === null ||
      origin === null ||
      flagged === null ||
      dismissed === null
    ) {
      throw new Error("database rollback entry is incomplete");
    }
    return { id, createdAt, sizeBytes, origin, flagged, dismissed };
  });
}

/** Reports flagged rollbacks whose notice the user has not dismissed, newest first. */
export function pendingFlaggedRollbacks(rollbacks: readonly DatabaseRollback[]): DatabaseRollback[] {
  return rollbacks.filter((rollback) => rollback.flagged && !rollback.dismissed);
}

/** Hides the flagged notice of a rollback; the copy stays restorable until retention prunes it. */
export async function dismissDatabaseRollback(id: string): Promise<void> {
  await invoke("vault_dismiss_database_rollback", { id });
}

/** Lists device-local copies kept by data resets, restores, and handoff replacements. */
export async function listDatabaseRollbacks(): Promise<DatabaseRollback[]> {
  return parseDatabaseRollbacks(await invoke<unknown>("vault_database_rollbacks"));
}

/** Restores a kept database copy; the current database is kept as a new copy. Restart afterwards. */
export async function restoreDatabaseRollback(id: string): Promise<void> {
  await invoke("vault_restore_database_rollback", { id });
}

/** Save a consistent, portable copy of the active Android vault to Downloads. */
export async function backupActiveVault(): Promise<VaultBackupOutcome> {
  return parseVaultBackupOutcome(await invoke<unknown>("vault_backup_to_downloads"));
}

/** Pick and transactionally restore a portable Android vault backup. */
export async function restoreVaultBackup(): Promise<VaultInfo | null> {
  const info = parseOptionalVaultInfo(await invoke<unknown>("vault_pick_and_restore_backup"));
  return info ? activateVaultInfo(info) : null;
}
