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

/** Converts native handoff failures into concise localized recovery guidance. */
export function formatHandoffError(cause: unknown, t: Translate): string {
  const raw = errorText(cause);
  const lower = raw.toLowerCase();
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
  const raw = errorText(cause);
  if (isConnectivityFailure(raw.toLowerCase())) {
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
