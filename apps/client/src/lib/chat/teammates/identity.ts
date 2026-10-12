import type {
  ChatAiTeammateRead,
  ChatParticipantId,
  ChatSettingsRead,
} from "$lib/chat/contracts";
import { chatModelParticipant, type ChatModelParticipant } from "./participant-identity";

/** Resolves the current model identity used as an AI teammate's shared avatar. */
export function chatTeammateModelParticipant(
  participantId: ChatParticipantId,
  teammates: readonly ChatAiTeammateRead[],
  settings: ChatSettingsRead | null,
): ChatModelParticipant | null {
  const policy = teammates.find((teammate) => teammate.participant.id === participantId)
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
