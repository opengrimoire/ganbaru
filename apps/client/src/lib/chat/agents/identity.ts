import type {
  ChatAiAgentRead,
  ChatParticipantId,
  ChatSettingsRead,
} from "$lib/chat/contracts";
import { chatModelParticipant, type ChatModelParticipant } from "./participant-identity";

/** Resolves the current model identity used as an AI agent's shared avatar. */
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
  return chatModelParticipant(
    provider.configuration.familyId,
    agent.latestPolicy?.modelId ?? null,
    provider.modelCatalog,
  );
}
