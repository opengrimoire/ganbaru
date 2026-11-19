import type { ChatAiAgentRead, ProviderFamilyId, ProviderModel } from "$lib/chat/contracts";
import { compareCompanyModels, modelCompany } from "$lib/chat/composer/model-company";

/**
 * The model lines each provider family offers as built-in agents, strongest first. The vault creates one agent per
 * line, such as `@Opus` or `@Sol`, that always runs the line's newest model. Mythos, which only some accounts can
 * run, gets its agent once a device's catalog offers it.
 */
const MODEL_LINES: Readonly<Partial<Record<ProviderFamilyId, readonly string[]>>> = {
  claude: ["mythos", "fable", "opus", "sonnet", "haiku"],
  codex: ["astra", "sol", "terra", "luna"],
};

/** The longest name a named agent can carry, in characters. */
export const MAX_AGENT_NAME_CHARACTERS = 32;

/** Why a name cannot name a new agent. */
export type AgentNameProblem = "empty" | "tooLong" | "characters" | "taken";

/** The model lines the provider family `familyId` offers as built-in agents, strongest first. */
export function familyModelLines(familyId: ProviderFamilyId): readonly string[] {
  return MODEL_LINES[familyId] ?? [];
}

/** Whether `name` is a built-in model line of any family, ignoring case. */
export function isModelLineName(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return Object.values(MODEL_LINES).some((lines) => lines?.includes(normalized));
}

/**
 * Checks a name for a new named agent the way the vault does: trimmed, at most {@link MAX_AGENT_NAME_CHARACTERS}
 * characters, no spaces, control characters, or `@`, and different, ignoring case, from every model line and from
 * `takenNames`, which should hold every agent's name, archived ones included, and every provider family's id and name.
 *
 * @param name The name as typed.
 * @param takenNames The names other agents already carry.
 * @returns The first problem, or `null` when the name can be used.
 */
export function agentNameProblem(name: string, takenNames: readonly string[]): AgentNameProblem | null {
  const trimmed = name.trim();
  if (!trimmed) return "empty";
  if ([...trimmed].length > MAX_AGENT_NAME_CHARACTERS) return "tooLong";
  if (/[\s\p{Cc}@]/u.test(trimmed)) return "characters";
  const normalized = trimmed.toLowerCase();
  if (isModelLineName(trimmed) || takenNames.some((taken) => taken.trim().toLowerCase() === normalized)) return "taken";
  return null;
}

/**
 * The model a built-in line agent runs now: the newest model of `models` whose name or id holds the line as a word.
 * Dispatch makes the same choice on the executing device.
 *
 * @param familyId The provider family that serves `models`.
 * @param line A model line such as `opus`.
 * @param models The provider's model catalog.
 */
export function newestModelOfLine(
  familyId: ProviderFamilyId,
  line: string,
  models: readonly ProviderModel[],
): ProviderModel | null {
  const ranked = models
    .filter((model) => (
      model.availability !== "deprecated"
      && model.availability !== "unavailable"
      && `${model.displayName} ${model.id}`.toLowerCase().split(/[^a-z0-9]+/u).includes(line)
    ))
    .map((model, index) => ({ model, index }))
    .sort((left, right) => (
      compareCompanyModels(modelCompany(familyId, left.model).id, left.model, right.model)
      || left.model.id.length - right.model.id.length
      || left.index - right.index
    ));
  return ranked[0]?.model ?? null;
}

/** The model line a built-in line agent runs, read from its fixed name, or `null` for any other agent. */
export function agentModelLine(agent: Pick<ChatAiAgentRead, "agentKind" | "providerFamilyId" | "participant">): string | null {
  if (agent.agentKind !== "line") return null;
  const line = agent.participant.displayName.trim().toLowerCase();
  return familyModelLines(agent.providerFamilyId).includes(line) ? line : null;
}
