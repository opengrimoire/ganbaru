import type {
  ChatAgentAccess,
  ChatAgentAccessSource,
  ChatAgentPolicyRead,
  ChatAgentResolvedAccessRead,
  ChatEffectiveAgentAccess,
  ChatParticipantId,
  ModelId,
  ProviderModel,
} from "$lib/chat/contracts";
import { CHAT_AGENT_ACCESS_LEVELS } from "$lib/chat/contracts";
import type { Translate } from "$lib/i18n/translator.svelte";

/** The access every scope inherits until the app settings choose another. */
export const CHAT_DEFAULT_AGENT_ACCESS: ChatAgentAccess = "ask";

/** The longest instructions the app, a project, or a channel may store, in characters. */
export const CHAT_AGENT_INSTRUCTIONS_LIMIT = 16384;

/** One option of an access `Select`. */
export interface ChatAccessOption {
  value: ChatAgentAccess;
  label: string;
  description: string;
  disabled: boolean;
}

/** What an access `Select` needs to know about the scope it edits. */
export interface ChatAccessOptionsContext {
  /** Whether Work on their own may be chosen at this scope on this device. */
  ownAvailable: boolean;
  /** Whether the default level carries its "(default)" suffix, as the app scope shows it. */
  markDefault: boolean;
}

/** Whether an agent working at `access` may edit files, and so may use folders and scratch for edits. */
export function effectiveAccessEditsFiles(access: ChatEffectiveAgentAccess | null): boolean {
  return access === "edit_only" || access === "ask" || access === "own";
}

/**
 * Whether a channel may choose Work on their own on this device. It stays available while no installed agent is
 * in the channel, since another device may run them, and otherwise needs one installed agent that can confine
 * its commands here.
 */
export function ownAccessAvailable(
  agents: readonly Pick<ChatAgentResolvedAccessRead, "installed" | "ownOffered">[] | null,
): boolean {
  if (!agents) return true;
  const installed = agents.filter((agent) => agent.installed);
  return installed.length === 0 || installed.some((agent) => agent.ownOffered);
}

/** The four access levels as `Select` options, narrowest first, with their one-sentence descriptions. */
export function chatAccessOptions(t: Translate, context: ChatAccessOptionsContext): ChatAccessOption[] {
  return CHAT_AGENT_ACCESS_LEVELS.map((value) => {
    const label = t(`chat.agentAccess.levels.${value}`);
    return {
      value,
      label: context.markDefault && value === CHAT_DEFAULT_AGENT_ACCESS ? t("chat.agentAccess.defaultLevel", label) : label,
      description: t(`chat.agentAccess.descriptions.${value}`),
      disabled: value === "own" && !context.ownAvailable,
    };
  });
}

/** Where a resolved access comes from, as the line under an access `Select` names it. */
export function chatAccessSourceLabel(t: Translate, source: ChatAgentAccessSource): string {
  return t(`chat.agentAccess.sources.${source}`);
}

/** A settings scope that may inherit its access: a channel from its project, a project from the app. */
export type ChatInheritingAgentScope = "channel" | "project";

/**
 * The source line under a scope's access `Select`: where the shown access comes from while the scope inherits,
 * else that the scope sets its own.
 */
export function chatScopeSourceLine(
  t: Translate,
  scope: ChatInheritingAgentScope,
  access: ChatAgentAccess | null,
  inheritedSource: ChatAgentAccessSource,
): string {
  if (access === null) return chatAccessSourceLabel(t, inheritedSource);
  return scope === "channel" ? chatAccessSourceLabel(t, "channel") : t("chat.agentAccess.setForProject");
}

/** The single line that says what one agent will actually do in a channel on this device. */
export function chatAgentBehaviorLine(
  t: Translate,
  agent: Pick<ChatAgentResolvedAccessRead, "effective" | "downgradeReason" | "installed">,
  providerName: string,
): string {
  if (!agent.installed) return t("chat.agentAccess.notInstalled");
  if (agent.effective === null) {
    return agent.downgradeReason
      ? t("chat.agentAccess.unavailable", providerName, t(`chat.agentAccess.reasons.${agent.downgradeReason}`))
      : t("chat.agentAccess.unavailableNoReason");
  }
  if (agent.downgradeReason) {
    return t(
      "chat.agentAccess.downgraded",
      t(`chat.agentAccess.behaviorHere.${agent.effective}`),
      providerName,
      t(`chat.agentAccess.reasons.${agent.downgradeReason}`),
    );
  }
  return t(`chat.agentAccess.behavior.${agent.effective}`);
}

/** The models of a provider catalog that name an agent's default model. */
export interface ChatAgentModelCatalog {
  models: readonly Pick<ProviderModel, "id" | "displayName">[];
  recommendedModelId: ModelId | null;
}

/**
 * The model an agent runs, as dispatch resolves it: its own model while the catalog offers it, else the
 * catalog's recommended model. Without a catalog on this device the agent's own model runs as named, or `null`
 * when the provider picks.
 */
export function chatAgentDefaultModelId(
  policy: Pick<ChatAgentPolicyRead, "providerManagedModel" | "modelId"> | null,
  catalog: ChatAgentModelCatalog | null,
): ModelId | null {
  const modelId = policy && !policy.providerManagedModel ? policy.modelId : null;
  if (!catalog) return modelId;
  if (modelId && catalog.models.some((model) => model.id === modelId)) return modelId;
  return catalog.recommendedModelId;
}

/** The model an agent runs, by display name. */
export function chatAgentDefaultModelName(
  t: Translate,
  policy: Pick<ChatAgentPolicyRead, "providerManagedModel" | "modelId"> | null,
  catalog: ChatAgentModelCatalog | null,
): string {
  const modelId = chatAgentDefaultModelId(policy, catalog);
  if (!modelId) return t("chat.agentAccess.providerDefault");
  return catalog?.models.find((model) => model.id === modelId)?.displayName ?? modelId;
}

