import type { Translate } from "$lib/i18n/translator.svelte";

interface LinkedDeviceIdentity {
  deviceId: string;
}

function errorText(cause: unknown): string {
  if (cause instanceof Error) return cause.message;
  return typeof cause === "string" ? cause : String(cause);
}

function isConnectivityFailure(message: string): boolean {
  return message.includes("connect")
    || message.includes("unavailable")
    || message.includes("unreachable")
    || message.includes("timed out")
    || message.includes("network");
}

/** Whether startup disabled linking because saved pairing state could not be loaded. */
function isLinkingDisabled(message: string): boolean {
  return message.includes("vault linking is disabled");
}

/** Whether the coordinator refused the connection because its connection budget is full. */
function isCoordinatorBusy(message: string): boolean {
  return message.includes("handling too many connections");
}

/** Guidance when this device's linking key is missing from or unreadable in the system keyring. */
function deviceKeyMessage(
  message: string,
): "vaultHandoff.deviceKeyMissing" | "vaultHandoff.deviceKeyUnavailable" | null {
  if (message.includes("linking key is missing")) return "vaultHandoff.deviceKeyMissing";
  if (message.includes("linking key could not be read")) {
    return "vaultHandoff.deviceKeyUnavailable";
  }
  return null;
}

/** Guidance for an enrollment the coordinator user declined, missed, or could not answer. */
function enrollmentApprovalMessage(
  message: string,
):
  | "vaultHandoff.enrollmentDeclined"
  | "vaultHandoff.enrollmentUnanswered"
  | "vaultHandoff.enrollmentBusy"
  | "vaultHandoff.enrollmentReplaced"
  | "vaultHandoff.enrollmentGone"
  | null {
  if (message.includes("the coordinator declined this device")) {
    return "vaultHandoff.enrollmentDeclined";
  }
  if (
    message.includes("did not approve this device in time")
    || message.includes("did not answer the enrollment")
  ) {
    return "vaultHandoff.enrollmentUnanswered";
  }
  if (message.includes("waiting for approval on the coordinator")) {
    return "vaultHandoff.enrollmentBusy";
  }
  if (message.includes("pairing code on the coordinator was replaced")) {
    return "vaultHandoff.enrollmentReplaced";
  }
  if (message.includes("no longer waiting for approval")) {
    return "vaultHandoff.enrollmentGone";
  }
  return null;
}

/** Guidance when copying the linked vault to this device is refused or cannot keep the replaced vault. */
function bootstrapRefusalMessage(
  message: string,
):
  | "vaultHandoff.replacedVaultBackupFailed"
  | "vaultHandoff.linkFolderNotEmpty"
  | "vaultHandoff.firstLinkNeedsCopy"
  | "vaultHandoff.ownerHoldsVault"
  | null {
  if (message.includes("vault this link replaces could not be backed up")) {
    return "vaultHandoff.replacedVaultBackupFailed";
  }
  if (message.includes("folder chosen for the linked vault is not empty")) {
    return "vaultHandoff.linkFolderNotEmpty";
  }
  if (message.includes("linked vault must be copied to this device")) {
    return "vaultHandoff.firstLinkNeedsCopy";
  }
  if (message.includes("this device owns the linked vault")) {
    return "vaultHandoff.ownerHoldsVault";
  }
  return null;
}

/** Converts native handoff failures into concise localized recovery guidance. */
export function formatHandoffError(cause: unknown, t: Translate): string {
  const raw = errorText(cause);
  const lower = raw.toLowerCase();
  if (isLinkingDisabled(lower)) {
    return t("vaultHandoff.linkingDisabled");
  }
  const keyMessage = deviceKeyMessage(lower);
  if (keyMessage) return t(keyMessage);
  const enrollmentMessage = enrollmentApprovalMessage(lower);
  if (enrollmentMessage) return t(enrollmentMessage);
  const bootstrapMessage = bootstrapRefusalMessage(lower);
  if (bootstrapMessage) return t(bootstrapMessage);
  if (lower.includes("chat terminals")) {
    return t("vaultHandoff.blockerTerminal");
  }
  if (lower.includes("chat") && (lower.includes("active") || lower.includes("running"))) {
    return t("vaultHandoff.blockerChat");
  }
  if (lower.includes("pomodoro") || lower.includes("focus session")) {
    return t("vaultHandoff.blockerFocus");
  }
  if (lower.includes("transfer") && (lower.includes("finish") || lower.includes("active"))) {
    return t("vaultHandoff.blockerTransfer");
  }
  if (
    lower.includes("handoff compatibility mismatch") ||
    lower.includes("handoff protocol version is unsupported")
  ) {
    return t("vaultHandoff.incompatibleVersions");
  }
  if (lower.includes("already linked to another coordinator")) {
    return t("vaultHandoff.differentCoordinator");
  }
  if (lower.includes("linked vault is not active on this device")) {
    return t("vaultHandoff.linkedVaultInactive");
  }
  if (lower.includes("vault ownership changed too often")) {
    return t("vaultHandoff.ownershipChurnLimit");
  }
  if (lower.includes("requested the vault too often")) {
    return t("vaultHandoff.bundleRequestLimit");
  }
  if (isCoordinatorBusy(lower)) {
    return t("vaultHandoff.coordinatorBusy");
  }
  if (lower.includes("vault is not active on the coordinator")) {
    return t("vaultHandoff.coordinatorVaultInactive");
  }
  if (isConnectivityFailure(lower)) {
    return t("vaultHandoff.unreachable");
  }
  return t("vaultHandoff.failed", raw || t("vaultHandoff.unknownError"));
}

/** Explain invalid or expired pairing codes without exposing parser errors. */
export function formatPairingCodeError(cause: unknown, t: Translate): string {
  const lower = errorText(cause).toLowerCase();
  if (lower.includes("pairing invitation has expired")) {
    return t("vaultHandoff.codeExpired");
  }
  if (lower.includes("pairing invitation")) {
    return t("vaultHandoff.codeInvalid");
  }
  return formatHandoffError(cause, t);
}

/** Converts ownership connectivity failures into main-device guidance. */
export function formatOwnershipHandoffError(cause: unknown, t: Translate): string {
  const lower = errorText(cause).toLowerCase();
  if (
    !isLinkingDisabled(lower)
    && !isCoordinatorBusy(lower)
    && !deviceKeyMessage(lower)
    && isConnectivityFailure(lower)
  ) {
    return t("vaultHandoff.ownerUnreachable");
  }
  return formatHandoffError(cause, t);
}

/** Reports whether an invitation enrolled a device absent when it was created. */
export function hasNewLinkedDevice(
  deviceIdsAtInvitation: ReadonlySet<string>,
  devices: readonly LinkedDeviceIdentity[],
): boolean {
  return devices.some((device) => !deviceIdsAtInvitation.has(device.deviceId));
}
