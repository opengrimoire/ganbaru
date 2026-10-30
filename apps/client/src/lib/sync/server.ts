import type { SyncServerError, SyncServerView } from "$lib/api/sync";
import type { Translate } from "$lib/i18n/translator.svelte";
import { syncRelayProblemLabel } from "./status";

/** Hexadecimal characters per group when a certificate fingerprint is shown. */
const FINGERPRINT_GROUP = 4;

/** Splits a hexadecimal fingerprint into short groups so it wraps and reads aloud in pieces. */
export function groupSyncFingerprint(fingerprint: string): string {
  const groups: string[] = [];
  for (let index = 0; index < fingerprint.length; index += FINGERPRINT_GROUP) {
    groups.push(fingerprint.slice(index, index + FINGERPRINT_GROUP));
  }
  return groups.join(" ");
}

/**
 * Describes the vault's server: its addresses, certificate, and, for a relay, whether it answered
 * and speaks this build's protocol, with the localized reason when it did not.
 */
export function syncServerLines(t: Translate, server: SyncServerView): string[] {
  const lines: string[] = [];
  if (server.endpoints.length > 0) lines.push(t("sync.server.endpoints", server.endpoints.join(", ")));
  if (server.certificateFingerprint) {
    lines.push(t("sync.server.fingerprint", groupSyncFingerprint(server.certificateFingerprint)));
  }
  const probe = server.probe;
  if (probe === null) return lines;
  if (probe.protocolVersion === null) {
    lines.push(t("sync.server.unreachable"));
  } else if (probe.compatible) {
    lines.push(t("sync.server.reachable", probe.protocolVersion));
  } else {
    lines.push(t("sync.server.incompatible", probe.protocolVersion));
  }
  if (probe.problem !== null && probe.problem !== "incompatible") lines.push(syncRelayProblemLabel(t, probe.problem));
  return lines;
}

/** Explains why adding or removing the server failed, using the relay's problem when it refused. */
export function syncServerErrorLabel(t: Translate, error: SyncServerError): string {
  if (error.code === "relay") {
    return error.problem === null ? t("sync.server.errors.failed") : syncRelayProblemLabel(t, error.problem);
  }
  return t(`sync.server.errors.${error.code}`);
}
