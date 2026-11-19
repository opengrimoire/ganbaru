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
import { compareCompanyModels, modelCompany } from "$lib/chat/composer/model-company";
import { familyModelLines, newestModelOfLine } from "./agent-names";

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
 * Orders the catalog `models` of `familyId` as Settings > Agents lists them: by company, then by the company's model
 * hierarchy, newest version first, then by display name.
 *
 * @param compareNames Orders display names the hierarchy leaves tied, in the reader's locale.
 */
export function sortCatalogModels<Model extends ProviderModel>(
  familyId: ProviderFamilyId,
  models: readonly Model[],
  compareNames: (left: string, right: string) => number,
): Model[] {
  return [...models].sort((left, right) => {
    const leftCompany = modelCompany(familyId, left);
    const rightCompany = modelCompany(familyId, right);
    return leftCompany.order - rightCompany.order
      || (leftCompany.id === rightCompany.id ? compareCompanyModels(leftCompany.id, left, right) : 0)
      || compareNames(left.displayName, right.displayName);
  });
}

/** A model a custom agent can run, as the Custom agent dialog offers it. */
export interface CustomAgentModelChoice {
  /** The model id the agent stores: a model line, such as `opus`, or a catalog model id. */
  modelId: ModelId;
  /** The model line the choice follows, or `null` for one fixed catalog model. */
  line: string | null;
  /** The catalog model the choice runs now, whose options the dialog offers. */
  model: ProviderModel;
}

/**
 * The models a custom agent of `familyId` can run, in Settings > Agents order: each model line the catalog offers a
 * model of, strongest first, which always runs the line's newest model, then every listed catalog model.
 *
 * @param models The provider's catalog models that are still offered.
 * @param compareNames Orders display names the model hierarchy leaves tied, in the reader's locale.
 */
export function customAgentModelChoices(
  familyId: ProviderFamilyId,
  models: readonly ProviderModel[],
  compareNames: (left: string, right: string) => number,
): CustomAgentModelChoice[] {
  const lineChoices = familyModelLines(familyId).flatMap((line) => {
    const model = newestModelOfLine(familyId, line, models);
    return model ? [{ modelId: line, line, model }] : [];
  });
  const catalogChoices = sortCatalogModels(familyId, listedCatalogModels(familyId, models), compareNames)
    .map((model) => ({ modelId: model.id, line: null, model }));
  return [...lineChoices, ...catalogChoices];
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
