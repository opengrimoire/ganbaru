import type { ChatAgentConfigurationState, ProviderInstanceId, ProviderInstanceRead } from "$lib/chat/contracts";
import { BUILD_PLATFORM_PROFILE, platformHasCapability } from "$lib/platform";

/** Whether this build runs agent work itself; without it a linked desktop runs the work this device posts. */
export const CHAT_LOCAL_EXECUTION = platformHasCapability(BUILD_PLATFORM_PROFILE, "chat.local-execution");

/**
 * Whether an agent in `state` needs provider setup on this device before its work can start. A device without local
 * execution never does, since a linked desktop runs the work.
 */
export function agentNeedsSetupHere(state: ChatAgentConfigurationState, localExecution = CHAT_LOCAL_EXECUTION): boolean {
  return localExecution && state !== "healthy";
}

/**
 * The provider installations in `providers` that can run work on this device: enabled, with an executable, and with no
 * runtime values left to confirm. This mirrors the readiness the native side checks before it starts queued work.
 */
export function readyProviderInstances(providers: readonly ProviderInstanceRead[]): Set<ProviderInstanceId> {
  return new Set(providers
    .filter((provider) =>
      provider.configuration.enabled
      && provider.configuration.executable.trim() !== ""
      && provider.runtimeReview.length === 0)
    .map((provider) => provider.configuration.instanceId));
}

/** Whether `next` holds a ready provider installation that `previous` lacks, so waiting work may now start here. */
export function providerBecameReady(
  previous: ReadonlySet<ProviderInstanceId>,
  next: ReadonlySet<ProviderInstanceId>,
): boolean {
  for (const instanceId of next) if (!previous.has(instanceId)) return true;
  return false;
}
