import type {
  ChatAiAgentRead,
  ProviderInstanceRead,
  ProviderModel,
  ProviderUsageLimitsObservation,
} from "$lib/chat/contracts";
import { CHAT_LOCAL_EXECUTION } from "./agent-setup";

/**
 * Whether an agent can take work, as its identity card shows it with one dot and one line of text: available, archived,
 * waiting for provider setup on this device, or with no linked desktop that can run it.
 */
export type AgentCardAvailability = "available" | "archived" | "needs_setup" | "no_linked_desktop";

/** Where an agent's card reads its model catalog and plan limits from, and whether the agent can take work. */
export interface AgentCardStatus {
  availability: AgentCardAvailability;
  /** The models of the provider that runs the agent: this device's catalog, or what the linked desktop shares. */
  models: readonly ProviderModel[];
  /** The newest plan limits of that provider, null when none were seen. */
  usageLimits: ProviderUsageLimitsObservation | null;
}

/**
 * Derives what an agent's identity card shows from the provider that runs the agent. A device with local execution
 * reads its own provider installation, so the agent is available only when that installation is healthy. A device
 * without it, such as a phone, reads what the linked desktop that would run the agent shares, so the agent is
 * available when that desktop can run the provider.
 *
 * @param agent - The agent, or null for a participant whose agent read has not loaded.
 * @param localProvider - This device's installation of the agent's provider, null when it has none.
 * @param archived - Whether the agent's participant is archived.
 * @param localExecution - Whether this device runs agent work itself.
 * @returns The availability with the catalog and plan limits the card shows.
 */
export function agentCardStatus(
  agent: ChatAiAgentRead | null,
  localProvider: ProviderInstanceRead | null,
  archived: boolean,
  localExecution = CHAT_LOCAL_EXECUTION,
): AgentCardStatus {
  if (localExecution) {
    return {
      availability: archived ? "archived" : agent?.configurationState === "healthy" ? "available" : "needs_setup",
      models: localProvider?.modelCatalog?.models ?? [],
      usageLimits: localProvider?.usageLimits ?? null,
    };
  }
  const linked = agent?.linkedProviderStatus ?? null;
  return {
    availability: archived ? "archived" : linked?.available ? "available" : "no_linked_desktop",
    models: linked?.models ?? [],
    usageLimits: linked?.usageLimits ?? null,
  };
}
