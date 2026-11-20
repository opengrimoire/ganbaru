import type {
  ChatAiAgentRead,
  ChatParticipantId,
  ChatSettingsRead,
} from "$lib/chat/contracts";
import { agentModelLine, newestModelOfLine } from "./agent-names";
import { modelCompanyForIdentity, type ModelCompanyIdentity } from "$lib/chat/composer/model-company";
import { chatModelParticipant, type ChatModelParticipant } from "./participant-identity";

/** Resolves the current model identity used as an AI agent's shared avatar; a model line agent shows its line's newest model. */
export function chatAgentModelParticipant(
  participantId: ChatParticipantId,
  agents: readonly ChatAiAgentRead[],
  settings: ChatSettingsRead | null,
): ChatModelParticipant | null {
  const agent = agents.find((entry) => entry.participant.id === participantId);
  if (!agent || !settings) return null;
  const provider = agent.localProviderInstanceId
    ? settings.providerInstances.find((candidate) => (
      candidate.configuration.instanceId === agent.localProviderInstanceId
    ))
    : undefined;
  if (!provider) return null;
  const line = agentModelLine(agent);
  const modelId = line
    ? newestModelOfLine(provider.configuration.familyId, line, provider.modelCatalog?.models ?? [])?.id ?? null
    : agent.latestPolicy?.modelId ?? null;
  return chatModelParticipant(provider.configuration.familyId, modelId, provider.modelCatalog);
}

/**
 * Resolves the company whose mark an AI agent shows as its avatar. A device without the agent's provider, such as a
 * phone, falls back to the agent's synced provider family and model.
 *
 * @param participantId - The agent's participant.
 * @param agents - Known agents, archived ones included.
 * @param settings - This device's Chat settings, null before they load.
 * @returns The company identity, or null for an unknown agent.
 */
export function chatAgentCompany(
  participantId: ChatParticipantId,
  agents: readonly ChatAiAgentRead[],
  settings: ChatSettingsRead | null,
): ModelCompanyIdentity | null {
  const local = chatAgentModelParticipant(participantId, agents, settings);
  if (local) return local.company;
  const agent = agents.find((entry) => entry.participant.id === participantId);
  return agent ? modelCompanyForIdentity(agent.providerFamilyId, agent.latestPolicy?.modelId ?? null) : null;
}
