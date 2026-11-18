import type {
  ChatAgentPolicyRead,
  ChatAiAgentRead,
  ModelId,
  ModelOptionDefinition,
  ModelOptionSelection,
  ProviderFamilyId,
  ProviderModel,
} from "$lib/chat/contracts";
import { modelOptionRole } from "$lib/chat/composer/model";
import { familyModelLines } from "./agent-names";

/** The fields of an agent the roster reads. */
export type RosterAgent = Pick<ChatAiAgentRead, "agentKind" | "providerFamilyId" | "participant"> & {
  latestPolicy: Pick<ChatAgentPolicyRead, "modelId"> | null;
};

/** The agents of one provider family as Settings > Agents lists them. */
export interface FamilyAgentRoster<Agent extends RosterAgent> {
  /** The built-in agents of the family's model lines, such as `@Opus`, strongest line first. */
  lineAgents: Agent[];
  /** The agents a person named, by name. */
  namedAgents: Agent[];
  /** The agent of each catalog model, such as `@Opus 5.5`, by the model it runs. */
  modelAgents: ReadonlyMap<ModelId, Agent>;
}

/**
 * Splits the agents of `familyId` into line, named, and model agents.
 *
 * @param familyId The provider family.
 * @param agents Every known agent, archived ones included.
 * @param compareNames Orders named agents by display name, in the reader's locale.
 */
export function familyAgentRoster<Agent extends RosterAgent>(
  familyId: ProviderFamilyId,
  agents: readonly Agent[],
  compareNames: (left: string, right: string) => number,
): FamilyAgentRoster<Agent> {
  const lines = familyModelLines(familyId);
  const lineIndex = (agent: Agent): number => {
    const index = lines.indexOf(agent.participant.displayName.trim().toLowerCase());
    return index === -1 ? lines.length : index;
  };
  const family = agents.filter((agent) => agent.providerFamilyId === familyId);
  const modelAgents = new Map<ModelId, Agent>();
  for (const agent of family) {
    const modelId = agent.latestPolicy?.modelId;
    if (agent.agentKind === "model" && modelId && !modelAgents.has(modelId)) modelAgents.set(modelId, agent);
  }
  return {
    lineAgents: family.filter((agent) => agent.agentKind === "line").sort((left, right) => lineIndex(left) - lineIndex(right)),
    namedAgents: family
      .filter((agent) => agent.agentKind === "custom")
      .sort((left, right) => compareNames(left.participant.displayName, right.participant.displayName)),
    modelAgents,
  };
}

/**
 * The catalog models Settings > Agents lists as model agents. A model whose id is a model line of the family, such as
 * the `opus` alias, is left out, since the line agent already runs it.
 */
export function listedCatalogModels<Model extends Pick<ProviderModel, "id">>(
  familyId: ProviderFamilyId,
  models: readonly Model[],
): Model[] {
  const lines = familyModelLines(familyId);
  return models.filter((model) => !lines.includes(model.id.toLowerCase()));
}

/**
 * The label of the effort `selections` choose for a model with option `definitions`, or `null` when they choose none
 * or the model has no effort option.
 */
export function selectedEffortLabel(
  selections: readonly ModelOptionSelection[],
  definitions: readonly ModelOptionDefinition[],
): string | null {
  for (const definition of definitions) {
    if (definition.kind !== "choice" || modelOptionRole(definition) !== "effort") continue;
    const selected = selections.find((selection) => selection.key === definition.key)?.value;
    if (selected?.kind !== "choice") return null;
    return definition.options.find((option) => option.value === selected.value)?.label ?? selected.value;
  }
  return null;
}
