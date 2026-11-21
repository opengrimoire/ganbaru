import type {
  ChatAiAgentRead,
  ChatParticipantId,
  ChatSettingsRead,
} from "$lib/chat/contracts";
import { agentModelLine, newestModelOfLine } from "./agent-names";
import { CHAT_LOCAL_EXECUTION } from "./agent-setup";
import { modelCompanyForIdentity, type ModelCompanyIdentity } from "$lib/chat/composer/model-company";
import { chatModelParticipant, type ChatModelParticipant } from "./participant-identity";

/**
 * Resolves the current model identity used as an AI agent's shared avatar; a model line agent shows its line's newest
 * model. A device without local execution, such as a phone, reads the catalog the linked desktop that would run the
 * agent shares.
 */
export function chatAgentModelParticipant(
  participantId: ChatParticipantId,
  agents: readonly ChatAiAgentRead[],
  settings: ChatSettingsRead | null,
  localExecution = CHAT_LOCAL_EXECUTION,
): ChatModelParticipant | null {
  const agent = agents.find((entry) => entry.participant.id === participantId);
  if (!agent) return null;
  const provider = agent.localProviderInstanceId
    ? settings?.providerInstances.find((candidate) => (
      candidate.configuration.instanceId === agent.localProviderInstanceId
    ))
    : undefined;
  const familyId = provider?.configuration.familyId ?? agent.providerFamilyId;
  const models = provider
    ? provider.modelCatalog?.models ?? []
    : localExecution ? null : agent.linkedProviderStatus?.models ?? null;
  if (!models) return null;
  const line = agentModelLine(agent);
  const modelId = line
    ? newestModelOfLine(familyId, line, models)?.id ?? null
    : agent.latestPolicy?.modelId ?? null;
  return chatModelParticipant(familyId, modelId, { models: [...models] });
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
