import { invoke } from "@tauri-apps/api/core";

export interface PairingQrMatrix {
  width: number;
  modules: boolean[];
}

export interface PairingInvitation {
  invitation: string;
  qr: PairingQrMatrix;
  endpoint: string;
  expiresAtMs: number;
  networkAccess?: DesktopNetworkAccess;
}

export type DesktopNetworkAccessState =
  | "notRequired"
  | "authorizationRequired"
  | "granted"
  | "manualActionRequired";

export interface DesktopNetworkAccess {
  state: DesktopNetworkAccessState;
}

export interface PairingStatus {
  deviceId: string;
  linked: boolean;
  devices: LinkedDevice[];
  peerDeviceId: string | null;
  peerLabel: string | null;
  coordinatorEndpoint: string | null;
  vaultId: string | null;
  canWrite: boolean | null;
  recoveryRequired: boolean;
  revokedByCoordinator: boolean;
  replicaReady: boolean;
  pendingTransfer: boolean;
  canInvite: boolean;
  untouchedStarterVault: boolean;
  /** A device waiting for this coordinator to approve it; absent where linking cannot be hosted. */
  pendingEnrollment?: PendingEnrollment | null;
  networkAccess?: DesktopNetworkAccess;
}

/** An enrolling device, shown with the code it also shows so the user can compare them. */
export interface PendingEnrollment {
  requestId: string;
  deviceLabel: string;
  deviceKind: LinkedDevice["kind"];
  code: string;
}

export interface LinkedDevice {
  deviceId: string;
  label: string | null;
  isOwner: boolean;
  isCoordinator: boolean;
  kind: "computer" | "phone" | "unknown";
}

let cachedPairingStatus: PairingStatus | undefined;

export type DesktopBundleReceiveMode = "ownership" | "refresh" | "bootstrap";
export type OwnerBundlePurpose = Exclude<DesktopBundleReceiveMode, "bootstrap">;

