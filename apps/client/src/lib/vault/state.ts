import { invoke } from "@tauri-apps/api/core";
import { translate, type Translate } from "$lib/i18n/translator.svelte";
import { setActiveVaultIdentity } from "$lib/vault/active-vault";

export interface VaultAppState {
  activeVaultPath: string | null;
  recentVaultPaths: string[];
}

export interface VaultInfo {
  path: string;
  configPath: string;
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
  /** A linked replica that can write replicated tables, such as Quick notes, while the rest of the vault is read-only. */
  replicatedWrites: boolean;
  transferPhase: VaultTransferPhase;
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

function readNonNegativeInteger(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item.trim() !== "");
}

function parseVaultAppState(value: unknown): VaultAppState {
  if (!isRecord(value)) {
    return { activeVaultPath: null, recentVaultPaths: [] };
  }
  return {
    activeVaultPath: readString(value.activeVaultPath),
    recentVaultPaths: readStringArray(value.recentVaultPaths),
  };
}

function parseVaultInfo(value: unknown): VaultInfo {
  if (!isRecord(value)) throw new Error("vault info response is not an object");
  const path = readString(value.path);
  const configPath = readString(value.configPath);
  const databasePath = readString(value.databasePath);
  const vaultId = readString(value.vaultId);
  const displayName = readString(value.displayName);
  if (!path || !configPath || !databasePath || !vaultId || !displayName) {
    throw new Error("vault info response is incomplete");
  }
  return { path, configPath, databasePath, vaultId, displayName };
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
  const replicatedWrites = readBoolean(value.replicatedWrites);
  if (
    !vaultId
    || !deviceId
    || !ownerDeviceId
    || generation === null
    || (role !== "owner" && role !== "read-only" && role !== "recovery")
    || canWrite === null
    || replicatedWrites === null
  ) {
    throw new Error("vault ownership response is incomplete");
  }
  if (canWrite !== (role === "owner")) {
    throw new Error("vault ownership response has an inconsistent role");
  }
  if (replicatedWrites && role !== "read-only") {
    throw new Error("vault ownership response has inconsistent replicated writes");
  }
  return {
    vaultId,
    deviceId,
    ownerDeviceId,
    generation,
    role,
    canWrite,
    replicatedWrites,
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

  if (lower.includes("selected folder is not empty and is not a ganbaru ai folder")) {
    if (action === "default") {
      return t("dataFolderError.defaultNotValid");
    }
    return t("dataFolderError.folderNotEmpty");
  }

  if (lower.includes("read ganbaru ai folder marker")) {
    return t("dataFolderError.missingMarker");
  }

  if (lower.includes("parse ganbaru ai folder marker")) {
    return t("dataFolderError.damagedMarker");
  }

  if (lower.includes("unsupported ganbaru ai folder schema version")) {
    return t("dataFolderError.newerSchema");
  }

  if (lower.includes("selected folder is not a ganbaru ai folder")) {
    return t("dataFolderError.notGanbaruFolder");
  }

  if (
    containsAny(lower, [
      "failed to inspect ganbaru ai folder path",
      "canonicalize ganbaru ai folder path",
      "no such file",
      "not found",
    ])
  ) {
    return t("dataFolderError.notFound");
  }

  return fallbackForAction(action, t);
}

export async function readVaultAppState(): Promise<VaultAppState> {
  return parseVaultAppState(await invoke<unknown>("vault_read_app_state"));
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

/** Save a consistent, portable copy of the active Android vault to Downloads. */
export async function backupActiveVault(): Promise<VaultBackupOutcome> {
  return parseVaultBackupOutcome(await invoke<unknown>("vault_backup_to_downloads"));
}

/** Pick and transactionally restore a portable Android vault backup. */
export async function restoreVaultBackup(): Promise<VaultInfo | null> {
  const info = parseOptionalVaultInfo(await invoke<unknown>("vault_pick_and_restore_backup"));
  return info ? activateVaultInfo(info) : null;
}
