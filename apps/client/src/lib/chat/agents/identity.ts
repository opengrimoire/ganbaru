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
  const policy = agents.find((agent) => agent.participant.id === participantId)
    ?.latestPolicy;
  if (!policy || !settings) return null;
  const provider = settings.providerInstances.find((candidate) => (
    candidate.configuration.instanceId === policy.providerInstanceId
  ));
  if (!provider) return null;
  return chatModelParticipant(
    provider.configuration.familyId,
    policy.modelId,
    provider.modelCatalog,
  );
}
