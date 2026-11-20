import type { ChatAgentConfigurationState } from "$lib/chat/contracts";
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