export interface DesktopBundleReceiveOutcome {
  transferId: string | null;
  generation: number | null;
  activated: boolean;
  inProgress: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Validates a native QR module grid; pairing invitations and contact cards share the shape. */
export function parseQrMatrix(value: unknown): PairingQrMatrix {
  if (!isRecord(value) || !Number.isSafeInteger(value.width) || !Array.isArray(value.modules)) {
    throw new Error("Invalid pairing QR matrix response");
  }
  const width = value.width as number;
  if (
    width <= 0 ||
    width > 177 ||
    value.modules.length !== width * width ||
    !value.modules.every((module) => typeof module === "boolean")
  ) {
    throw new Error("Invalid pairing QR matrix response");
  }
  return { width, modules: value.modules as boolean[] };
}

function nullableString(value: unknown, label: string): string | null {
  if (value === null || typeof value === "string") return value;
  throw new Error(`Invalid ${label} response`);
}

function parseInvitation(value: unknown): PairingInvitation {
  if (
    !isRecord(value) ||
    typeof value.invitation !== "string" ||
    typeof value.endpoint !== "string" ||
    typeof value.expiresAtMs !== "number"
  ) {
    throw new Error("Invalid pairing invitation response");
  }
  return {
    invitation: value.invitation,
    qr: parseQrMatrix(value.qr),
    endpoint: value.endpoint,
    expiresAtMs: value.expiresAtMs,
    networkAccess: value.networkAccess === undefined
      ? undefined
      : parseDesktopNetworkAccess(value.networkAccess),
  };
}

function parseDesktopNetworkAccess(value: unknown): DesktopNetworkAccess {
  if (
    !isRecord(value)
    || ![
      "notRequired",
      "authorizationRequired",
      "granted",
      "manualActionRequired",
    ].includes(value.state as string)
  ) {
    throw new Error("Invalid desktop network access response");
  }
  return { state: value.state as DesktopNetworkAccessState };
}

const DEVICE_KINDS: readonly LinkedDevice["kind"][] = ["computer", "phone", "unknown"];

function isDeviceKind(value: unknown): value is LinkedDevice["kind"] {
  return DEVICE_KINDS.includes(value as LinkedDevice["kind"]);
}

function parsePendingEnrollment(value: unknown): PendingEnrollment | null {
  if (value === null) return null;
  if (
    !isRecord(value)
    || typeof value.requestId !== "string"
    || typeof value.deviceLabel !== "string"
    || !isDeviceKind(value.deviceKind)
    || typeof value.code !== "string"
  ) {
    throw new Error("Invalid pending enrollment response");
  }
  return {
    requestId: value.requestId,
    deviceLabel: value.deviceLabel,
    deviceKind: value.deviceKind,
    code: value.code,
  };
}

function parseStatus(value: unknown): PairingStatus {
  if (
    !isRecord(value) ||
    typeof value.deviceId !== "string" ||
    typeof value.linked !== "boolean" ||
    !Array.isArray(value.devices) ||
    (value.canWrite !== null && typeof value.canWrite !== "boolean") ||
    typeof value.recoveryRequired !== "boolean" ||
    typeof value.revokedByCoordinator !== "boolean" ||
    typeof value.replicaReady !== "boolean" ||
    typeof value.pendingTransfer !== "boolean"
    || typeof value.canInvite !== "boolean"
    || typeof value.untouchedStarterVault !== "boolean"
  ) {
    throw new Error("Invalid pairing status response");
  }
  const devices = value.devices.map((device, index) => {
    if (
      !isRecord(device)
      || typeof device.deviceId !== "string"
      || (device.label !== null && typeof device.label !== "string")
      || typeof device.isOwner !== "boolean"
      || typeof device.isCoordinator !== "boolean"
      || !isDeviceKind(device.kind)
    ) {
      throw new Error(`Invalid linked device ${index + 1} response`);
    }
    return {
      deviceId: device.deviceId,
      label: device.label as string | null,
      isOwner: device.isOwner,
      isCoordinator: device.isCoordinator,
      kind: device.kind,
    };
  });
  return {
    deviceId: value.deviceId,
    linked: value.linked,
    devices,
    peerDeviceId: nullableString(value.peerDeviceId, "peer device id"),
    peerLabel: nullableString(value.peerLabel, "peer label"),
    coordinatorEndpoint: nullableString(value.coordinatorEndpoint, "coordinator endpoint"),
    vaultId: nullableString(value.vaultId, "vault id"),
    canWrite: value.canWrite as boolean | null,
    recoveryRequired: value.recoveryRequired,
    revokedByCoordinator: value.revokedByCoordinator,
    replicaReady: value.replicaReady,
    pendingTransfer: value.pendingTransfer,
    canInvite: value.canInvite,
    untouchedStarterVault: value.untouchedStarterVault,
    pendingEnrollment: value.pendingEnrollment === undefined
      ? undefined
      : parsePendingEnrollment(value.pendingEnrollment),
    networkAccess: value.networkAccess === undefined
      ? undefined
      : parseDesktopNetworkAccess(value.networkAccess),
  };
}

function parseReceiveOutcome(value: unknown): DesktopBundleReceiveOutcome {
  if (
    !isRecord(value) ||
    typeof value.activated !== "boolean" ||
    typeof value.inProgress !== "boolean" ||
    (value.transferId !== null && typeof value.transferId !== "string") ||
    (value.generation !== null && !Number.isSafeInteger(value.generation))
  ) {
    throw new Error("Invalid desktop bundle receive response");
  }
  return {
    transferId: value.transferId as string | null,
    generation: value.generation as number | null,
    activated: value.activated,
    inProgress: value.inProgress,
  };
}

/** Creates a short-lived single-use invitation on the desktop coordinator. */
export async function createPairingInvitation(): Promise<PairingInvitation> {
  return parseInvitation(await invoke<unknown>("handoff_create_pairing_invitation"));
}

/** Requests narrowly scoped Linux firewall access for phone linking. */
export async function grantDesktopNetworkAccess(): Promise<DesktopNetworkAccess> {
  return parseDesktopNetworkAccess(
    await invoke<unknown>("handoff_grant_network_access"),
  );
}

/** Removes Linux firewall rules previously created by Ganbaru. */
export async function revokeDesktopNetworkAccess(): Promise<DesktopNetworkAccess> {
  return parseDesktopNetworkAccess(
    await invoke<unknown>("handoff_revoke_network_access"),
  );
}

/** Decodes a bounded grayscale camera frame into an authenticated invitation. */
export async function decodePairingQr(
  width: number,
  height: number,
  luma: Uint8Array,
): Promise<string> {
  const value = await invoke<unknown>("handoff_decode_pairing_qr", {
    width,
    height,
    luma: Array.from(luma),
  });
  if (typeof value !== "string") throw new Error("Invalid pairing QR response");
  return value;
}

/** Enrolls this device using the coordinator identity pinned by the QR code. */
export function enrollWithDesktop(
  invitation: string,
  deviceLabel: string,
): Promise<void> {
  return invoke("handoff_enroll", { invitation, deviceLabel });
}

/** Returns the code this device shows while it enrolls; the coordinator shows the same code. */
export async function readEnrollmentCode(invitation: string): Promise<string> {
  const value = await invoke<unknown>("handoff_enrollment_code", { invitation });
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Invalid enrollment code response");
  }
  return value;
}

