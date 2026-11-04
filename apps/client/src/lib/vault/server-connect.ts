import type { Translate } from "$lib/i18n/translator.svelte";

/** The native refusal text of an anchor server that runs no vault yet. */
const NO_VAULT_TEXT = "runs no vault";

function causeText(cause: unknown): string {
  return typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "";
}

/**
 * Whether a refused join came from an anchor server that runs no vault yet, so this computer can
 * put its own vault on it instead.
 *
 * @param cause - Refusal from the join command.
 * @returns True when the server is empty.
 */
export function isServerWithoutVault(cause: unknown): boolean {
  return causeText(cause).includes(NO_VAULT_TEXT);
}

/**
 * The localized message of a refused connection to an anchor server, by joining or by recovering
 * with the kit. Native text is matched and never shown, so nothing the user typed can reach the
 * screen or a log through an error.
 *
 * @param cause - Refusal from the join or recovery command.
 * @param t - Translator.
 * @returns The message to show.
 */
export function serverConnectError(cause: unknown, t: Translate): string {
  const message = causeText(cause);
  if (message.includes("anchor setup code") || message.includes("refused this setup code")) {
    return t("vaultHandoff.serverConnect.errors.code");
  }
  if (message.includes("read the recovery kit")) return t("vaultHandoff.serverConnect.errors.kit");
  if (message.includes("rejected the recovery proposal")) {
    return t("vaultHandoff.serverConnect.errors.rejected");
  }
  if (message.includes("rejected the enrollment")) {
    return t("vaultHandoff.serverConnect.errors.joinRejected");
  }
  if (message.includes("holds this enrollment")) return t("vaultHandoff.serverConnect.errors.waiting");
  if (message.includes("does not enroll this device")) {
    return t("vaultHandoff.serverConnect.errors.notEnrolled");
  }
  if (message.includes(NO_VAULT_TEXT)) return t("vaultHandoff.serverConnect.errors.noVault");
  if (message.includes("already open")) return t("vaultHandoff.serverConnect.errors.alreadyOpen");
  if (message.includes("does not belong to the vault")) {
    return t("vaultHandoff.serverConnect.errors.wrongKit");
  }
  if (message.includes("could not reach the server")) {
    return t("vaultHandoff.serverConnect.errors.unreachable");
  }
  return t("vaultHandoff.serverConnect.errors.failed");
}
