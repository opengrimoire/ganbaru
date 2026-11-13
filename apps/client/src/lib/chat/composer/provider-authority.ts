import type {
  ChatAssignmentTargetRead,
  ProviderAuthoritySupport,
  SafetyMode,
} from "$lib/chat/contracts";

/** Localized blocker messages a provider's authority support can raise for one target. */
export type ProviderTargetAuthorityBlocker =
  | "chat.organization.providerCannotReadRoot"
  | "chat.organization.providerCannotWriteRoot"
  | "chat.organization.providerCannotDenyShell"
  | "chat.organization.providerCannotConfineCommands"
  | "chat.organization.providerCommandsNeedAsk"
  | "chat.organization.providerCannotEnforceNetwork"
  | "chat.organization.providerCannotPublish";

/**
 * Mirrors the send gate's provider enforcement check for one assignment target.
 *
 * Commands are confined when the provider confines them itself, or when it sends every
 * command to the person and the target's effective approval is Ask under a non-custom
 * teammate policy. Publish stays refused until the provider classifies publication.
 *
 * @param authority - Authority support from the teammate provider's last probe.
 * @param target - The selected assignment target.
 * @param teammateSafetyMode - The teammate policy's safety mode, or null when unknown.
 * @returns The blockers in display order; empty when the provider enforces the target.
 */
export function providerTargetAuthorityBlockers(
  authority: ProviderAuthoritySupport,
  target: Pick<ChatAssignmentTargetRead, "kind" | "folderCapability" | "effectiveRuntimeApproval">,
  teammateSafetyMode: SafetyMode | null,
): ProviderTargetAuthorityBlocker[] {
  const blockers: ProviderTargetAuthorityBlocker[] = [];
  const capability = target.kind === "scratch" ? null : target.folderCapability;
  const runsCommands = target.kind === "scratch"
    || capability === "execute"
    || capability === "publish";
  if (capability === "read" && !authority.readOnlyRoot) {
    blockers.push("chat.organization.providerCannotReadRoot");
  }
  if ((runsCommands || capability === "edit") && !authority.writableRoot) {
    blockers.push("chat.organization.providerCannotWriteRoot");
  }
  if ((capability === "read" || capability === "edit") && !authority.denyShell) {
    blockers.push("chat.organization.providerCannotDenyShell");
  }
  if (runsCommands && !authority.confinedCommands) {
    const asksForEachCommand = target.effectiveRuntimeApproval === "ask"
      && teammateSafetyMode !== null
      && teammateSafetyMode !== "custom";
    if (!authority.personApprovedCommands) {
      blockers.push("chat.organization.providerCannotConfineCommands");
    } else if (!asksForEachCommand) {
      blockers.push("chat.organization.providerCommandsNeedAsk");
    }
  }
  const hasNativeScope = target.kind === "scratch" || (capability !== null && capability !== "none");
  if (hasNativeScope && !authority.networkBoundary) {
    blockers.push("chat.organization.providerCannotEnforceNetwork");
  }
  if (capability === "publish" && !authority.classifiedPublish) {
    blockers.push("chat.organization.providerCannotPublish");
  }
  return blockers;
}