/** Approves or declines the device waiting on this coordinator. */
export function respondToEnrollment(requestId: string, approve: boolean): Promise<void> {
  return invoke("handoff_respond_enrollment", { requestId, approve });
}

/** Returns a stable human-readable label for this device when it enrolls. */
export async function readSuggestedDeviceLabel(): Promise<string> {
  const value = await invoke<unknown>("handoff_suggested_device_label");
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new Error("Invalid device label response");
  }
  return value;
}

/** Reads device-local linked coordinator status. */
export async function readPairingStatus(): Promise<PairingStatus> {
  const status = parseStatus(await invoke<unknown>("handoff_pairing_status"));
  cachedPairingStatus = status;
  return status;
}

/** Returns the most recently validated linked-device status without native work. */
export function getCachedPairingStatus(): PairingStatus | undefined {
  return cachedPairingStatus;
}

/** Receives and activates the desktop vault through the shared whole-vault path. */
export async function receiveDesktopBundle(
  mode: DesktopBundleReceiveMode,
): Promise<DesktopBundleReceiveOutcome> {
  return parseReceiveOutcome(
    await invoke<unknown>("handoff_receive_desktop_bundle", { mode }),
  );
}

/** Records that the user entered the app with the starter vault, so linking keeps a backup of it. */
export function releaseStarterVault(): Promise<void> {
  return invoke("vault_release_starter");
}

/** Cancels the currently streaming desktop bundle, leaving resumable staging. */
export function cancelDesktopBundleReceive(): Promise<void> {
  return invoke("handoff_cancel_receive");
}

/** Requests the current linked owner to upload a whole-vault bundle. */
export function requestOwnerBundle(
  purpose: OwnerBundlePurpose,
): Promise<void> {
  return invoke("handoff_request_owner_bundle", { purpose });
}

/** Removes the linked-device relationship without changing the current owner. */
export function unlinkVaultDevice(deviceId: string): Promise<void> {
  return invoke("handoff_unlink", { deviceId });
}

/** Explicitly forks the last local copy after the owning device is permanently unavailable. */
export async function recoverLocalVaultCopy(): Promise<number> {
  const generation = await invoke<unknown>("handoff_recover_local_copy");
  if (!Number.isSafeInteger(generation) || (generation as number) < 0) {
    throw new Error("Invalid vault recovery generation response");
  }
  return generation as number;
}
