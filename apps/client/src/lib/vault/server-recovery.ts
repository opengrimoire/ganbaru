import type { Translate } from "$lib/i18n/translator.svelte";

/**
 * The localized message of a refused recovery through an anchor server. Native text is matched
 * and never shown, so nothing the user typed can reach the screen or a log through an error.
 *
 * @param cause - Refusal from the recovery command.
 * @param t - Translator.
 * @returns The message to show.
 */
export function serverRecoveryError(cause: unknown, t: Translate): string {
  const message = typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "";
  if (message.includes("anchor setup code") || message.includes("refused this setup code")) {
    return t("vaultHandoff.serverRecovery.errors.code");
  }
  if (message.includes("read the recovery kit")) return t("vaultHandoff.serverRecovery.errors.kit");
  if (message.includes("rejected the recovery proposal")) {
    return t("vaultHandoff.serverRecovery.errors.rejected");
  }
  if (message.includes("runs no vault")) return t("vaultHandoff.serverRecovery.errors.noVault");
  if (message.includes("already open")) return t("vaultHandoff.serverRecovery.errors.alreadyOpen");
  return t("vaultHandoff.serverRecovery.errors.failed");
}
