import { modelOptionRole } from "$lib/chat/composer/model";
import type { ModelOptionDefinition, ModelOptionSelection, ModelOptionValue } from "$lib/chat/contracts";

/** A model choice option, such as the reasoning effort or a service tier. */
export type ChoiceModelOption = Extract<ModelOptionDefinition, { kind: "choice" }>;
/** A model on or off option, such as a fast mode switch. */
export type BooleanModelOption = Extract<ModelOptionDefinition, { kind: "boolean" }>;

/** The options a custom agent offers as droplists: the reasoning effort and the speed, when the model has them. */
export interface CustomAgentControls {
  reasoning: ChoiceModelOption | null;
  speed: ChoiceModelOption | BooleanModelOption | null;
}

/** Finds the reasoning and speed options among a model's option `definitions`. */
export function customAgentControls(definitions: readonly ModelOptionDefinition[]): CustomAgentControls {
  let reasoning: ChoiceModelOption | null = null;
  let speed: ChoiceModelOption | BooleanModelOption | null = null;
  for (const definition of definitions) {
    if (definition.kind !== "choice" && definition.kind !== "boolean") continue;
    const role = modelOptionRole(definition);
    if (role === "effort" && definition.kind === "choice" && !reasoning) reasoning = definition;
    if (role === "speed" && !speed) speed = definition;
  }
  return { reasoning, speed };
}

/** `selections` with the option `key` set to `value`, replacing an earlier value for the same key. */
export function withModelOption(
  selections: readonly ModelOptionSelection[],
  key: string,
  value: ModelOptionValue,
): ModelOptionSelection[] {
  return [...selections.filter((selection) => selection.key !== key), { key, value }];
}

/** The value `selections` give the option `key`, or `null` when they give none. */
export function selectedModelOption(
  selections: readonly ModelOptionSelection[],
  key: string,
): ModelOptionValue | null {
  return selections.find((selection) => selection.key === key)?.value ?? null;
}
