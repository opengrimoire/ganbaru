import type {
  ChatAiAgentRead,
  ChatParticipantId,
  ChatSettingsRead,
} from "$lib/chat/contracts";
import { agentModelLine, newestModelOfLine } from "./agent-names";
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
