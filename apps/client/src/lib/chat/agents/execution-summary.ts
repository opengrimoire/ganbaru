import type { ModelOptionSelection, ProviderModel } from "$lib/chat/contracts";
import { isKnownModelOption, modelOptionRole } from "$lib/chat/composer/model-picker-model";

/** Effort and speed a policy's model options select, or null when the options do not say. */
export interface AgentExecutionSummary {
  effort: string | null;
  speed: "standard" | "fast" | null;
}

/**
 * Derives stable agent policy summaries from provider model options.
 */
export function agentExecutionSummary(
  options: readonly ModelOptionSelection[],
  model: ProviderModel | null,
): AgentExecutionSummary {
  const effortDefinition = model?.options.find((option) => (
    isKnownModelOption(option) && modelOptionRole(option) === "effort"
  ));
  const effortSelection = options.find((option) => (
    option.key === effortDefinition?.key || /effort|reasoning/iu.test(option.key)
  ));
  const effort = effortSelection?.value.kind === "choice"
    ? effortSelection.value.value
    : null;

  const speedDefinition = model?.options.find((option) => (
    isKnownModelOption(option) && modelOptionRole(option) === "speed"
  ));
  const speedSelection = options.find((option) => (
    option.key === speedDefinition?.key
      || /speed|fast|service[_ ]?tier/iu.test(option.key)
  ))?.value;
  if (speedSelection?.kind === "boolean") {
    return { effort, speed: speedSelection.value ? "fast" : "standard" };
  }
  if (speedSelection?.kind !== "choice") return { effort, speed: null };
  const choice = speedDefinition?.kind === "choice"
    ? speedDefinition.options.find((entry) => entry.value === speedSelection.value)
    : null;
  const identity = `${choice?.value ?? speedSelection.value} ${choice?.label ?? ""}`.toLowerCase();
  if (identity.includes("fast") || identity.includes("priority")) {
    return { effort, speed: "fast" };
  }
  if (identity.includes("standard") || identity.includes("default") || identity.includes("normal")) {
    return { effort, speed: "standard" };
  }
  return { effort, speed: null };
}
